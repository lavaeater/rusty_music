//! epic_orchestra — a generative orchestral epic, intensity-driven.
//!
//! Five layers enter progressively as intensity rises:
//!
//!   0.00–0.15  Silence — ambient gathering
//!   0.15–0.35  Orchestral pad + bass ostinato enter
//!   0.35–0.55  String ostinato joins (quarter → 8th notes)
//!   0.55–0.75  Brass section enters (chord voicing — multiple voices at once)
//!   0.75–1.00  Full climax — cymbals, 16th-note ostinato, fills, all voices
//!
//! Controls:
//!   ↑ / ↓     raise / lower intensity manually
//!   Space      toggle auto-build (64-bar rise-and-fall cycle)
//!   q / Esc    quit
//!
//! Chord progression: Dm (i) → C (VII) → Bb (VI) → C (VII), 4 bars total.
//! All layers improvise within this minor-key framework.

use std::collections::HashMap;
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
use rusty_music::musicians::drummer::{
    generate_double_time_snare_beat, generate_half_time_kick_beat,
    generate_half_time_snare_beat, generate_snare_fill_beat, SuperDrummer,
};
use rusty_music::musicians::soloist::Soloist;
use rusty_music::musicians::{midi_diff_to_pitch, Chord, MusicPlayer, Note, Sampler, TonalPlayer};
use rusty_music::musicians::Musician;
use rusty_music::player::Intensity;
use rusty_music::{create_drummer_only, MusicPlugin};

// ── Chord progression ─────────────────────────────────────────────────────────

/// D natural minor: i − VII − VI − VII.
/// Each chord spans one bar; chord_length_bars = 4.0.
fn epic_chords() -> Vec<Chord> {
    // D natural minor intervals from root D:
    // 0(D), 2(E), 3(F), 5(G), 7(A), 8(Bb), 10(C)
    let scale: Vec<Note> = [
        (-12i32, 0.9), (-10, 0.5), (-9, 0.6), (-7, 0.7), (-5, 0.8),
        (-4, 0.6), (-2, 0.5), (0, 1.0), (2, 0.5), (3, 0.7),
        (5, 0.6), (7, 0.8), (8, 0.5), (10, 0.4), (12, 0.4), (15, 0.3),
    ]
    .into_iter()
    .map(|(d, s)| Note::new(d, s))
    .collect();

    vec![
        // i — Dm: dark, grounded home
        Chord::new(0.0, vec![
            Note::new(-12, 1.0), // D bass (low) — always
            Note::new(0,   1.0), // D root
            Note::new(3,   0.9), // F — minor 3rd, the dark character
            Note::new(7,   0.8), // A — 5th, stability
            Note::new(-5,  0.7), // A low 5th
            Note::new(10,  0.5), // C — minor 7th colour
            Note::new(12,  0.4), // D octave high
            Note::new(15,  0.3), // F high embellishment
        ], scale.clone()),

        // VII — C major: lift, hopeful motion
        Chord::new(1.0, vec![
            Note::new(-14, 1.0), // C bass
            Note::new(-2,  1.0), // C
            Note::new(2,   0.9), // E — major 3rd, brightness
            Note::new(7,   0.8), // G — 5th
            Note::new(-5,  0.7), // G low 5th
            Note::new(10,  0.4), // Bb — modal colour
            Note::new(-9,  0.3), // F (passing, embellishment)
        ], scale.clone()),

        // VI — Bb major: sweeping emotional peak
        Chord::new(2.0, vec![
            Note::new(-16, 1.0), // Bb bass (very low)
            Note::new(-4,  1.0), // Bb
            Note::new(-1,  0.9), // D — 3rd
            Note::new(3,   0.8), // F — 5th
            Note::new(-9,  0.7), // F low 5th
            Note::new(8,   0.5), // Bb high
            Note::new(-7,  0.4), // G added-6th colour
        ], scale.clone()),

        // VII — C major again: driving tension back toward Dm
        Chord::new(3.0, vec![
            Note::new(-14, 1.0), // C bass
            Note::new(-2,  1.0), // C
            Note::new(2,   0.9), // E
            Note::new(7,   0.8), // G
            Note::new(-5,  0.7), // G low
            Note::new(12,  0.4), // D hint — anticipates Dm return
            Note::new(0,   0.3), // D suspension
        ], scale.clone()),
    ]
}

// ── Custom MusicPlayer implementations ───────────────────────────────────────

/// Orchestral pad / string sustain.
/// Triggers on quarter-note positions; more beats become active as intensity rises.
struct OrchestraPad {
    sampler: Sampler,
}

impl MusicPlayer for OrchestraPad {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, chord: &Chord) {
        if base_intensity < 0.12 || beat.sixteenth != 0 {
            return;
        }
        let active = match beat.beat {
            0 => true,
            2 => base_intensity > 0.28,
            1 | 3 => base_intensity > 0.62,
            _ => false,
        };
        if !active {
            return;
        }
        let min_strength = 1.0 - base_intensity;
        if let Some(note) = TonalPlayer::get_chord_note(chord, min_strength) {
            // Swell the volume as intensity rises
            let vol = self.sampler.volume as f32 - 3.0 + base_intensity * 4.0;
            commands.spawn((
                SamplePlayer::new(self.sampler.handle.clone())
                    .with_volume(Volume::Decibels(vol)),
                PlaybackSettings::default()
                    .with_speed(midi_diff_to_pitch(note.midi_note_diff)),
            ));
        }
    }
}

/// String ostinato: ascending repeating arpeggio.
/// Rate scales with intensity: quarter → 8th → 16th notes.
struct OstinatoStrings {
    sampler: Sampler,
    seq_idx: u32,
}

impl MusicPlayer for OstinatoStrings {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, chord: &Chord) {
        if base_intensity < 0.22 {
            return;
        }
        let step: u32 = if base_intensity < 0.42 { 4 }   // quarter notes
                        else if base_intensity < 0.68 { 2 } // 8th notes
                        else { 1 };                          // 16th notes

        if TonalPlayer::flat_step(&beat) % step != 0 {
            return;
        }

        let notes = &chord.chord_notes;
        if notes.is_empty() {
            return;
        }
        let note = &notes[(self.seq_idx as usize) % notes.len()];
        self.seq_idx = self.seq_idx.wrapping_add(1);

        if note.strength < (1.0 - base_intensity) {
            return;
        }

        let vol = self.sampler.volume as f32 - 1.0 + base_intensity * 2.0;
        commands.spawn((
            SamplePlayer::new(self.sampler.handle.clone())
                .with_volume(Volume::Decibels(vol)),
            PlaybackSettings::default()
                .with_speed(midi_diff_to_pitch(note.midi_note_diff)),
        ));
    }
}

/// Brass section: multi-voice chord hits.
///
/// The key feature: at high intensity this spawns 2–3 simultaneous SamplePlayers,
/// each at a different chord-tone pitch, creating a thick orchestral brass chord.
struct BrassSection {
    /// Multiple samples for timbral variety between hits.
    samples: Vec<Handle<AudioSample>>,
    volume: f32,
}

impl MusicPlayer for BrassSection {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, chord: &Chord) {
        if base_intensity < 0.42 || self.samples.is_empty() {
            return;
        }

        // Strong beats at all intensities; weak beats only at high
        let on_quarter = beat.sixteenth == 0 && match beat.beat {
            0 | 2 => true,
            1 | 3 => base_intensity > 0.75,
            _ => false,
        };
        // Syncopated "and of 2" accent near climax
        let syncopated = base_intensity > 0.88 && beat.beat == 1 && beat.sixteenth == 2;

        if !on_quarter && !syncopated {
            return;
        }

        // Stack 1–3 simultaneous voices for orchestral chord thickness
        let num_voices: usize = if base_intensity > 0.85 { 3 }
                                else if base_intensity > 0.62 { 2 }
                                else { 1 };

        let min_strength = (1.0 - base_intensity).max(0.0);
        let strong_notes: Vec<&Note> = chord.chord_notes.iter()
            .filter(|n| n.strength >= min_strength)
            .take(num_voices)
            .collect();

        for (i, note) in strong_notes.iter().enumerate() {
            // Each additional voice is slightly quieter for blend
            let vol = self.volume - i as f32 * 1.8;
            // Cycle through different hit samples for variation
            let sample_idx = (beat.beat as usize + beat.bar_count as usize * 3 + i)
                % self.samples.len();
            commands.spawn((
                SamplePlayer::new(self.samples[sample_idx].clone())
                    .with_volume(Volume::Decibels(vol)),
                PlaybackSettings::default()
                    .with_speed(midi_diff_to_pitch(note.midi_note_diff)),
            ));
        }
    }
}

/// Cymbal layer: crash swells and ride shimmer.
struct CymbalLayer {
    crash: Handle<AudioSample>,
    ride: Handle<AudioSample>,
}

impl MusicPlayer for CymbalLayer {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, _chord: &Chord) {
        // Crash on downbeat of every 2nd bar (every bar at climax)
        if beat.beat == 0 && beat.sixteenth == 0 && base_intensity > 0.52 {
            if beat.bar_count % 2 == 0 || base_intensity > 0.82 {
                let vol = -8.0 + (base_intensity - 0.52).max(0.0) / 0.48 * 9.0;
                commands.spawn((
                    SamplePlayer::new(self.crash.clone())
                        .with_volume(Volume::Decibels(vol)),
                    PlaybackSettings::default(),
                ));
            }
        }
        // Ride shimmer on off-beats at medium intensity
        if base_intensity > 0.48 && beat.sixteenth == 2 && beat.beat % 2 == 1 {
            commands.spawn((
                SamplePlayer::new(self.ride.clone())
                    .with_volume(Volume::Decibels(-6.0 + base_intensity * 3.0)),
                PlaybackSettings::default(),
            ));
        }
    }
}

// ── Orchestral percussion patterns ────────────────────────────────────────────

fn timpani_normal() -> HashMap<(u32, u32), Note> {
    // Orchestral bass drum / timpani: strong beats, embellishments at high intensity
    HashMap::from([
        ((0, 0), Note::new(0, 1.0)),  // beat 1 always
        ((2, 0), Note::new(0, 1.0)),  // beat 3 always
        ((1, 2), Note::new(0, 0.3)),  // 8th offbeat embellishment
        ((3, 2), Note::new(0, 0.3)),  // 8th offbeat embellishment
    ])
}

fn timpani_march() -> HashMap<(u32, u32), Note> {
    // All four beats plus syncopations — double-time march feel
    HashMap::from([
        ((0, 0), Note::new(0, 1.0)),
        ((1, 0), Note::new(0, 0.85)),
        ((2, 0), Note::new(0, 1.0)),
        ((3, 0), Note::new(0, 0.85)),
        ((0, 2), Note::new(0, 0.35)),
        ((2, 2), Note::new(0, 0.35)),
    ])
}

fn snare_orchestral() -> HashMap<(u32, u32), Note> {
    // Snare on backbeats with ghost notes
    HashMap::from([
        ((1, 0), Note::new(0, 1.0)),
        ((3, 0), Note::new(0, 1.0)),
        ((0, 2), Note::new(0, 0.12)),
        ((1, 2), Note::new(0, 0.12)),
        ((2, 2), Note::new(0, 0.12)),
        ((3, 2), Note::new(0, 0.12)),
    ])
}

// ── Resources ─────────────────────────────────────────────────────────────────

#[derive(Resource)]
struct TuiTerminal(Terminal<CrosstermBackend<Stdout>>);

#[derive(Resource, Default)]
struct VisState {
    bar: u32,
    beat: u32,
    sixteenth: u32,
    last_sixteenth_count: u32,
    // Activity flashes (decay each 16th)
    timp_flash: u8,
    pad_flash: u8,
    strings_flash: u8,
    brass_flash: u8,
    lead_flash: u8,
    cymbals_flash: u8,
    // Auto-build
    auto_mode: bool,
    last_draw: Option<Instant>,
}

// ── Systems ───────────────────────────────────────────────────────────────────

fn update_vis(clock: Res<Clock>, intensity: Res<Intensity>, mut vis: ResMut<VisState>) {
    vis.bar = clock.bar_count;
    vis.beat = clock.beat;
    vis.sixteenth = clock.sixteenth;

    if clock.sixteenth_count == vis.last_sixteenth_count {
        return;
    }
    vis.last_sixteenth_count = clock.sixteenth_count;

    vis.timp_flash = vis.timp_flash.saturating_sub(1);
    vis.pad_flash = vis.pad_flash.saturating_sub(1);
    vis.strings_flash = vis.strings_flash.saturating_sub(1);
    vis.brass_flash = vis.brass_flash.saturating_sub(1);
    vis.lead_flash = vis.lead_flash.saturating_sub(1);
    vis.cymbals_flash = vis.cymbals_flash.saturating_sub(1);

    let i = intensity.0;

    if clock.sixteenth == 0 {
        if i >= 0.12 { vis.pad_flash = 5; }
        vis.timp_flash = 6;
        if i >= 0.52 { vis.cymbals_flash = 7; }
        if i >= 0.42 { vis.brass_flash = 8; }
    }
    if i >= 0.22 && clock.sixteenth_count % 2 == 0 {
        vis.strings_flash = 4;
    }
    if clock.beat == 0 && clock.sixteenth == 0 {
        vis.lead_flash = 8;
    }
}

fn auto_build(clock: Res<Clock>, mut intensity: ResMut<Intensity>, vis: Res<VisState>) {
    if !vis.auto_mode {
        return;
    }
    // 64-bar cycle: 48 bars rising to 1.0, then 16 bars fading back to 0.0
    let cycle = (clock.bar_count % 64) as f32;
    intensity.0 = if cycle < 48.0 {
        (cycle / 48.0).clamp(0.0, 1.0)
    } else {
        (1.0 - (cycle - 48.0) / 16.0).clamp(0.0, 1.0)
    };
}

fn handle_input(mut intensity: ResMut<Intensity>, mut vis: ResMut<VisState>) {
    while event::poll(Duration::ZERO).unwrap_or(false) {
        let Ok(Event::Key(key)) = event::read() else { continue; };
        if key.kind != KeyEventKind::Press { continue; }
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                let _ = disable_raw_mode();
                let _ = execute!(io::stdout(), LeaveAlternateScreen);
                std::process::exit(0);
            }
            KeyCode::Up => {
                vis.auto_mode = false;
                intensity.0 = (intensity.0 + 0.05).min(1.0);
            }
            KeyCode::Down => {
                vis.auto_mode = false;
                intensity.0 = (intensity.0 - 0.05).max(0.0);
            }
            KeyCode::Char(' ') => {
                vis.auto_mode = !vis.auto_mode;
            }
            _ => {}
        }
    }
}

// ── TUI rendering ─────────────────────────────────────────────────────────────

fn section_info(intensity: f32) -> (&'static str, Color) {
    match (intensity * 6.0) as u32 {
        0 => ("Silence — gathering storm...", Color::DarkGray),
        1 => ("Low strings emerge", Color::Blue),
        2 => ("Ostinato joins", Color::Cyan),
        3 => ("Brass section enters", Color::Yellow),
        4 => ("Full orchestra", Color::Green),
        _ => ("★  C L I M A X  ★", Color::Red),
    }
}

fn chord_name(time_bars: f32) -> (&'static str, Color) {
    match (time_bars % 4.0) as u32 {
        0 => ("Dm  (i)   — minor home", Color::Blue),
        1 => ("C   (VII) — lift",       Color::Cyan),
        2 => ("Bb  (VI)  — epic swell", Color::Yellow),
        _ => ("C   (VII) — drive home", Color::Cyan),
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

    let i = intensity.0;
    let (section, section_color) = section_info(i);
    let time_bars = vis.bar as f32 + vis.beat as f32 / 4.0 + vis.sixteenth as f32 / 16.0;
    let (chord, chord_color) = chord_name(time_bars);
    let auto = vis.auto_mode;

    let snap = DrawSnap {
        bar: vis.bar, beat: vis.beat, sixteenth: vis.sixteenth,
        timp_flash: vis.timp_flash,
        pad_flash: vis.pad_flash,
        strings_flash: vis.strings_flash,
        brass_flash: vis.brass_flash,
        lead_flash: vis.lead_flash,
        cymbals_flash: vis.cymbals_flash,
        intensity: i,
        section, section_color,
        chord, chord_color,
        auto_mode: auto,
    };

    let _ = terminal.0.draw(|frame| render(frame, &snap));
}

struct DrawSnap {
    bar: u32,
    beat: u32,
    sixteenth: u32,
    timp_flash: u8,
    pad_flash: u8,
    strings_flash: u8,
    brass_flash: u8,
    lead_flash: u8,
    cymbals_flash: u8,
    intensity: f32,
    section: &'static str,
    section_color: Color,
    chord: &'static str,
    chord_color: Color,
    auto_mode: bool,
}

fn render(frame: &mut ratatui::Frame, s: &DrawSnap) {
    let area = frame.area();

    let rows = Layout::vertical([
        Constraint::Length(3),  // header
        Constraint::Min(12),    // body
        Constraint::Length(3),  // footer
    ])
    .split(area);

    // ── Header ────────────────────────────────────────────────────────────────
    let auto_tag = if s.auto_mode {
        Span::styled(" [AUTO BUILD]", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("              ", Style::default())
    };

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" EPIC ORCHESTRA ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw("│  Bar "),
            Span::styled(format!("{:04}", s.bar + 1), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("  Beat "),
            Span::styled((s.beat + 1).to_string(), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("  ▪"),
            Span::styled((s.sixteenth + 1).to_string(), Style::default().fg(Color::Cyan)),
            Span::raw("  │  BPM 88  ─  D minor"),
            auto_tag,
        ]))
        .block(Block::new().borders(Borders::ALL)),
        rows[0],
    );

    // ── Body ──────────────────────────────────────────────────────────────────
    let cols = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(rows[1]);

    let left = Layout::vertical([
        Constraint::Length(4),   // section + chord
        Constraint::Min(7),      // layer activity
    ])
    .split(cols[0]);

    // Section / chord display
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(" Section: ", Style::default().fg(Color::DarkGray)),
                Span::styled(s.section, Style::default().fg(s.section_color).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled(" Chord:   ", Style::default().fg(Color::DarkGray)),
                Span::styled(s.chord, Style::default().fg(s.chord_color)),
            ]),
            Line::from(vec![
                Span::styled(
                    format!(" Progression: Dm → C → Bb → C  (bar {} of 4)", (s.bar % 4) + 1),
                    Style::default().fg(Color::DarkGray),
                ),
            ]),
        ])
        .block(Block::new().borders(Borders::ALL).title(" Composition ")),
        left[0],
    );

    // Layer activity bars
    let layer = |label: &'static str, flash: u8, threshold: f32, active_col: Color| -> Line<'static> {
        let filled = flash.min(5) as usize;
        let bar_str = format!("[{}{}]", "█".repeat(filled), "░".repeat(5 - filled));
        let bar_color = if flash > 3 { active_col } else if flash > 0 { Color::Yellow } else { Color::DarkGray };
        Line::from(vec![
            Span::styled(format!(" {label:<18}"), Style::default().fg(Color::White)),
            Span::styled(bar_str, Style::default().fg(bar_color)),
            Span::styled(
                format!("  >={:.0}%", threshold * 100.0),
                Style::default().fg(Color::DarkGray),
            ),
        ])
    };

    frame.render_widget(
        Paragraph::new(vec![
            layer("Timpani / Drums", s.timp_flash,   0.0,  Color::Red),
            layer("Orchestral Pad",  s.pad_flash,    0.12, Color::Blue),
            layer("String Ostinato", s.strings_flash,0.22, Color::Cyan),
            layer("Brass Section",   s.brass_flash,  0.42, Color::Yellow),
            layer("Cymbals",         s.cymbals_flash,0.52, Color::Magenta),
            layer("Lead Melody",     s.lead_flash,   0.0,  Color::Green),
        ])
        .block(Block::new().borders(Borders::ALL).title(" Orchestral Layers ")),
        left[1],
    );

    // Right column
    let right = Layout::vertical([
        Constraint::Length(3),   // intensity gauge
        Constraint::Min(9),      // info panel
    ])
    .split(cols[1]);

    let auto_label = if s.auto_mode {
        " Intensity [AUTO] "
    } else {
        " Intensity [↑][↓] "
    };
    frame.render_widget(
        Gauge::default()
            .block(Block::new().borders(Borders::ALL).title(auto_label))
            .gauge_style(Style::default().fg(s.section_color).bg(Color::DarkGray))
            .ratio(s.intensity as f64)
            .label(format!("{:.2}", s.intensity)),
        right[0],
    );

    // Ostinato rate info
    let ostinato_rate = if s.intensity < 0.22 { "silent" }
                        else if s.intensity < 0.42 { "♩  quarter notes" }
                        else if s.intensity < 0.68 { "♪  eighth notes" }
                        else { "♬  sixteenth notes" };

    let brass_voices = if s.intensity < 0.42 { "0" }
                       else if s.intensity < 0.62 { "1 voice" }
                       else if s.intensity < 0.85 { "2 voices" }
                       else { "3 voices (full chord)" };

    // Beat position grid
    let beat_cells: Vec<Span> = (0u32..4)
        .flat_map(|b| {
            let active = b == s.beat;
            [
                Span::styled(
                    format!(" {} ", b + 1),
                    if active {
                        Style::default().bg(s.section_color).fg(Color::Black).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ),
                Span::raw(" "),
            ]
        })
        .collect();

    let info = vec![
        Line::from(vec![Span::styled(" Beat:  ", Style::default().fg(Color::DarkGray))]
            .into_iter().chain(beat_cells).collect::<Vec<_>>()),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Strings:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(ostinato_rate, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled(" Brass:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(brass_voices, Style::default().fg(Color::Yellow)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                " The brass section stacks multiple simultaneous",
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                " note-samples to voice full chords.",
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            ),
        ]),
    ];

    frame.render_widget(
        Paragraph::new(info)
            .block(Block::new().borders(Borders::ALL).title(" Arrangement ")),
        right[1],
    );

    // ── Footer ────────────────────────────────────────────────────────────────
    let space_label = if s.auto_mode {
        Span::styled("[Space] stop auto", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("[Space] auto-build", Style::default().fg(Color::Magenta))
    };

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" Controls: ", Style::default().fg(Color::DarkGray)),
            Span::styled("[↑][↓]", Style::default().fg(Color::Yellow)),
            Span::raw(" intensity  │  "),
            space_label,
            Span::raw("  │  "),
            Span::styled("[q]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(" quit"),
        ]))
        .block(Block::new().borders(Borders::ALL)),
        rows[2],
    );
}

// ── Startup ───────────────────────────────────────────────────────────────────

fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    // Load samples
    let kick_h:   Handle<AudioSample> = asset_server.load("samples/glicol/kick1.wav");
    let kick2_h:  Handle<AudioSample> = asset_server.load("samples/glicol/kick2.wav");
    let snare_h:  Handle<AudioSample> = asset_server.load("samples/glicol/snare1.wav");
    let crash_h:  Handle<AudioSample> = asset_server.load("samples/glicol/crash.wav");
    let ride_h:   Handle<AudioSample> = asset_server.load("samples/glicol/ride.wav");
    let pad_h:    Handle<AudioSample> = asset_server.load("samples/glicol/pad.wav");
    let pluck_h:  Handle<AudioSample> = asset_server.load("samples/glicol/pluck.wav");
    let moog_h:   Handle<AudioSample> = asset_server.load("samples/glicol/moog.wav");
    let hit1_h:   Handle<AudioSample> = asset_server.load("samples/glicol/hit1.wav");
    let hit2_h:   Handle<AudioSample> = asset_server.load("samples/glicol/hit2.wav");
    let hit3_h:   Handle<AudioSample> = asset_server.load("samples/glicol/hit3.wav");
    let sax_h:    Handle<AudioSample> = asset_server.load("samples/glicol/sax.wav");

    // ── Orchestral Percussion (SuperDrummer) ──────────────────────────────────
    // Normal: timpani + snare + ride
    // Half-time: sparse kicks only
    // Double-time: march feel
    // Fill: snare roll on last bar
    let mut drums = SuperDrummer::new(vec![
        create_drummer_only(kick_h.clone(),  1.0, timpani_normal()),
        create_drummer_only(snare_h.clone(), 0.8, snare_orchestral()),
    ]);
    drums.auto_time_feel = true;
    drums.half_time_drums = vec![
        create_drummer_only(kick_h.clone(), 1.0, generate_half_time_kick_beat()),
        create_drummer_only(snare_h.clone(), 0.7, generate_half_time_snare_beat()),
    ];
    drums.double_time_drums = vec![
        create_drummer_only(kick2_h.clone(), 1.0, timpani_march()),
        create_drummer_only(snare_h.clone(), 0.8, generate_double_time_snare_beat()),
    ];
    let drums = drums.with_fills(4, vec![
        create_drummer_only(kick_h.clone(), 1.0, timpani_normal()),
        create_drummer_only(snare_h.clone(), 1.0, generate_snare_fill_beat()),
    ]);
    commands.spawn(Musician::new("Timpani".to_string(), drums));

    // ── Orchestral Pad ────────────────────────────────────────────────────────
    commands.spawn(Musician::new(
        "Pad".to_string(),
        OrchestraPad { sampler: Sampler { handle: pad_h, volume: -1.0 } },
    ));

    // ── String Ostinato (pluck — pizzicato feel) ───────────────────────────────
    commands.spawn(Musician::new(
        "Strings".to_string(),
        OstinatoStrings { sampler: Sampler { handle: pluck_h, volume: -2.0 }, seq_idx: 0 },
    ));

    // ── Bass Ostinato (moog — cello/bass section) ─────────────────────────────
    // A second ostinato at lower pitch offsets for bass depth.
    // Runs at half rate (quarter notes only) for a walking bass feel.
    commands.spawn(Musician::new(
        "Bass".to_string(),
        OstinatoStrings { sampler: Sampler { handle: moog_h, volume: -1.0 }, seq_idx: 3 },
    ));

    // ── Brass Section ─────────────────────────────────────────────────────────
    // Three different hit samples cycle for timbral variation.
    commands.spawn(Musician::new(
        "Brass".to_string(),
        BrassSection {
            samples: vec![hit1_h, hit2_h, hit3_h],
            volume: -1.0,
        },
    ));

    // ── Cymbal Layer ──────────────────────────────────────────────────────────
    commands.spawn(Musician::new(
        "Cymbals".to_string(),
        CymbalLayer { crash: crash_h, ride: ride_h },
    ));

    // ── Lead Melody (AABA soloist) ────────────────────────────────────────────
    commands.spawn(Musician::new(
        "Lead".to_string(),
        Soloist::new(Sampler { handle: sax_h, volume: -1.0 }, 4),
    ));

    // ── Conductor ────────────────────────────────────────────────────────────
    commands.insert_resource(Conductor {
        chords: epic_chords(),
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
            bpm: 88.0,
        })
        .insert_resource(TuiTerminal(terminal))
        .insert_resource(VisState::default())
        .insert_resource(Intensity(0.0)) // start silent — auto-build or manual
        .add_systems(Startup, setup)
        .add_systems(Update, update_vis)
        .add_systems(Update, auto_build.after(update_vis))
        .add_systems(Update, handle_input.after(auto_build))
        .add_systems(Update, draw_tui.after(handle_input))
        .run();
}
