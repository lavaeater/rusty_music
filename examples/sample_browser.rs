//! sample_browser — a tiny TUI for auditioning audio samples anywhere on disk.
//!
//! Point it at any folder, walk around, and hit Space to hear a sample. Mark the
//! ones you like into a "pool" (favourites) and save it to a TOML file you can
//! feed into music_machine or your game later.
//!
//! Usage:
//!   cargo run --example sample_browser                 # start in $HOME
//!   cargo run --example sample_browser -- ~/samples    # start in a folder
//!   cargo run --example sample_browser -- ~/samples my_pool.toml
//!
//! Controls:
//!   ↑/↓            move selection
//!   Enter / →      enter folder
//!   Backspace / ←  go up a folder
//!   [Space]        audition the highlighted sample
//!   [R]            toggle recursive "grouped by folder" view
//!   [A]            add highlighted sample to the pool
//!   [Tab]          switch focus between Browser and Pool
//!   [D]/[Del]      remove from pool (Pool focus) / un-favourite (Browser focus)
//!   [S]/[L]        save / load the pool TOML
//!   [Q]/Esc        quit

use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::app::ScheduleRunnerPlugin;
use bevy::asset::io::AssetSourceBuilder;
use bevy::asset::{AssetApp, AssetPlugin};
use bevy::prelude::*;
use bevy_seedling::prelude::{AudioSample, PlaybackSettings, SamplePlayer, SeedlingPlugin, Volume};

use ratatui::crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Terminal,
};

#[path = "common/mod.rs"]
mod common;
use common::{disk_asset_path, EntryKind, FsBrowser, SamplePool};

const DEFAULT_POOL_FILE: &str = "sample_pool.toml";

#[derive(PartialEq, Eq, Clone, Copy)]
enum Focus {
    Browser,
    Pool,
}

#[derive(Resource)]
struct AppState {
    browser: FsBrowser,
    pool: SamplePool,
    pool_selected: usize,
    pool_file: PathBuf,
    focus: Focus,
    status: String,
    /// Entity of the currently-playing audition, so we can cut it off.
    audition: Option<Entity>,
    last_draw: Option<Instant>,
}

#[derive(Resource)]
struct TuiTerminal(Terminal<CrosstermBackend<Stdout>>);

struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Input
// ─────────────────────────────────────────────────────────────────────────────

fn handle_input(
    mut state: ResMut<AppState>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    while event::poll(Duration::ZERO).unwrap_or(false) {
        let Ok(Event::Key(key)) = event::read() else { continue; };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                let _ = disable_raw_mode();
                let _ = execute!(io::stdout(), LeaveAlternateScreen);
                std::process::exit(0);
            }
            KeyCode::Tab => {
                state.focus = match state.focus {
                    Focus::Browser => Focus::Pool,
                    Focus::Pool => Focus::Browser,
                };
            }
            KeyCode::Up => match state.focus {
                Focus::Browser => state.browser.move_up(),
                Focus::Pool => {
                    state.pool_selected = state.pool_selected.saturating_sub(1);
                }
            },
            KeyCode::Down => match state.focus {
                Focus::Browser => state.browser.move_down(),
                Focus::Pool => {
                    let max = state.pool.len().saturating_sub(1);
                    state.pool_selected = (state.pool_selected + 1).min(max);
                }
            },
            KeyCode::Enter | KeyCode::Right if state.focus == Focus::Browser => {
                state.browser.enter_selected();
            }
            KeyCode::Backspace | KeyCode::Left if state.focus == Focus::Browser => {
                state.browser.go_up();
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                state.browser.toggle_recursive();
                let mode = if state.browser.recursive { "recursive (grouped)" } else { "folder" };
                state.status = format!("View: {mode}");
            }
            KeyCode::Char(' ') => audition(&mut state, &mut commands, &asset_server),
            KeyCode::Char('a') | KeyCode::Char('A') => add_selected_to_pool(&mut state),
            KeyCode::Char('d') | KeyCode::Char('D') | KeyCode::Delete => remove_from_pool(&mut state),
            KeyCode::Char('s') | KeyCode::Char('S') => {
                let file = state.pool_file.clone();
                match state.pool.save(&file) {
                    Ok(_) => state.status = format!("Saved {} samples → {}", state.pool.len(), file.display()),
                    Err(e) => state.status = format!("Save error: {e}"),
                }
            }
            KeyCode::Char('l') | KeyCode::Char('L') => {
                let file = state.pool_file.clone();
                match SamplePool::load(&file) {
                    Ok(p) => {
                        state.status = format!("Loaded {} samples from {}", p.len(), file.display());
                        state.pool = p;
                        state.pool_selected = 0;
                    }
                    Err(e) => state.status = format!("Load error: {e}"),
                }
            }
            _ => {}
        }
    }
}

/// The path of the sample currently relevant to the focused pane, if any.
fn focused_sample_path(state: &AppState) -> Option<PathBuf> {
    match state.focus {
        Focus::Browser => state
            .browser
            .selected_entry()
            .filter(|e| e.is_file())
            .map(|e| e.path.clone()),
        Focus::Pool => state.pool.paths().get(state.pool_selected).cloned(),
    }
}

fn audition(state: &mut AppState, commands: &mut Commands, asset_server: &AssetServer) {
    let Some(path) = focused_sample_path(state) else {
        state.status = "Nothing to play here — highlight a sample.".into();
        return;
    };
    if let Some(prev) = state.audition.take() {
        commands.entity(prev).try_despawn();
    }
    let handle = asset_server.load::<AudioSample>(disk_asset_path(&path));
    let entity = commands
        .spawn((
            SamplePlayer::new(handle).with_volume(Volume::Decibels(0.0)),
            PlaybackSettings::default(),
        ))
        .id();
    state.audition = Some(entity);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    state.status = format!("▶ {name}");
}

fn add_selected_to_pool(state: &mut AppState) {
    let Some(entry) = state.browser.selected_entry() else { return; };
    if entry.kind != EntryKind::File {
        state.status = "Highlight a sample (not a folder) to add.".into();
        return;
    }
    let path = entry.path.clone();
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if state.pool.add(path) {
        state.status = format!("＋ added '{name}' to pool ({} total)", state.pool.len());
    } else {
        state.status = format!("'{name}' is already in the pool");
    }
}

fn remove_from_pool(state: &mut AppState) {
    let target = match state.focus {
        Focus::Pool => state.pool.paths().get(state.pool_selected).cloned(),
        Focus::Browser => state
            .browser
            .selected_entry()
            .filter(|e| e.is_file())
            .map(|e| e.path.clone()),
    };
    let Some(path) = target else { return; };
    if state.pool.remove(&path) {
        let max = state.pool.len().saturating_sub(1);
        state.pool_selected = state.pool_selected.min(max);
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        state.status = format!("－ removed '{name}' from pool ({} left)", state.pool.len());
    }
}

/// Clean up audition entities once their sample has finished, so they don't pile up.
fn reap_audition(mut state: ResMut<AppState>, players: Query<(), With<SamplePlayer>>) {
    if let Some(e) = state.audition {
        if players.get(e).is_err() {
            state.audition = None;
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Rendering
// ─────────────────────────────────────────────────────────────────────────────

fn draw_tui(mut terminal: ResMut<TuiTerminal>, mut state: ResMut<AppState>) {
    let now = Instant::now();
    if state.last_draw.map_or(false, |t| now.duration_since(t).as_millis() < 33) {
        return;
    }
    state.last_draw = Some(now);

    let _ = terminal.0.draw(|frame| {
        let area = frame.area();
        let rows = Layout::vertical([
            Constraint::Length(3), // header
            Constraint::Min(8),    // body
            Constraint::Length(4), // footer
        ])
        .split(area);

        // Header
        let view = if state.browser.recursive { "grouped" } else { "folder" };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" SAMPLE BROWSER ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw("│ "),
                Span::styled(state.browser.dir.to_string_lossy().into_owned(), Style::default().fg(Color::Cyan)),
                Span::raw("  ["),
                Span::styled(view, Style::default().fg(Color::Magenta)),
                Span::raw(" view]"),
            ]))
            .block(Block::new().borders(Borders::ALL)),
            rows[0],
        );

        // Body: browser | pool
        let cols = Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)]).split(rows[1]);

        let browser_focused = state.focus == Focus::Browser;
        let items: Vec<ListItem> = state
            .browser
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let selected = i == state.browser.selected && browser_focused;
                let indent = "  ".repeat(e.depth);
                let in_pool = e.is_file() && state.pool.contains(&e.path);
                let (icon, color) = match e.kind {
                    EntryKind::Header => ("", Color::DarkGray),
                    EntryKind::Parent => ("⮤ ", Color::Yellow),
                    EntryKind::Dir => ("📁 ", Color::Yellow),
                    EntryKind::File => (if in_pool { "★ " } else { "🔉 " }, if in_pool { Color::Green } else { Color::White }),
                };
                let mut style = Style::default().fg(color);
                if e.kind == EntryKind::Header {
                    style = style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
                }
                if selected {
                    style = style.add_modifier(Modifier::BOLD).bg(Color::DarkGray);
                }
                ListItem::new(Line::from(Span::styled(format!(" {indent}{icon}{}", e.label), style)))
            })
            .collect();
        let mut bstate = ListState::default().with_selected(Some(state.browser.selected));
        let btitle = if browser_focused { " Browser ◀ " } else { " Browser " };
        frame.render_stateful_widget(
            List::new(items).block(Block::new().borders(Borders::ALL).title(btitle)),
            cols[0],
            &mut bstate,
        );

        // Pool
        let pool_focused = state.focus == Focus::Pool;
        let pool_items: Vec<ListItem> = state
            .pool
            .paths()
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let selected = i == state.pool_selected && pool_focused;
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let mut style = Style::default().fg(Color::Green);
                if selected {
                    style = style.add_modifier(Modifier::BOLD).bg(Color::DarkGray);
                }
                ListItem::new(Line::from(Span::styled(format!(" ★ {name}"), style)))
            })
            .collect();
        let mut pstate = ListState::default().with_selected(Some(state.pool_selected));
        let ptitle = if pool_focused {
            format!(" Pool ({}) ◀ ", state.pool.len())
        } else {
            format!(" Pool ({}) ", state.pool.len())
        };
        frame.render_stateful_widget(
            List::new(pool_items).block(Block::new().borders(Borders::ALL).title(ptitle)),
            cols[1],
            &mut pstate,
        );

        // Footer: status + keys
        let footer = Layout::vertical([Constraint::Length(1), Constraint::Length(3)]).split(rows[2]);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" Status: ", Style::default().fg(Color::DarkGray)),
                Span::styled(&state.status, Style::default().fg(Color::White)),
            ]))
            .block(Block::new().borders(Borders::LEFT | Borders::RIGHT | Borders::TOP)),
            footer[0],
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" [↑↓]", Style::default().fg(Color::Yellow)), Span::raw(" move  "),
                Span::styled("[Enter/→]", Style::default().fg(Color::Yellow)), Span::raw(" open  "),
                Span::styled("[⌫/←]", Style::default().fg(Color::Yellow)), Span::raw(" up  "),
                Span::styled("[Space]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)), Span::raw(" play  "),
                Span::styled("[R]", Style::default().fg(Color::Magenta)), Span::raw(" group  "),
                Span::styled("[A]", Style::default().fg(Color::Green)), Span::raw(" +pool  "),
                Span::styled("[Tab]", Style::default().fg(Color::Cyan)), Span::raw(" focus  "),
                Span::styled("[D]", Style::default().fg(Color::Red)), Span::raw(" -pool  "),
                Span::styled("[S/L]", Style::default().fg(Color::Yellow)), Span::raw(" save/load  "),
                Span::styled("[Q]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)), Span::raw(" quit"),
            ]))
            .block(Block::new().borders(Borders::ALL)),
            footer[1],
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
//  Entry point
// ─────────────────────────────────────────────────────────────────────────────

fn main() {
    // CLI: [start_dir] [pool_file]
    let mut args = std::env::args().skip(1);
    let start = args
        .next()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(FsBrowser::default_start);
    let pool_file = args.next().map(PathBuf::from).unwrap_or_else(|| PathBuf::from(DEFAULT_POOL_FILE));

    let pool = SamplePool::load(&pool_file).unwrap_or_default();
    let status = format!(
        "Browsing {} — pool has {} sample(s). Space to play, A to favourite.",
        start.display(),
        pool.len()
    );

    enable_raw_mode().expect("enable raw mode");
    execute!(io::stdout(), EnterAlternateScreen).expect("enter alternate screen");
    let _guard = TerminalGuard;
    let terminal = Terminal::new(CrosstermBackend::new(io::stdout())).expect("terminal");

    App::new()
        .add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(1))))
        // The `disk` source is rooted at `/`, letting us load any absolute path
        // on disk as `disk://<path-without-leading-slash>`.
        .register_asset_source("disk", AssetSourceBuilder::platform_default("/", None))
        .add_plugins(AssetPlugin::default())
        .add_plugins(SeedlingPlugin::default())
        .insert_resource(TuiTerminal(terminal))
        .insert_resource(AppState {
            browser: FsBrowser::new(start),
            pool,
            pool_selected: 0,
            pool_file,
            focus: Focus::Browser,
            status,
            audition: None,
            last_draw: None,
        })
        .add_systems(Update, (handle_input, reap_audition.after(handle_input), draw_tui.after(reap_audition)))
        .run();
}
