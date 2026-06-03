//! real_orchestra — epic_orchestra, but voiced with a *real* multisampled
//! orchestra loaded from `assets/all-samples` (Philharmonia / Sonatina set).
//!
//! Where `epic_orchestra` pitch-shifts a single synth sample across each chord,
//! every section here is a [`MultiSampler`]: it picks the nearest *recorded*
//! note to its target pitch (shifting only a residual semitone or two) and
//! swaps articulation + dynamic *samples* as intensity rises — so both the
//! playing style and the timbre change, not just the volume.
//!
//! Layers enter progressively as intensity rises:
//!
//!   0.00–0.13  Silence
//!   0.13–0.30  Cellos sustain + violin pizzicato        (soft, sparse)
//!   0.30–0.45  Clarinet harmony pad joins under strings
//!   0.45–0.58  Violin spiccato ostinato + flute runs    (driving 8ths)
//!   0.58–0.78  Brass chord stabs + bassoon + sustains   (legato, fuller)
//!   0.78–1.00  Climax: string tremolo, oboe, 16ths, full percussion, ff samples
//!
//! Woodwind choir (flute / oboe / clarinet / bassoon) layers in by role: flute
//! sparkles offbeat scale runs, clarinet + bassoon fill harmony, oboe adds
//! reedy upper colour at the peak.
//!
//! Controls:
//!   ↑ / ↓     raise / lower intensity manually
//!   Space      toggle auto-build (64-bar rise-and-fall cycle)
//!   q / Esc    quit
//!
//! Chord progression: Dm (i) → C (VII) → Bb (VI) → C (VII), 4 bars total.

use std::io::{self, Stdout};
use std::path::PathBuf;
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

use std::collections::HashMap;

use rusty_music::clock::{Beat, Clock};
use rusty_music::musicians::conductor::Conductor;
use rusty_music::musicians::drummer::{generate_snare_fill_beat, SuperDrummer};
use rusty_music::musicians::{midi_diff_to_pitch, Chord, MusicPlayer, Musician, Note, TonalPlayer};
use rusty_music::player::Intensity;
use rusty_music::sampler::{Dynamic, MultiSampler, SampleLibrary, VoiceFilter};
use rusty_music::{create_drummer_only, MusicPlugin};

// ── Chord progression ─────────────────────────────────────────────────────────

/// D natural minor: i − VII − VI − VII. One chord per bar (chord_length_bars = 4).
fn epic_chords() -> Vec<Chord> {
    let scale: Vec<Note> = [
        (-12i32, 0.9), (-10, 0.5), (-9, 0.6), (-7, 0.7), (-5, 0.8),
        (-4, 0.6), (-2, 0.5), (0, 1.0), (2, 0.5), (3, 0.7),
        (5, 0.6), (7, 0.8), (8, 0.5), (10, 0.4), (12, 0.4),
    ]
    .into_iter()
    .map(|(d, s)| Note::new(d, s))
    .collect();

    vec![
        // i — Dm
        Chord::new(0.0, vec![
            Note::new(-12, 1.0), Note::new(0, 1.0), Note::new(3, 0.9),
            Note::new(7, 0.8), Note::new(-5, 0.7), Note::new(10, 0.5),
            Note::new(12, 0.4), Note::new(15, 0.3),
        ], scale.clone()),
        // VII — C
        Chord::new(1.0, vec![
            Note::new(-14, 1.0), Note::new(-2, 1.0), Note::new(2, 0.9),
            Note::new(7, 0.8), Note::new(-5, 0.7), Note::new(10, 0.4),
        ], scale.clone()),
        // VI — Bb
        Chord::new(2.0, vec![
            Note::new(-16, 1.0), Note::new(-4, 1.0), Note::new(-1, 0.9),
            Note::new(3, 0.8), Note::new(-9, 0.7), Note::new(8, 0.5),
        ], scale.clone()),
        // VII — C
        Chord::new(3.0, vec![
            Note::new(-14, 1.0), Note::new(-2, 1.0), Note::new(2, 0.9),
            Note::new(7, 0.8), Note::new(-5, 0.7), Note::new(12, 0.4),
        ], scale.clone()),
    ]
}

// ── Spawning helper ─────────────────────────────────────────────────────────

/// Spawn one note from a multisampler at the target pitch, if a sample exists.
fn play_note(
    commands: &mut Commands,
    sampler: &MultiSampler,
    target_midi: i32,
    role: &str,
    dynamic: Dynamic,
    extra_db: f32,
) {
    if let Some((handle, residual)) = sampler.pick(target_midi, role, dynamic) {
        commands.spawn((
            SamplePlayer::new(handle).with_volume(Volume::Decibels(sampler.volume_db + extra_db)),
            PlaybackSettings::default().with_speed(midi_diff_to_pitch(residual)),
        ));
    }
}

// ── String section ────────────────────────────────────────────────────────────

/// A multisampled string section. As intensity rises it changes *playing mode*:
/// pizzicato → spiccato ostinato → legato sustain → tremolo. `is_bass` sections
/// hold low roots/fifths; melodic sections run an arpeggiated ostinato.
struct StringSection {
    sampler: MultiSampler,
    /// Absolute MIDI note that chord offset 0 maps to.
    root_midi: i32,
    is_bass: bool,
    seq_idx: u32,
}

impl StringSection {
    fn play_bass(&mut self, beat: Beat, commands: &mut Commands, i: f32, chord: &Chord) {
        // Bass plays on strong beats only; whole-bar feel.
        if beat.sixteenth != 0 {
            return;
        }
        let active = match beat.beat {
            0 => true,
            2 => i > 0.45,
            _ => false,
        };
        if !active {
            return;
        }
        let dynamic = Dynamic::for_intensity(i);
        let role = if i > 0.78 && self.sampler.has_role("tremolo") {
            "tremolo"
        } else {
            "sustain"
        };
        // Root (lowest chord tone), plus the 5th above it as intensity grows.
        let Some(root) = chord.chord_notes.iter().min_by_key(|n| n.midi_note_diff) else { return };
        play_note(commands, &self.sampler, self.root_midi + root.midi_note_diff, role, dynamic, -1.0 + i * 3.0);
        if i > 0.6 {
            play_note(commands, &self.sampler, self.root_midi + root.midi_note_diff + 7, role, dynamic, -4.0 + i * 3.0);
        }
    }

    fn play_melodic(&mut self, beat: Beat, commands: &mut Commands, i: f32, chord: &Chord) {
        let dynamic = Dynamic::for_intensity(i);
        let step16 = TonalPlayer::flat_step(&beat);

        // Choose role + rhythmic rate by intensity band.
        let (role, rate, extra) = if i < 0.32 {
            ("pizz", 4, 0.0)        // quarter-note pizzicato
        } else if i < 0.58 {
            ("ostinato", 2, -1.0)   // eighth-note spiccato
        } else if i < 0.78 {
            ("sustain", 4, 1.0)     // sustained chord tones on quarters
        } else {
            ("ostinato", 1, -2.0)   // sixteenth-note drive at the climax
        };

        // Climax also lays a tremolo bed on the downbeats.
        if i >= 0.78
            && beat.sixteenth == 0
            && (beat.beat == 0 || beat.beat == 2)
            && let Some(note) = top_chord_note(chord, i)
        {
            play_note(commands, &self.sampler, self.root_midi + note.midi_note_diff, "tremolo", dynamic, 1.0);
        }

        if !step16.is_multiple_of(rate) {
            return;
        }
        let notes = &chord.chord_notes;
        if notes.is_empty() {
            return;
        }
        // Sustained role voices the chord top; rhythmic roles arpeggiate.
        let note = if role == "sustain" {
            match top_chord_note(chord, i) {
                Some(n) => n,
                None => return,
            }
        } else {
            let n = notes[(self.seq_idx as usize) % notes.len()];
            self.seq_idx = self.seq_idx.wrapping_add(1);
            if n.strength < (1.0 - i) {
                return;
            }
            n
        };
        play_note(commands, &self.sampler, self.root_midi + note.midi_note_diff, role, dynamic, extra + i * 2.0);
    }
}

/// Highest-pitched chord tone whose strength clears the intensity gate.
fn top_chord_note(chord: &Chord, intensity: f32) -> Option<Note> {
    let min_strength = 1.0 - intensity;
    chord
        .chord_notes
        .iter()
        .filter(|n| n.strength >= min_strength)
        .max_by_key(|n| n.midi_note_diff)
        .copied()
}

impl MusicPlayer for StringSection {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, chord: &Chord) {
        if base_intensity < 0.13 || self.sampler.is_empty() {
            return;
        }
        if self.is_bass {
            self.play_bass(beat, commands, base_intensity, chord);
        } else {
            self.play_melodic(beat, commands, base_intensity, chord);
        }
    }
}

// ── Brass section ──────────────────────────────────────────────────────────────

/// Multisampled brass chord stabs. Stacks 1–3 simultaneous chord tones (a full
/// brass chord) on strong beats once intensity is high enough.
struct BrassSection {
    sampler: MultiSampler,
    root_midi: i32,
}

impl MusicPlayer for BrassSection {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, chord: &Chord) {
        let i = base_intensity;
        if i < 0.42 || self.sampler.is_empty() {
            return;
        }
        let on_quarter = beat.sixteenth == 0
            && match beat.beat {
                0 | 2 => true,
                1 | 3 => i > 0.75,
                _ => false,
            };
        let syncopated = i > 0.88 && beat.beat == 1 && beat.sixteenth == 2;
        if !on_quarter && !syncopated {
            return;
        }

        let num_voices = if i > 0.85 { 3 } else if i > 0.62 { 2 } else { 1 };
        let dynamic = Dynamic::for_intensity(i);
        let min_strength = (1.0 - i).max(0.0);
        let voices: Vec<Note> = chord
            .chord_notes
            .iter()
            .filter(|n| n.strength >= min_strength)
            .take(num_voices)
            .copied()
            .collect();

        for (idx, note) in voices.iter().enumerate() {
            // Each stacked voice slightly quieter, for blend.
            play_note(
                commands,
                &self.sampler,
                self.root_midi + note.midi_note_diff,
                "stab",
                dynamic,
                -2.0 - idx as f32 * 1.8 + i * 3.0,
            );
        }
    }
}

// ── Woodwind choir ─────────────────────────────────────────────────────────────

/// What a given woodwind contributes to the choir.
#[derive(Clone, Copy, PartialEq)]
enum WoodwindRole {
    /// Sparkling offbeat scale runs, plus a sustained top line at the climax.
    RunTop,
    /// Sustained mid/upper inner chord tone — a warm reed pad.
    Harmony,
    /// Sustained root, doubling the cellos with woody low weight.
    BassDouble,
}

/// A single multisampled woodwind. Woodwinds are almost all `normal`
/// articulation, so the variant axes in play here are *duration* (short `run`
/// vs. long `sustain` voices) and *dynamic* layers.
struct WoodwindSection {
    sampler: MultiSampler,
    root_midi: i32,
    enter: f32,
    role: WoodwindRole,
    seq_idx: u32,
}

impl MusicPlayer for WoodwindSection {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, chord: &Chord) {
        let i = base_intensity;
        if i < self.enter || self.sampler.is_empty() {
            return;
        }
        let dynamic = Dynamic::for_intensity(i);
        match self.role {
            WoodwindRole::RunTop => {
                // Sustained top line on strong beats once the climax arrives.
                if i > 0.7
                    && beat.sixteenth == 0
                    && (beat.beat == 0 || beat.beat == 2)
                    && let Some(note) = top_chord_note(chord, i)
                {
                    play_note(commands, &self.sampler, self.root_midi + note.midi_note_diff, "sustain", dynamic, 0.0);
                }
                // Ascending scale-run sparkle on the eighth-note offbeats.
                if beat.sixteenth == 2 {
                    let scale = &chord.scale_notes;
                    if scale.is_empty() {
                        return;
                    }
                    let note = scale[(self.seq_idx as usize) % scale.len()];
                    self.seq_idx = self.seq_idx.wrapping_add(1);
                    if note.strength < (1.0 - i) {
                        return;
                    }
                    play_note(commands, &self.sampler, self.root_midi + note.midi_note_diff, "run", dynamic, -2.0 + i * 2.0);
                }
            }
            WoodwindRole::Harmony => {
                let active = beat.sixteenth == 0 && (beat.beat == 0 || (beat.beat == 2 && i > 0.5));
                if active && let Some(note) = mid_chord_note(chord, i) {
                    play_note(commands, &self.sampler, self.root_midi + note.midi_note_diff, "sustain", dynamic, i * 2.0);
                }
            }
            WoodwindRole::BassDouble => {
                if beat.sixteenth == 0
                    && beat.beat == 0
                    && let Some(root) = chord.chord_notes.iter().min_by_key(|n| n.midi_note_diff)
                {
                    play_note(commands, &self.sampler, self.root_midi + root.midi_note_diff, "sustain", dynamic, i * 2.0);
                }
            }
        }
    }
}

/// Median-register chord tone passing the intensity strength gate — the inner
/// "tenor/alto" voice, avoiding both the bass root and the very top.
fn mid_chord_note(chord: &Chord, intensity: f32) -> Option<Note> {
    let min_strength = 1.0 - intensity;
    let mut notes: Vec<Note> = chord
        .chord_notes
        .iter()
        .filter(|n| n.strength >= min_strength)
        .copied()
        .collect();
    if notes.is_empty() {
        return None;
    }
    notes.sort_by_key(|n| n.midi_note_diff);
    Some(notes[notes.len() / 2])
}

// ── Percussion (single hits the existing Drummer can't voice as swells) ────────

/// Crash + roll layer, using resolved orchestral-percussion handles directly.
struct CymbalLayer {
    clash: Option<Handle<AudioSample>>,
    susp_roll: Option<Handle<AudioSample>>,
}

impl MusicPlayer for CymbalLayer {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, _chord: &Chord) {
        let i = base_intensity;
        // Clash accent on the downbeat of every other bar (every bar at climax).
        if beat.beat == 0
            && beat.sixteenth == 0
            && i > 0.52
            && (beat.bar_count.is_multiple_of(2) || i > 0.82)
            && let Some(h) = &self.clash
        {
            let vol = -10.0 + (i - 0.52).max(0.0) / 0.48 * 11.0;
            commands.spawn((
                SamplePlayer::new(h.clone()).with_volume(Volume::Decibels(vol)),
                PlaybackSettings::default(),
            ));
        }
        // Suspended-cymbal roll swell on the bar before each chord change at climax.
        if i > 0.7
            && beat.beat == 3
            && beat.sixteenth == 0
            && let Some(h) = &self.susp_roll
        {
            commands.spawn((
                SamplePlayer::new(h.clone()).with_volume(Volume::Decibels(-9.0 + i * 5.0)),
                PlaybackSettings::default(),
            ));
        }
    }
}

// ── Percussion patterns (timpani/bass-drum + snare) ────────────────────────────

fn bassdrum_pattern() -> HashMap<(u32, u32), Note> {
    HashMap::from([
        ((0, 0), Note::new(0, 1.0)),
        ((2, 0), Note::new(0, 1.0)),
        ((1, 2), Note::new(0, 0.3)),
        ((3, 2), Note::new(0, 0.3)),
    ])
}

fn snare_pattern() -> HashMap<(u32, u32), Note> {
    HashMap::from([
        ((1, 0), Note::new(0, 1.0)),
        ((3, 0), Note::new(0, 1.0)),
        ((0, 2), Note::new(0, 0.12)),
        ((1, 2), Note::new(0, 0.12)),
        ((2, 2), Note::new(0, 0.12)),
        ((3, 2), Note::new(0, 0.12)),
    ])
}

// ── Resources / VisState ───────────────────────────────────────────────────────

#[derive(Resource)]
struct TuiTerminal(Terminal<CrosstermBackend<Stdout>>);

#[derive(Resource)]
struct LoadReport {
    lines: Vec<(String, usize)>,
}

#[derive(Resource, Default)]
struct VisState {
    bar: u32,
    beat: u32,
    sixteenth: u32,
    last_sixteenth_count: u32,
    cello_flash: u8,
    violin_flash: u8,
    wood_flash: u8,
    brass_flash: u8,
    perc_flash: u8,
    cymbal_flash: u8,
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

    vis.cello_flash = vis.cello_flash.saturating_sub(1);
    vis.violin_flash = vis.violin_flash.saturating_sub(1);
    vis.wood_flash = vis.wood_flash.saturating_sub(1);
    vis.brass_flash = vis.brass_flash.saturating_sub(1);
    vis.perc_flash = vis.perc_flash.saturating_sub(1);
    vis.cymbal_flash = vis.cymbal_flash.saturating_sub(1);

    let i = intensity.0;
    if clock.sixteenth == 0 {
        if i >= 0.13 && (clock.beat == 0 || clock.beat == 2) { vis.cello_flash = 6; }
        vis.perc_flash = 6;
        if i >= 0.30 && clock.beat == 0 { vis.wood_flash = 6; }
        if i >= 0.42 && (clock.beat == 0 || clock.beat == 2) { vis.brass_flash = 8; }
        if i >= 0.52 && clock.beat == 0 { vis.cymbal_flash = 7; }
    }
    // Flute runs land on the eighth-note offbeats.
    if i >= 0.45 && clock.sixteenth == 2 { vis.wood_flash = 4; }
    if i >= 0.13 { vis.violin_flash = 4; }
}

fn auto_build(clock: Res<Clock>, mut intensity: ResMut<Intensity>, vis: Res<VisState>) {
    if !vis.auto_mode {
        return;
    }
    let cycle = (clock.bar_count % 64) as f32;
    intensity.0 = if cycle < 48.0 {
        (cycle / 48.0).clamp(0.0, 1.0)
    } else {
        (1.0 - (cycle - 48.0) / 16.0).clamp(0.0, 1.0)
    };
}

fn handle_input(mut intensity: ResMut<Intensity>, mut vis: ResMut<VisState>) {
    while event::poll(Duration::ZERO).unwrap_or(false) {
        let Ok(Event::Key(key)) = event::read() else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }
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
            KeyCode::Char(' ') => vis.auto_mode = !vis.auto_mode,
            _ => {}
        }
    }
}

// ── TUI rendering ───────────────────────────────────────────────────────────────

fn section_info(i: f32) -> (&'static str, Color) {
    match (i * 6.0) as u32 {
        0 => ("Silence — gathering", Color::DarkGray),
        1 => ("Cellos + pizzicato", Color::Blue),
        2 => ("Spiccato ostinato", Color::Cyan),
        3 => ("Brass enters", Color::Yellow),
        4 => ("Full orchestra", Color::Green),
        _ => ("★  C L I M A X  ★", Color::Red),
    }
}

fn chord_name(time_bars: f32) -> (&'static str, Color) {
    match (time_bars % 4.0) as u32 {
        0 => ("Dm  (i)   — minor home", Color::Blue),
        1 => ("C   (VII) — lift", Color::Cyan),
        2 => ("Bb  (VI)  — epic swell", Color::Yellow),
        _ => ("C   (VII) — drive home", Color::Cyan),
    }
}

fn string_mode(i: f32) -> &'static str {
    if i < 0.13 { "—" }
    else if i < 0.32 { "pizzicato (♩)" }
    else if i < 0.58 { "spiccato ostinato (♪)" }
    else if i < 0.78 { "legato sustain (♩)" }
    else { "tremolo + 16ths (♬)" }
}

fn draw_tui(mut terminal: ResMut<TuiTerminal>, mut vis: ResMut<VisState>, intensity: Res<Intensity>, report: Res<LoadReport>) {
    let now = Instant::now();
    if vis.last_draw.is_some_and(|t| now.duration_since(t).as_millis() < 33) {
        return;
    }
    vis.last_draw = Some(now);

    let i = intensity.0;
    let (section, section_color) = section_info(i);
    let time_bars = vis.bar as f32 + vis.beat as f32 / 4.0 + vis.sixteenth as f32 / 16.0;
    let (chord, chord_color) = chord_name(time_bars);

    let snap = DrawSnap {
        bar: vis.bar, beat: vis.beat, sixteenth: vis.sixteenth,
        cello_flash: vis.cello_flash, violin_flash: vis.violin_flash, wood_flash: vis.wood_flash,
        brass_flash: vis.brass_flash, perc_flash: vis.perc_flash, cymbal_flash: vis.cymbal_flash,
        intensity: i, section, section_color, chord, chord_color,
        auto_mode: vis.auto_mode,
        dynamic: Dynamic::for_intensity(i),
        string_mode: string_mode(i),
        report: report.lines.clone(),
    };
    let _ = terminal.0.draw(|frame| render(frame, &snap));
}

struct DrawSnap {
    bar: u32, beat: u32, sixteenth: u32,
    cello_flash: u8, violin_flash: u8, wood_flash: u8, brass_flash: u8, perc_flash: u8, cymbal_flash: u8,
    intensity: f32,
    section: &'static str, section_color: Color,
    chord: &'static str, chord_color: Color,
    auto_mode: bool,
    dynamic: Dynamic,
    string_mode: &'static str,
    report: Vec<(String, usize)>,
}

fn render(frame: &mut ratatui::Frame, s: &DrawSnap) {
    let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(12), Constraint::Length(3)])
        .split(frame.area());

    // Header
    let auto_tag = if s.auto_mode {
        Span::styled(" [AUTO BUILD]", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD))
    } else {
        Span::raw("              ")
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" REAL ORCHESTRA ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw("│  Bar "),
            Span::styled(format!("{:04}", s.bar + 1), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("  Beat "),
            Span::styled((s.beat + 1).to_string(), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("  ▪"),
            Span::styled((s.sixteenth + 1).to_string(), Style::default().fg(Color::Cyan)),
            Span::raw("  │  BPM 88  ─  D minor  ─  multisampled"),
            auto_tag,
        ]))
        .block(Block::new().borders(Borders::ALL)),
        rows[0],
    );

    // Body
    let cols = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).split(rows[1]);
    let left = Layout::vertical([Constraint::Length(5), Constraint::Min(7)]).split(cols[0]);

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
                Span::styled(" Dynamic: ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:?}", s.dynamic), Style::default().fg(Color::Magenta)),
                Span::styled("   (sample layer, not just volume)", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
            ]),
        ])
        .block(Block::new().borders(Borders::ALL).title(" Composition ")),
        left[0],
    );

    let layer = |label: &'static str, flash: u8, threshold: f32, active: Color| -> Line<'static> {
        let filled = flash.min(5) as usize;
        let bar = format!("[{}{}]", "█".repeat(filled), "░".repeat(5 - filled));
        let color = if flash > 3 { active } else if flash > 0 { Color::Yellow } else { Color::DarkGray };
        Line::from(vec![
            Span::styled(format!(" {label:<18}"), Style::default().fg(Color::White)),
            Span::styled(bar, Style::default().fg(color)),
            Span::styled(format!("  >={:.0}%", threshold * 100.0), Style::default().fg(Color::DarkGray)),
        ])
    };
    frame.render_widget(
        Paragraph::new(vec![
            layer("Cellos (low)", s.cello_flash, 0.13, Color::Blue),
            layer("Violins", s.violin_flash, 0.13, Color::Cyan),
            layer("Woodwinds", s.wood_flash, 0.30, Color::Green),
            layer("Brass", s.brass_flash, 0.42, Color::Yellow),
            layer("Bass drum/Snare", s.perc_flash, 0.0, Color::Red),
            layer("Cymbals", s.cymbal_flash, 0.52, Color::Magenta),
        ])
        .block(Block::new().borders(Borders::ALL).title(" Orchestral Layers ")),
        left[1],
    );

    // Right column
    let right = Layout::vertical([Constraint::Length(3), Constraint::Min(9)]).split(cols[1]);
    let auto_label = if s.auto_mode { " Intensity [AUTO] " } else { " Intensity [↑][↓] " };
    frame.render_widget(
        Gauge::default()
            .block(Block::new().borders(Borders::ALL).title(auto_label))
            .gauge_style(Style::default().fg(s.section_color).bg(Color::DarkGray))
            .ratio(s.intensity as f64)
            .label(format!("{:.2}", s.intensity)),
        right[0],
    );

    let mut info = vec![
        Line::from(vec![
            Span::styled(" Strings: ", Style::default().fg(Color::DarkGray)),
            Span::styled(s.string_mode, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(""),
        Line::from(Span::styled(" Loaded multisamplers:", Style::default().fg(Color::DarkGray))),
    ];
    for (name, n) in &s.report {
        info.push(Line::from(vec![
            Span::styled(format!("   {name:<10} "), Style::default().fg(Color::White)),
            Span::styled(format!("{n} samples"), Style::default().fg(Color::Green)),
        ]));
    }
    frame.render_widget(
        Paragraph::new(info).block(Block::new().borders(Borders::ALL).title(" Arrangement ")),
        right[1],
    );

    // Footer
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

// ── Startup ─────────────────────────────────────────────────────────────────────

const DYNAMICS: &[Dynamic] = &[Dynamic::Piano, Dynamic::MezzoForte, Dynamic::Fortissimo];

fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    // Scan the whole sample set (metadata only — cheap).
    let library = SampleLibrary::scan(&PathBuf::from("assets"), "all-samples");

    let mut report: Vec<(String, usize)> = Vec::new();

    // ── Violins: pizz / spiccato ostinato / legato sustain / tremolo ───────────
    // Violin range A3–Gs7; melodic register ~D4..F6.
    let mut violins = MultiSampler::new("violin", -3.0);
    violins.add_voice(&library, &asset_server,
        &VoiceFilter { role: "pizz", articulations: &["pizz-normal"], durations: &[], midi_range: (62, 88) }, DYNAMICS);
    violins.add_voice(&library, &asset_server,
        &VoiceFilter { role: "ostinato", articulations: &["arco-normal"], durations: &["025", "05"], midi_range: (62, 88) }, DYNAMICS);
    violins.add_voice(&library, &asset_server,
        &VoiceFilter { role: "sustain", articulations: &["arco-legato", "arco-normal"], durations: &["1", "15", "long"], midi_range: (62, 88) }, DYNAMICS);
    violins.add_voice(&library, &asset_server,
        &VoiceFilter { role: "tremolo", articulations: &["arco-tremolo"], durations: &[], midi_range: (62, 88) }, DYNAMICS);
    report.push(("violins".into(), violins.sample_count()));
    commands.spawn(Musician::new("Violins".into(), StringSection {
        sampler: violins, root_midi: 74, is_bass: false, seq_idx: 0,
    }));

    // ── Cellos: low sustain + tremolo ──────────────────────────────────────────
    // Cello range A2–Gs5; root D3 = 50.
    let mut cellos = MultiSampler::new("cello", -2.0);
    cellos.add_voice(&library, &asset_server,
        &VoiceFilter { role: "sustain", articulations: &["arco-normal", "arco-legato"], durations: &["1", "15", "long"], midi_range: (38, 65) }, DYNAMICS);
    cellos.add_voice(&library, &asset_server,
        &VoiceFilter { role: "tremolo", articulations: &["arco-tremolo"], durations: &[], midi_range: (38, 65) }, DYNAMICS);
    report.push(("cellos".into(), cellos.sample_count()));
    commands.spawn(Musician::new("Cellos".into(), StringSection {
        sampler: cellos, root_midi: 50, is_bass: true, seq_idx: 0,
    }));

    // ── Brass: trombone chord stabs ────────────────────────────────────────────
    // Trombone range A2–Gs5; root D3 = 50.
    let mut brass = MultiSampler::new("trombone", -3.0);
    brass.add_voice(&library, &asset_server,
        &VoiceFilter { role: "stab", articulations: &["normal"], durations: &["025", "05", "1"], midi_range: (45, 74) }, DYNAMICS);
    report.push(("brass".into(), brass.sample_count()));
    commands.spawn(Musician::new("Brass".into(), BrassSection { sampler: brass, root_midi: 50 }));

    // ── Woodwind choir ─────────────────────────────────────────────────────────
    // All mostly `normal` articulation, so we lean on duration (run vs. sustain)
    // and dynamic layers. Each reed enters at a different intensity in a distinct
    // role, building flute → clarinet → bassoon → oboe into a full choir.
    let mut woodwinds = 0usize;
    // Flute — high offbeat scale runs + a sustained top line at the climax.
    // Range A4–Gs6; root D5 = 74.
    let mut flute = MultiSampler::new("flute", -6.0);
    flute.add_voice(&library, &asset_server,
        &VoiceFilter { role: "run", articulations: &["normal"], durations: &["025", "05"], midi_range: (72, 92) }, DYNAMICS);
    flute.add_voice(&library, &asset_server,
        &VoiceFilter { role: "sustain", articulations: &["normal"], durations: &["1", "15", "long"], midi_range: (72, 92) }, DYNAMICS);
    woodwinds += flute.sample_count();
    commands.spawn(Musician::new("Flute".into(), WoodwindSection {
        sampler: flute, root_midi: 74, enter: 0.45, role: WoodwindRole::RunTop, seq_idx: 0,
    }));

    // Clarinet — mid harmony pad under the strings. Range A3–Gs6; root D4 = 62.
    let mut clarinet = MultiSampler::new("clarinet", -6.0);
    clarinet.add_voice(&library, &asset_server,
        &VoiceFilter { role: "sustain", articulations: &["normal"], durations: &["1", "15", "long"], midi_range: (58, 82) }, DYNAMICS);
    woodwinds += clarinet.sample_count();
    commands.spawn(Musician::new("Clarinet".into(), WoodwindSection {
        sampler: clarinet, root_midi: 62, enter: 0.30, role: WoodwindRole::Harmony, seq_idx: 0,
    }));

    // Bassoon — low sustain doubling the cellos. Range A2–Gs4; root D4 = 62.
    let mut bassoon = MultiSampler::new("bassoon", -5.0);
    bassoon.add_voice(&library, &asset_server,
        &VoiceFilter { role: "sustain", articulations: &["normal"], durations: &["1", "15", "long"], midi_range: (45, 65) }, DYNAMICS);
    woodwinds += bassoon.sample_count();
    commands.spawn(Musician::new("Bassoon".into(), WoodwindSection {
        sampler: bassoon, root_midi: 62, enter: 0.40, role: WoodwindRole::BassDouble, seq_idx: 0,
    }));

    // Oboe — reedy upper sustained color, octave above the clarinet, climax only.
    // Range A4–Gs6; root D6 = 86 doubles the clarinet's harmony an octave up.
    let mut oboe = MultiSampler::new("oboe", -8.0);
    oboe.add_voice(&library, &asset_server,
        &VoiceFilter { role: "sustain", articulations: &["normal"], durations: &["1", "15"], midi_range: (72, 92) }, DYNAMICS);
    woodwinds += oboe.sample_count();
    commands.spawn(Musician::new("Oboe".into(), WoodwindSection {
        sampler: oboe, root_midi: 86, enter: 0.72, role: WoodwindRole::Harmony, seq_idx: 0,
    }));
    report.push(("woodwinds".into(), woodwinds));

    // ── Percussion: bass drum + snare via existing SuperDrummer ─────────────────
    let bd = library.resolve(&asset_server, "bass drum", &["struck-singly", "bass-drum-mallet", "rhythm"], Dynamic::Forte);
    let sn = library.resolve(&asset_server, "snare drum", &["with-snares", "rhythm"], Dynamic::Forte);
    let sn_roll = library.resolve(&asset_server, "snare drum", &["roll", "with-snares"], Dynamic::Forte);
    if let (Some(bd), Some(sn)) = (bd, sn) {
        let drums = SuperDrummer::new(vec![
            create_drummer_only(bd.clone(), 1.0, bassdrum_pattern()),
            create_drummer_only(sn.clone(), 0.8, snare_pattern()),
        ])
        .with_fills(4, vec![
            create_drummer_only(bd, 1.0, bassdrum_pattern()),
            create_drummer_only(sn_roll.unwrap_or(sn), 1.0, generate_snare_fill_beat()),
        ]);
        commands.spawn(Musician::new("Percussion".into(), drums));
    }

    // ── Cymbals: clash accents + suspended-cymbal roll swells ──────────────────
    commands.spawn(Musician::new("Cymbals".into(), CymbalLayer {
        clash: library.resolve(&asset_server, "clash cymbals", &["struck-together", "undamped"], Dynamic::Fortissimo),
        susp_roll: library.resolve(&asset_server, "suspended cymbal", &["roll", "undamped"], Dynamic::Forte),
    }));

    commands.insert_resource(Conductor { chords: epic_chords(), chord_length_bars: 4.0 });
    commands.insert_resource(LoadReport { lines: report });
}

// ── Entry point ──────────────────────────────────────────────────────────────────

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

    let terminal = Terminal::new(CrosstermBackend::new(io::stdout())).expect("create ratatui terminal");

    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(1))),
            AssetPlugin::default(),
        ))
        .add_plugins(MusicPlugin { beats: 4, note_type: 4, bpm: 88.0 })
        .insert_resource(TuiTerminal(terminal))
        .insert_resource(VisState::default())
        .insert_resource(Intensity(0.0))
        .add_systems(Startup, setup)
        .add_systems(Update, update_vis)
        .add_systems(Update, auto_build.after(update_vis))
        .add_systems(Update, handle_input.after(auto_build))
        .add_systems(Update, draw_tui.after(handle_input))
        .run();
}
