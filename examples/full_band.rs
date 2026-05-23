//! Full-band example — all rusty_music features with a ratatui terminal visualizer.
//!
//! Controls:
//!   ↑ / ↓        raise / lower intensity (0.0 – 1.0)
//!   D            toggle drummer on/off
//!   B            toggle bassist on/off
//!   A            toggle arpeggiator on/off
//!   S            toggle soloist on/off
//!   q / Esc      quit

use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use bevy::app::ScheduleRunnerPlugin;
use bevy::asset::AssetPlugin;
use bevy::prelude::*;

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
    widgets::{Block, Borders, Gauge, Paragraph},
    Terminal,
};

use rusty_music::clock::Clock;
use rusty_music::musicians::arpeggiator::Arpeggiator;
use rusty_music::musicians::bassist::Bassist;
use rusty_music::musicians::conductor::Conductor;
use rusty_music::musicians::drummer::{
    generate_double_time_kick_beat, generate_double_time_snare_beat, generate_snare_fill_beat,
    generate_half_time_kick_beat, generate_half_time_snare_beat, generate_hihat_beat,
    generate_kick_beat, generate_snare_beat, SuperDrummer,
};
use rusty_music::musicians::soloist::Soloist;
use rusty_music::musicians::{Musician, Muted, Note, Sampler};
use rusty_music::player::Intensity;
use rusty_music::{create_drummer_only, generate_chords, MusicPlugin};

// ── Resources ─────────────────────────────────────────────────────────────────

#[derive(Resource)]
struct TuiTerminal(Terminal<CrosstermBackend<Stdout>>);

/// Entity IDs so we can toggle Muted on specific musicians.
#[derive(Resource)]
struct BandEntities {
    drummer: Entity,
    bassist: Entity,
    arpeggiator: Entity,
    soloist: Entity,
}

/// Which musicians are currently muted.
#[derive(Resource, Default)]
struct MuteState {
    drummer: bool,
    bassist: bool,
    arpeggiator: bool,
    soloist: bool,
}

/// State shared between the update systems and the draw system.
#[derive(Resource, Default)]
struct VisState {
    bar: u32,
    beat: u32,
    sixteenth: u32,
    last_sixteenth_count: u32,
    kick_flash: u8,
    snare_flash: u8,
    hihat_flash: u8,
    bass_flash: u8,
    arp_flash: u8,
    solo_flash: u8,
    last_draw: Option<Instant>,
}

/// Drum note-maps mirrored here so the TUI can show which instruments fire.
#[derive(Resource)]
struct NotePatterns {
    kick: std::collections::HashMap<(u32, u32), Note>,
    snare: std::collections::HashMap<(u32, u32), Note>,
    hihat: std::collections::HashMap<(u32, u32), Note>,
}

// ── Systems ───────────────────────────────────────────────────────────────────

fn update_vis(
    clock: Res<Clock>,
    patterns: Res<NotePatterns>,
    intensity: Res<Intensity>,
    mutes: Res<MuteState>,
    mut vis: ResMut<VisState>,
) {
    vis.bar = clock.bar_count;
    vis.beat = clock.beat;
    vis.sixteenth = clock.sixteenth;

    if clock.sixteenth_count == vis.last_sixteenth_count {
        return;
    }
    vis.last_sixteenth_count = clock.sixteenth_count;

    vis.kick_flash = vis.kick_flash.saturating_sub(1);
    vis.snare_flash = vis.snare_flash.saturating_sub(1);
    vis.hihat_flash = vis.hihat_flash.saturating_sub(1);
    vis.bass_flash = vis.bass_flash.saturating_sub(1);
    vis.arp_flash = vis.arp_flash.saturating_sub(1);
    vis.solo_flash = vis.solo_flash.saturating_sub(1);

    let min_strength = 1.0 - intensity.0;
    let key = (clock.beat, clock.sixteenth);

    if !mutes.drummer {
        if patterns.kick.get(&key).map_or(false, |n: &Note| n.strength >= min_strength) {
            vis.kick_flash = 5;
        }
        if patterns.snare.get(&key).map_or(false, |n: &Note| n.strength >= min_strength) {
            vis.snare_flash = 5;
        }
        if patterns.hihat.get(&key).map_or(false, |n: &Note| n.strength >= min_strength) {
            vis.hihat_flash = 4;
        }
    }
    if !mutes.bassist && clock.sixteenth == 0 {
        vis.bass_flash = 6;
    }
    if !mutes.arpeggiator && clock.sixteenth % 2 == 0 {
        vis.arp_flash = 4;
    }
    if !mutes.soloist && clock.beat == 0 && clock.sixteenth == 0 {
        vis.solo_flash = 8;
    }
}

fn toggle_mute(
    commands: &mut Commands,
    muted_q: &Query<Entity, With<Muted>>,
    entity: Entity,
    state: &mut bool,
) {
    if muted_q.contains(entity) {
        commands.entity(entity).remove::<Muted>();
        *state = false;
    } else {
        commands.entity(entity).insert(Muted);
        *state = true;
    }
}

fn handle_input(
    mut commands: Commands,
    band: Res<BandEntities>,
    muted_q: Query<Entity, With<Muted>>,
    mut intensity: ResMut<Intensity>,
    mut mutes: ResMut<MuteState>,
) {
    while event::poll(Duration::ZERO).unwrap_or(false) {
        let Ok(Event::Key(key)) = event::read() else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                let _ = disable_raw_mode();
                let _ = execute!(io::stdout(), LeaveAlternateScreen);
                std::process::exit(0);
            }
            KeyCode::Up => intensity.0 = (intensity.0 + 0.05).min(1.0),
            KeyCode::Down => intensity.0 = (intensity.0 - 0.05).max(0.0),
            KeyCode::Char('d') | KeyCode::Char('D') => {
                toggle_mute(&mut commands, &muted_q, band.drummer, &mut mutes.drummer);
            }
            KeyCode::Char('b') | KeyCode::Char('B') => {
                toggle_mute(&mut commands, &muted_q, band.bassist, &mut mutes.bassist);
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                toggle_mute(&mut commands, &muted_q, band.arpeggiator, &mut mutes.arpeggiator);
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                toggle_mute(&mut commands, &muted_q, band.soloist, &mut mutes.soloist);
            }
            _ => {}
        }
    }
}

fn draw_tui(
    mut terminal: ResMut<TuiTerminal>,
    mut vis: ResMut<VisState>,
    intensity: Res<Intensity>,
    mutes: Res<MuteState>,
) {
    let now = Instant::now();
    if vis.last_draw.map_or(false, |t| now.duration_since(t).as_millis() < 33) {
        return;
    }
    vis.last_draw = Some(now);

    let snap = DrawSnap {
        bar: vis.bar,
        beat: vis.beat,
        sixteenth: vis.sixteenth,
        kick_flash: vis.kick_flash,
        snare_flash: vis.snare_flash,
        hihat_flash: vis.hihat_flash,
        bass_flash: vis.bass_flash,
        arp_flash: vis.arp_flash,
        solo_flash: vis.solo_flash,
        intensity: intensity.0,
        mutes: MuteSnapshot {
            drummer: mutes.drummer,
            bassist: mutes.bassist,
            arpeggiator: mutes.arpeggiator,
            soloist: mutes.soloist,
        },
    };

    let _ = terminal.0.draw(|frame| render(frame, &snap));
}

struct MuteSnapshot {
    drummer: bool,
    bassist: bool,
    arpeggiator: bool,
    soloist: bool,
}

struct DrawSnap {
    bar: u32,
    beat: u32,
    sixteenth: u32,
    kick_flash: u8,
    snare_flash: u8,
    hihat_flash: u8,
    bass_flash: u8,
    arp_flash: u8,
    solo_flash: u8,
    intensity: f32,
    mutes: MuteSnapshot,
}

fn render(frame: &mut ratatui::Frame, s: &DrawSnap) {
    let area = frame.area();

    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(11),
        Constraint::Length(3),
    ])
    .split(area);

    // ── Header ────────────────────────────────────────────────────────────────
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " RustyMusic Full Band ",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::raw("│  "),
            Span::styled(format!("Bar {:03}", s.bar + 1), Style::default().fg(Color::Green)),
            Span::raw("  Beat "),
            Span::styled(
                (s.beat + 1).to_string(),
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  16th "),
            Span::styled((s.sixteenth + 1).to_string(), Style::default().fg(Color::Cyan)),
            Span::raw("  │  BPM 120"),
        ]))
        .block(Block::new().borders(Borders::ALL)),
        rows[0],
    );

    // ── Body ──────────────────────────────────────────────────────────────────
    let cols =
        Layout::horizontal([Constraint::Percentage(52), Constraint::Percentage(48)]).split(rows[1]);

    let left = Layout::vertical([Constraint::Length(5), Constraint::Min(5)]).split(cols[0]);

    let beat_cells: Vec<Span> = (0u32..4)
        .flat_map(|b| {
            let active = b == s.beat;
            [
                Span::styled(
                    format!(" Beat {} ", b + 1),
                    if active {
                        Style::default().bg(Color::Green).fg(Color::Black).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ),
                Span::raw(" "),
            ]
        })
        .collect();

    let six_cells: Vec<Span> = (0u32..4)
        .flat_map(|i| {
            let active = i == s.sixteenth;
            [
                Span::styled(
                    format!(" ▪{} ", i + 1),
                    if active {
                        Style::default().bg(Color::Cyan).fg(Color::Black)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ),
                Span::raw(" "),
            ]
        })
        .collect();

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(""),
            Line::from(beat_cells),
            Line::from(vec![Span::styled("  16ths within beat:", Style::default().fg(Color::DarkGray))]),
            Line::from(six_cells),
        ])
        .block(Block::new().borders(Borders::ALL).title(" Position ")),
        left[0],
    );

    let flash_line = |label: &'static str, flash: u8, muted: bool| -> Line<'static> {
        let filled = flash.min(5) as usize;
        let bar = format!("[{}{}]", "█".repeat(filled), "░".repeat(5 - filled));
        let (label_color, bar_color) = if muted {
            (Color::DarkGray, Color::DarkGray)
        } else if flash > 3 {
            (Color::White, Color::Green)
        } else if flash > 0 {
            (Color::White, Color::Yellow)
        } else {
            (Color::White, Color::DarkGray)
        };
        let mute_badge = if muted { " [MUTED]" } else { "" };
        Line::from(vec![
            Span::styled(format!(" {label:<9}"), Style::default().fg(label_color)),
            Span::styled(bar, Style::default().fg(bar_color)),
            Span::styled(mute_badge, Style::default().fg(Color::Red).add_modifier(Modifier::DIM)),
        ])
    };

    frame.render_widget(
        Paragraph::new(vec![
            flash_line("Kick     ", s.kick_flash, s.mutes.drummer),
            flash_line("Snare    ", s.snare_flash, s.mutes.drummer),
            flash_line("Hi-hat   ", s.hihat_flash, s.mutes.drummer),
            flash_line("Bass     ", s.bass_flash, s.mutes.bassist),
            flash_line("Arp      ", s.arp_flash, s.mutes.arpeggiator),
            flash_line("Soloist  ", s.solo_flash, s.mutes.soloist),
        ])
        .block(Block::new().borders(Borders::ALL).title(" Musician Activity ")),
        left[1],
    );

    let right = Layout::vertical([Constraint::Length(3), Constraint::Min(8)]).split(cols[1]);

    frame.render_widget(
        Gauge::default()
            .block(Block::new().borders(Borders::ALL).title(" Intensity  [↑] [↓] "))
            .gauge_style(Style::default().fg(Color::Yellow).bg(Color::DarkGray))
            .ratio(s.intensity as f64)
            .label(format!("{:.2}", s.intensity)),
        right[0],
    );

    let time_feel = if s.intensity < 0.33 {
        ("Half-time", Color::Blue)
    } else if s.intensity >= 0.67 {
        ("Double-time", Color::Red)
    } else {
        ("Normal time", Color::Green)
    };

    let arp_mode = if s.intensity < 0.4 {
        "Up ↑"
    } else if s.intensity < 0.7 {
        "UpDown ↕  (ping-pong)"
    } else {
        "Random ?"
    };

    let fill_countdown = 3u32.saturating_sub(s.bar % 4);
    let fills_label = if s.bar % 4 == 3 {
        " ← FILL BAR NOW".to_string()
    } else {
        format!("next fill in {} bar(s)", fill_countdown)
    };

    let soloist_section = match (s.bar / 4) % 4 {
        0 => ("Recording A  ", Color::Magenta),
        1 => ("Replaying A  ", Color::Cyan),
        2 => ("Recording B  ", Color::Magenta),
        _ => ("Replaying A final", Color::Cyan),
    };

    let bassist_info = if s.intensity >= 0.5 {
        "memory + scale runs"
    } else {
        "memory (chord tones)"
    };

    let mute_hint = |muted: bool, key: &'static str, name: &'static str| -> Line<'static> {
        let (color, tag) = if muted {
            (Color::Red, format!("[{key}] {name:<10} MUTED"))
        } else {
            (Color::DarkGray, format!("[{key}] {name:<10} on"))
        };
        Line::from(Span::styled(format!(" {tag}"), Style::default().fg(color)))
    };

    let info = vec![
        Line::from(vec![
            Span::styled(" Drummer  ", Style::default().fg(Color::Cyan)),
            Span::styled(time_feel.0, Style::default().fg(time_feel.1)),
        ]),
        Line::from(vec![
            Span::styled(" Fills    ", Style::default().fg(Color::Cyan)),
            Span::raw(fills_label.as_str()),
        ]),
        Line::from(vec![
            Span::styled(" Bass     ", Style::default().fg(Color::Cyan)),
            Span::raw(bassist_info),
        ]),
        Line::from(vec![
            Span::styled(" Arp      ", Style::default().fg(Color::Cyan)),
            Span::raw(arp_mode),
        ]),
        Line::from(vec![
            Span::styled(" Soloist  ", Style::default().fg(Color::Cyan)),
            Span::styled(soloist_section.0, Style::default().fg(soloist_section.1)),
        ]),
        Line::from(""),
        mute_hint(s.mutes.drummer, "D", "Drummer"),
        mute_hint(s.mutes.bassist, "B", "Bassist"),
        mute_hint(s.mutes.arpeggiator, "A", "Arpeggiator"),
        mute_hint(s.mutes.soloist, "S", "Soloist"),
    ];

    frame.render_widget(
        Paragraph::new(info)
            .block(Block::new().borders(Borders::ALL).title(" Feature Status / Mutes ")),
        right[1],
    );

    // ── Footer ────────────────────────────────────────────────────────────────
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" Controls: ", Style::default().fg(Color::DarkGray)),
            Span::styled("[↑][↓]", Style::default().fg(Color::Yellow)),
            Span::raw(" intensity  │  "),
            Span::styled("[D][B][A][S]", Style::default().fg(Color::Cyan)),
            Span::raw(" toggle instruments  │  "),
            Span::styled("[q]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(" quit"),
        ]))
        .block(Block::new().borders(Borders::ALL)),
        rows[2],
    );
}

// ── Startup ───────────────────────────────────────────────────────────────────

fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    let kick_h: Handle<bevy_seedling::prelude::AudioSample> =
        asset_server.load("samples/glicol/kick1.wav");
    let snare_h: Handle<bevy_seedling::prelude::AudioSample> =
        asset_server.load("samples/glicol/snare1.wav");
    let hihat_h: Handle<bevy_seedling::prelude::AudioSample> =
        asset_server.load("samples/glicol/closedhh.wav");
    let bass_h = asset_server.load("samples/glicol/bass3.wav");
    let pad_h = asset_server.load("samples/glicol/pad.wav");
    let acid_h = asset_server.load("samples/glicol/stab.wav");

    let kick808: Handle<bevy_seedling::prelude::AudioSample> =
        asset_server.load("samples/glicol/808bd.wav");
    let snare808: Handle<bevy_seedling::prelude::AudioSample> =
        asset_server.load("samples/glicol/808sd.wav");
    let hat808: Handle<bevy_seedling::prelude::AudioSample> =
        asset_server.load("samples/glicol/808oh.wav");

    let mut drummer = SuperDrummer::new(vec![
        create_drummer_only(kick_h.clone(), 1.0, generate_kick_beat()),
        create_drummer_only(snare_h.clone(), 1.0, generate_snare_beat()),
        create_drummer_only(hihat_h.clone(), 0.7, generate_hihat_beat()),
    ]);
    drummer.auto_time_feel = true;
    drummer.half_time_drums = vec![
        create_drummer_only(kick_h.clone(), 1.0, generate_half_time_kick_beat()),
        create_drummer_only(snare_h.clone(), 1.0, generate_half_time_snare_beat()),
        create_drummer_only(hihat_h.clone(), 0.6, generate_hihat_beat()),
    ];
    drummer.double_time_drums = vec![
        create_drummer_only(kick808.clone(), 1.0, generate_double_time_kick_beat()),
        create_drummer_only(snare808.clone(), 0.9, generate_double_time_snare_beat()),
        create_drummer_only(hat808.clone(), 0.5, generate_hihat_beat()),
    ];
    let drummer = drummer.with_fills(4, vec![
        create_drummer_only(kick_h.clone(), 1.0, generate_kick_beat()),
        create_drummer_only(snare_h.clone(), 1.0, generate_snare_fill_beat()),
        create_drummer_only(hihat_h.clone(), 0.6, generate_hihat_beat()),
    ]);

    let drummer_entity = commands.spawn(Musician::new("Drummer".to_string(), drummer)).id();

    let mut bassist = Bassist::new(Sampler { handle: bass_h, volume: 0.7 });
    bassist.memory_bars = 2;
    bassist.memory_repeats = 2;
    let bassist_entity = commands.spawn(Musician::new("Bassist".to_string(), bassist)).id();

    // volume reduced from 0.4 — pad.wav is inherently loud
    let mut arp = Arpeggiator::new(Sampler { handle: pad_h, volume: -4.0 });
    arp.use_scale_runs = true;
    let arp_entity = commands.spawn(Musician::new("Arpeggiator".to_string(), arp)).id();

    let soloist = Soloist::new(Sampler { handle: acid_h, volume: 0.3 }, 4);
    let soloist_entity = commands.spawn(Musician::new("Soloist".to_string(), soloist)).id();

    commands.insert_resource(BandEntities {
        drummer: drummer_entity,
        bassist: bassist_entity,
        arpeggiator: arp_entity,
        soloist: soloist_entity,
    });

    commands.insert_resource(Conductor {
        chords: generate_chords(),
        chord_length_bars: 4.0,
    });
}

// ── Entry point ───────────────────────────────────────────────────────────────

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

fn main() {
    enable_raw_mode().expect("enable raw mode");
    execute!(io::stdout(), EnterAlternateScreen).expect("enter alternate screen");
    let _guard = TerminalGuard;

    let terminal =
        Terminal::new(CrosstermBackend::new(io::stdout())).expect("create ratatui terminal");

    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(1))),
            AssetPlugin::default(),
        ))
        .add_plugins(MusicPlugin {
            beats: 4,
            note_type: 4,
            bpm: 120.0,
        })
        .insert_resource(TuiTerminal(terminal))
        .insert_resource(VisState::default())
        .insert_resource(MuteState::default())
        .insert_resource(NotePatterns {
            kick: generate_kick_beat(),
            snare: generate_snare_beat(),
            hihat: generate_hihat_beat(),
        })
        .add_systems(Update, update_vis)
        .add_systems(Update, handle_input.after(update_vis))
        .add_systems(Update, draw_tui.after(handle_input))
        .add_systems(Startup, setup)
        .run();
}
