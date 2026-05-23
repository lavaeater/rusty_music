//! time_signatures — explore how BPM and time signature change the groove.
//!
//! Controls:
//!   T            cycle time signature (4/4 → 3/4 → 5/4 → 7/4)
//!   + / =        increase BPM by 5
//!   -            decrease BPM by 5
//!   ↑ / ↓        raise / lower intensity
//!   q / Esc      quit
//!
//! The drum pattern adapts automatically:
//!   kick  — always on beat 1
//!   snare — always on the middle beat
//!   hi-hat — on every beat

use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use bevy::app::ScheduleRunnerPlugin;
use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy_seedling::prelude::{AudioSample, PlaybackSettings, SamplePlayer, Volume};

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

use rusty_music::clock::{Beat, Clock};
use rusty_music::musicians::conductor::Conductor;
use rusty_music::player::Intensity;
use rusty_music::{generate_chords, MusicPlugin};

// ── Time-signature table ──────────────────────────────────────────────────────

/// (beats_per_bar, display_name)
const TIME_SIGS: &[(u32, &str)] = &[
    (4, "4/4  common time"),
    (3, "3/4  waltz"),
    (5, "5/4  quintuple"),
    (7, "7/4  septuple"),
];

// ── Resources ─────────────────────────────────────────────────────────────────

#[derive(Resource)]
struct TuiTerminal(Terminal<CrosstermBackend<Stdout>>);

#[derive(Resource)]
struct PercussionAssets {
    kick: Handle<AudioSample>,
    snare: Handle<AudioSample>,
    hihat: Handle<AudioSample>,
}

#[derive(Resource)]
struct TimeSigState {
    index: usize,
}

impl TimeSigState {
    fn current(&self) -> (u32, &'static str) {
        let (b, name) = TIME_SIGS[self.index];
        (b, name)
    }

    fn advance(&mut self) {
        self.index = (self.index + 1) % TIME_SIGS.len();
    }
}

#[derive(Resource, Default)]
struct VisState {
    bar: u32,
    beat: u32,
    sixteenth: u32,
    beats_per_bar: u32,
    bpm: f32,
    sig_name: &'static str,
    beat_flashes: Vec<u8>,
    last_sixteenth_count: u32,
    last_draw: Option<Instant>,
}

// ── Systems ───────────────────────────────────────────────────────────────────

fn update_vis(clock: Res<Clock>, sig: Res<TimeSigState>, mut vis: ResMut<VisState>) {
    let (beats, name) = sig.current();
    vis.bar = clock.bar_count;
    vis.beat = clock.beat;
    vis.sixteenth = clock.sixteenth;
    vis.beats_per_bar = beats;
    vis.bpm = clock.tempo_bpm;
    vis.sig_name = name;

    if clock.sixteenth_count == vis.last_sixteenth_count {
        return;
    }
    vis.last_sixteenth_count = clock.sixteenth_count;

    // Grow/shrink beat-flash vec to match current beats-per-bar.
    vis.beat_flashes.resize(beats as usize, 0);
    for f in vis.beat_flashes.iter_mut() {
        *f = f.saturating_sub(1);
    }
    if clock.sixteenth == 0 {
        let idx = clock.beat as usize;
        if idx < vis.beat_flashes.len() {
            vis.beat_flashes[idx] = 5;
        }
    }
}

fn play_percussion(
    mut beat_reader: bevy::prelude::MessageReader<Beat>,
    mut commands: Commands,
    clock: Res<Clock>,
    assets: Res<PercussionAssets>,
    intensity: Res<Intensity>,
) {
    for beat in beat_reader.read() {
        if beat.sixteenth != 0 {
            continue;
        }
        let beats = clock.beats as u32;
        let mid = beats / 2;

        // Hihat on every beat (volume scales with intensity).
        let hh_vol = -6.0 + intensity.0 * 4.0;
        commands.spawn((
            SamplePlayer::new(assets.hihat.clone()).with_volume(Volume::Decibels(hh_vol)),
            PlaybackSettings::default(),
        ));

        // Kick on beat 0.
        if beat.beat == 0 {
            commands.spawn((
                SamplePlayer::new(assets.kick.clone()).with_volume(Volume::Decibels(1.0)),
                PlaybackSettings::default(),
            ));
        }

        // Snare on middle beat (skipped in 2-beat bars to avoid overlap with kick).
        if mid > 0 && beat.beat == mid {
            commands.spawn((
                SamplePlayer::new(assets.snare.clone()).with_volume(Volume::Decibels(0.9)),
                PlaybackSettings::default(),
            ));
        }

        // Extra hit on the last beat at high intensity.
        if intensity.0 > 0.6 && beat.beat == beats.saturating_sub(1) && beat.beat != 0 && beat.beat != mid {
            commands.spawn((
                SamplePlayer::new(assets.snare.clone()).with_volume(Volume::Decibels(-2.0)),
                PlaybackSettings::default(),
            ));
        }
    }
}

fn handle_input(
    mut clock: ResMut<Clock>,
    mut intensity: ResMut<Intensity>,
    mut sig: ResMut<TimeSigState>,
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
            KeyCode::Up => intensity.0 = (intensity.0 + 0.05).min(1.0),
            KeyCode::Down => intensity.0 = (intensity.0 - 0.05).max(0.0),
            KeyCode::Char('+') | KeyCode::Char('=') => {
                let bpm = (clock.tempo_bpm + 5.0).min(240.0);
                clock.tempo_bpm = bpm;
                clock.beat_length = 60.0 / (bpm * clock.beats);
            }
            KeyCode::Char('-') => {
                let bpm = (clock.tempo_bpm - 5.0).max(40.0);
                clock.tempo_bpm = bpm;
                clock.beat_length = 60.0 / (bpm * clock.beats);
            }
            KeyCode::Char('t') | KeyCode::Char('T') => {
                sig.advance();
                let (new_beats, _) = sig.current();
                clock.beats = new_beats as f32;
                clock.note_type = new_beats as f32;
                clock.beat_length = 60.0 / (clock.tempo_bpm * new_beats as f32);
                clock.beat = 0;
                clock.sixteenth = 0;
            }
            _ => {}
        }
    }
}

fn draw_tui(
    mut terminal: ResMut<TuiTerminal>,
    mut vis: ResMut<VisState>,
    intensity: Res<Intensity>,
) {
    let now = Instant::now();
    if vis.last_draw.map_or(false, |t| now.duration_since(t).as_millis() < 33) {
        return;
    }
    vis.last_draw = Some(now);

    // Copy state to avoid borrow issues inside draw closure.
    let beat = vis.beat;
    let sixteenth = vis.sixteenth;
    let bar = vis.bar;
    let bpm = vis.bpm;
    let sig_name = vis.sig_name;
    let beats_per_bar = vis.beats_per_bar;
    let flashes = vis.beat_flashes.clone();
    let intensity_val = intensity.0;

    let _ = terminal.0.draw(|frame| {
        let area = frame.area();
        let rows = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(area);

        // ── Header ────────────────────────────────────────────────────────────
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    " RustyMusic Time Signatures ",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
                Span::raw("│  Bar "),
                Span::styled(format!("{:03}", bar + 1), Style::default().fg(Color::Green)),
                Span::raw("  Beat "),
                Span::styled(
                    (beat + 1).to_string(),
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  16th "),
                Span::styled((sixteenth + 1).to_string(), Style::default().fg(Color::Cyan)),
            ]))
            .block(Block::new().borders(Borders::ALL)),
            rows[0],
        );

        // ── Body ──────────────────────────────────────────────────────────────
        let cols = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(rows[1]);

        let left = Layout::vertical([Constraint::Length(4), Constraint::Min(5)]).split(cols[0]);

        // Time sig + BPM info
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    Span::styled(" Time sig: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(sig_name, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::styled("  [T] to cycle", Style::default().fg(Color::DarkGray)),
                ]),
                Line::from(vec![
                    Span::styled(" BPM:      ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{:.0}", bpm),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("  [+][-] to adjust", Style::default().fg(Color::DarkGray)),
                ]),
            ])
            .block(Block::new().borders(Borders::ALL).title(" Time Feel ")),
            left[0],
        );

        // Beat grid — one cell per beat, lights up as each beat fires.
        let beat_cells: Vec<Span> = (0..beats_per_bar)
            .flat_map(|b| {
                let flash = flashes.get(b as usize).copied().unwrap_or(0);
                let is_kick = b == 0;
                let is_snare = b == beats_per_bar / 2 && b != 0;
                let label = if is_kick {
                    format!(" K{} ", b + 1)
                } else if is_snare {
                    format!(" S{} ", b + 1)
                } else {
                    format!(" ·{} ", b + 1)
                };
                let style = if flash > 3 {
                    if is_kick {
                        Style::default().bg(Color::Yellow).fg(Color::Black).add_modifier(Modifier::BOLD)
                    } else if is_snare {
                        Style::default().bg(Color::Red).fg(Color::White).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().bg(Color::Blue).fg(Color::White)
                    }
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                [Span::styled(label, style), Span::raw(" ")]
            })
            .collect();

        let legend = Line::from(vec![
            Span::styled(" K=kick  ", Style::default().fg(Color::Yellow)),
            Span::styled("S=snare  ", Style::default().fg(Color::Red)),
            Span::styled("·=hihat", Style::default().fg(Color::Blue)),
        ]);

        frame.render_widget(
            Paragraph::new(vec![Line::from(""), Line::from(beat_cells), Line::from(""), legend])
                .block(Block::new().borders(Borders::ALL).title(" Beat Grid ")),
            left[1],
        );

        // Right column
        let right = Layout::vertical([Constraint::Length(3), Constraint::Min(6)]).split(cols[1]);

        frame.render_widget(
            Gauge::default()
                .block(Block::new().borders(Borders::ALL).title(" Intensity [↑][↓] "))
                .gauge_style(Style::default().fg(Color::Yellow).bg(Color::DarkGray))
                .ratio(intensity_val as f64)
                .label(format!("{:.2}", intensity_val)),
            right[0],
        );

        let sig_info: Vec<Line> = TIME_SIGS
            .iter()
            .enumerate()
            .map(|(i, (_, name))| {
                let active = i == (beats_per_bar as usize - 3).min(TIME_SIGS.len() - 1)
                    || TIME_SIGS[i].0 == beats_per_bar;
                let style = if active {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                Line::from(vec![
                    Span::styled(if active { " ▶ " } else { "   " }, style),
                    Span::styled(*name, style),
                ])
            })
            .collect();

        frame.render_widget(
            Paragraph::new(sig_info)
                .block(Block::new().borders(Borders::ALL).title(" Time Signatures ")),
            right[1],
        );

        // ── Footer ────────────────────────────────────────────────────────────
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" Controls: ", Style::default().fg(Color::DarkGray)),
                Span::styled("[T]", Style::default().fg(Color::Yellow)),
                Span::raw(" time sig  │  "),
                Span::styled("[+][-]", Style::default().fg(Color::Cyan)),
                Span::raw(" BPM  │  "),
                Span::styled("[↑][↓]", Style::default().fg(Color::Green)),
                Span::raw(" intensity  │  "),
                Span::styled("[q]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw(" quit"),
            ]))
            .block(Block::new().borders(Borders::ALL)),
            rows[2],
        );
    });
}

// ── Startup ───────────────────────────────────────────────────────────────────

fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.insert_resource(PercussionAssets {
        kick: asset_server.load("samples/glicol/kick1.wav"),
        snare: asset_server.load("samples/glicol/snare1.wav"),
        hihat: asset_server.load("samples/glicol/closedhh.wav"),
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
            bpm: 100.0,
        })
        .insert_resource(TuiTerminal(terminal))
        .insert_resource(VisState {
            sig_name: "4/4  common time",
            beats_per_bar: 4,
            bpm: 100.0,
            ..Default::default()
        })
        .insert_resource(TimeSigState { index: 0 })
        .add_systems(Startup, setup)
        .add_systems(Update, update_vis)
        .add_systems(Update, handle_input.after(update_vis))
        .add_systems(Update, play_percussion.after(update_vis))
        .add_systems(Update, draw_tui.after(handle_input))
        .run();
}
