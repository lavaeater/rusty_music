//! music_machine — interactive TUI music workstation for rusty_music.
//!
//! Configure a band from scratch, assign samples, tweak every parameter, then
//! save to a human-readable TOML file you can share or hand-edit. Load it back
//! any time to resume exactly where you left off.
//!
//! Usage:
//!   cargo run --example music_machine
//!   cargo run --example music_machine -- my_band.toml   # load on start
//!
//! Main controls (Normal mode):
//!   ↑/↓          select instrument
//!   [F]          browse for primary sample   [G] secondary (cymbal ride / brass add)
//!   [V]/[B]      volume −1 dB / +1 dB
//!   [M]          toggle mute
//!   [P]          cycle drum pattern (drummer only)
//!   [N]          add new instrument
//!   [Del]        remove selected instrument
//!   [A]          apply all changes & rebuild musicians
//!   [I]/[O]      intensity up / down
//!   [+]/[-]      BPM up / down
//!   [C]          cycle chord progression
//!   [S]          save to TOML   [L] load from TOML
//!   [Q]/Esc      quit

use std::collections::HashMap;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::app::ScheduleRunnerPlugin;
use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy_seedling::prelude::{AudioSample, PlaybackSettings, SamplePlayer, Volume};

use ratatui::crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph},
    Terminal,
};

use serde::{Deserialize, Serialize};

use rusty_music::clock::{Beat, Clock};
use rusty_music::musicians::conductor::Conductor;
use rusty_music::musicians::drummer::{
    generate_double_time_snare_beat, generate_half_time_kick_beat,
    generate_half_time_snare_beat, generate_snare_fill_beat, Drummer,
};
use rusty_music::musicians::soloist::Soloist;
use rusty_music::musicians::arpeggiator::Arpeggiator;
use rusty_music::musicians::bassist::Bassist;
use rusty_music::musicians::{midi_diff_to_pitch, Chord, MusicPlayer, Muted, Note, Sampler, TonalPlayer};
use rusty_music::musicians::Musician;
use rusty_music::player::Intensity;
use rusty_music::MusicPlugin;

// ─────────────────────────────────────────────────────────────────────────────
//  TOML save format
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
struct MachineFile {
    settings: SettingsFile,
    instruments: Vec<InstrumentFile>,
}

#[derive(Serialize, Deserialize, Clone)]
struct SettingsFile {
    bpm: f32,
    beats: u32,
    note_type: u32,
    chord: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct InstrumentFile {
    name: String,
    kind: String,
    samples: Vec<String>,
    volume: f32,
    muted: bool,
    #[serde(default)]
    pattern: String,
    #[serde(default = "default_record_bars")]
    record_bars: u32,
}

fn default_record_bars() -> u32 {
    4
}

// ─────────────────────────────────────────────────────────────────────────────
//  Chord presets
// ─────────────────────────────────────────────────────────────────────────────


fn chords_for(name: &str) -> (Vec<Chord>, f32) {
    match name {
        "a_minor" => (chords_a_minor(), 4.0),
        "c_major" => (chords_c_major(), 4.0),
        "g_major" => (chords_g_major(), 4.0),
        _ => (chords_d_minor(), 4.0),
    }
}

fn scale(intervals: &[(i32, f32)]) -> Vec<Note> {
    intervals.iter().map(|&(d, s)| Note::new(d, s)).collect()
}

fn chords_d_minor() -> Vec<Chord> {
    let sc = scale(&[
        (-12, 0.9), (-10, 0.5), (-9, 0.6), (-7, 0.7), (-5, 0.8),
        (-4, 0.6), (-2, 0.5), (0, 1.0), (2, 0.5), (3, 0.7),
        (5, 0.6), (7, 0.8), (8, 0.5), (10, 0.4), (12, 0.4), (15, 0.3),
    ]);
    vec![
        Chord::new(0.0, vec![Note::new(-12,1.0),Note::new(0,1.0),Note::new(3,0.9),Note::new(7,0.8),Note::new(-5,0.7),Note::new(10,0.5),Note::new(12,0.4)], sc.clone()),
        Chord::new(1.0, vec![Note::new(-14,1.0),Note::new(-2,1.0),Note::new(2,0.9),Note::new(7,0.8),Note::new(-5,0.7),Note::new(10,0.4)], sc.clone()),
        Chord::new(2.0, vec![Note::new(-16,1.0),Note::new(-4,1.0),Note::new(-1,0.9),Note::new(3,0.8),Note::new(-9,0.7),Note::new(8,0.5)], sc.clone()),
        Chord::new(3.0, vec![Note::new(-14,1.0),Note::new(-2,1.0),Note::new(2,0.9),Note::new(7,0.8),Note::new(-5,0.7),Note::new(12,0.4)], sc.clone()),
    ]
}

fn chords_a_minor() -> Vec<Chord> {
    // Am → C → G → F
    let sc = scale(&[(-12,0.9),(-10,0.5),(-9,0.6),(-7,0.7),(-5,0.8),(-3,0.5),(0,1.0),(2,0.5),(3,0.7),(5,0.6),(7,0.8),(10,0.4),(12,0.4)]);
    vec![
        Chord::new(0.0, vec![Note::new(-9,1.0),Note::new(0,1.0),Note::new(3,0.9),Note::new(7,0.8),Note::new(-2,0.6)], sc.clone()),
        Chord::new(1.0, vec![Note::new(-9,1.0),Note::new(-2,1.0),Note::new(2,0.9),Note::new(7,0.8)], sc.clone()),
        Chord::new(2.0, vec![Note::new(-5,1.0),Note::new(-7,1.0),Note::new(-3,0.9),Note::new(2,0.8)], sc.clone()),
        Chord::new(3.0, vec![Note::new(-4,1.0),Note::new(-9,1.0),Note::new(-5,0.9),Note::new(0,0.8)], sc.clone()),
    ]
}

fn chords_c_major() -> Vec<Chord> {
    // C → G → Am → F
    let sc = scale(&[(-12,0.8),(-10,0.5),(-9,0.6),(-7,0.7),(-5,0.8),(-3,0.6),(0,1.0),(2,0.5),(4,0.7),(5,0.6),(7,0.8),(9,0.5),(12,0.4)]);
    vec![
        Chord::new(0.0, vec![Note::new(-12,1.0),Note::new(0,1.0),Note::new(4,0.9),Note::new(7,0.8),Note::new(-5,0.7)], sc.clone()),
        Chord::new(1.0, vec![Note::new(-5,1.0),Note::new(-7,1.0),Note::new(-3,0.9),Note::new(2,0.8),Note::new(7,0.7)], sc.clone()),
        Chord::new(2.0, vec![Note::new(-9,1.0),Note::new(0,1.0),Note::new(3,0.9),Note::new(7,0.8)], sc.clone()),
        Chord::new(3.0, vec![Note::new(-7,1.0),Note::new(-4,1.0),Note::new(0,0.9),Note::new(5,0.8)], sc.clone()),
    ]
}

fn chords_g_major() -> Vec<Chord> {
    // G → C → D → G
    let sc = scale(&[(-12,0.8),(-10,0.5),(-7,0.7),(-5,0.8),(-3,0.6),(0,1.0),(2,0.7),(4,0.6),(5,0.5),(7,0.9),(9,0.4),(11,0.3),(12,0.5)]);
    vec![
        Chord::new(0.0, vec![Note::new(-5,1.0),Note::new(0,1.0),Note::new(4,0.9),Note::new(7,0.8),Note::new(-12,0.7)], sc.clone()),
        Chord::new(1.0, vec![Note::new(-12,1.0),Note::new(-2,1.0),Note::new(2,0.9),Note::new(7,0.8)], sc.clone()),
        Chord::new(2.0, vec![Note::new(-10,1.0),Note::new(-3,1.0),Note::new(0,0.9),Note::new(6,0.8)], sc.clone()),
        Chord::new(3.0, vec![Note::new(-5,1.0),Note::new(0,1.0),Note::new(4,0.9),Note::new(7,0.8)], sc.clone()),
    ]
}

// ─────────────────────────────────────────────────────────────────────────────
//  Custom MusicPlayer implementations
// ─────────────────────────────────────────────────────────────────────────────

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
        if let Some(note) = TonalPlayer::get_chord_note(chord, 1.0 - base_intensity) {
            let vol = self.sampler.volume as f32 - 3.0 + base_intensity * 4.0;
            commands.spawn((
                SamplePlayer::new(self.sampler.handle.clone()).with_volume(Volume::Decibels(vol)),
                PlaybackSettings::default().with_speed(midi_diff_to_pitch(note.midi_note_diff)),
            ));
        }
    }
}

struct OstinatoStrings {
    sampler: Sampler,
    seq_idx: u32,
}

impl MusicPlayer for OstinatoStrings {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, chord: &Chord) {
        if base_intensity < 0.15 {
            return;
        }
        let step: u32 = if base_intensity < 0.42 { 4 } else if base_intensity < 0.68 { 2 } else { 1 };
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
            SamplePlayer::new(self.sampler.handle.clone()).with_volume(Volume::Decibels(vol)),
            PlaybackSettings::default().with_speed(midi_diff_to_pitch(note.midi_note_diff)),
        ));
    }
}

struct BrassSection {
    samples: Vec<Handle<AudioSample>>,
    volume: f32,
}

impl MusicPlayer for BrassSection {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, chord: &Chord) {
        if base_intensity < 0.42 || self.samples.is_empty() {
            return;
        }
        let on_quarter = beat.sixteenth == 0 && matches!(beat.beat, 0 | 2)
            || (beat.sixteenth == 0 && beat.beat % 2 == 1 && base_intensity > 0.75);
        let syncopated = base_intensity > 0.88 && beat.beat == 1 && beat.sixteenth == 2;
        if !on_quarter && !syncopated {
            return;
        }
        let num_voices: usize = if base_intensity > 0.85 { 3 } else if base_intensity > 0.62 { 2 } else { 1 };
        let min_strength = (1.0 - base_intensity).max(0.0);
        let strong_notes: Vec<&Note> = chord.chord_notes.iter()
            .filter(|n| n.strength >= min_strength)
            .take(num_voices)
            .collect();
        for (i, note) in strong_notes.iter().enumerate() {
            let vol = self.volume - i as f32 * 1.8;
            let si = (beat.beat as usize + beat.bar_count as usize * 3 + i) % self.samples.len();
            commands.spawn((
                SamplePlayer::new(self.samples[si].clone()).with_volume(Volume::Decibels(vol)),
                PlaybackSettings::default().with_speed(midi_diff_to_pitch(note.midi_note_diff)),
            ));
        }
    }
}

struct CymbalLayer {
    crash: Handle<AudioSample>,
    ride: Handle<AudioSample>,
    volume: f32,
}

impl MusicPlayer for CymbalLayer {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, _chord: &Chord) {
        if beat.beat == 0 && beat.sixteenth == 0 && base_intensity > 0.52 {
            if beat.bar_count % 2 == 0 || base_intensity > 0.82 {
                let vol = self.volume - 8.0 + (base_intensity - 0.52).max(0.0) / 0.48 * 9.0;
                commands.spawn((
                    SamplePlayer::new(self.crash.clone()).with_volume(Volume::Decibels(vol)),
                    PlaybackSettings::default(),
                ));
            }
        }
        if base_intensity > 0.48 && beat.sixteenth == 2 && beat.beat % 2 == 1 {
            commands.spawn((
                SamplePlayer::new(self.ride.clone())
                    .with_volume(Volume::Decibels(self.volume - 6.0 + base_intensity * 3.0)),
                PlaybackSettings::default(),
            ));
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Drum patterns
// ─────────────────────────────────────────────────────────────────────────────

const DRUM_PATTERNS: &[&str] = &[
    "kick_normal",
    "kick_march",
    "kick_half_time",
    "snare_normal",
    "snare_orchestral",
    "snare_fill",
    "snare_half_time",
    "snare_double_time",
    "hihat",
];

fn drum_pattern(name: &str) -> HashMap<(u32, u32), Note> {
    match name {
        "kick_normal" => HashMap::from([
            ((0, 0), Note::new(0, 1.0)),
            ((2, 0), Note::new(0, 1.0)),
            ((1, 2), Note::new(0, 0.3)),
            ((3, 2), Note::new(0, 0.3)),
        ]),
        "kick_march" => HashMap::from([
            ((0, 0), Note::new(0, 1.0)), ((1, 0), Note::new(0, 0.85)),
            ((2, 0), Note::new(0, 1.0)), ((3, 0), Note::new(0, 0.85)),
            ((0, 2), Note::new(0, 0.35)), ((2, 2), Note::new(0, 0.35)),
        ]),
        "kick_half_time" => generate_half_time_kick_beat(),
        "snare_normal" => HashMap::from([
            ((1, 0), Note::new(0, 1.0)),
            ((3, 0), Note::new(0, 1.0)),
        ]),
        "snare_orchestral" => HashMap::from([
            ((1, 0), Note::new(0, 1.0)), ((3, 0), Note::new(0, 1.0)),
            ((0, 2), Note::new(0, 0.12)), ((1, 2), Note::new(0, 0.12)),
            ((2, 2), Note::new(0, 0.12)), ((3, 2), Note::new(0, 0.12)),
        ]),
        "snare_fill" => generate_snare_fill_beat(),
        "snare_half_time" => generate_half_time_snare_beat(),
        "snare_double_time" => generate_double_time_snare_beat(),
        "hihat" => {
            let mut m = HashMap::new();
            for b in 0..4u32 {
                m.insert((b, 0), Note::new(0, 1.0));
                m.insert((b, 2), Note::new(0, 0.6));
            }
            m
        }
        _ => HashMap::from([((0, 0), Note::new(0, 1.0))]),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Runtime instrument config
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct InstrumentConfig {
    name: String,
    kind: String,
    samples: Vec<String>,
    volume: f32,
    muted: bool,
    pattern: String,
    record_bars: u32,
}

impl InstrumentConfig {
    fn kind_label(&self) -> &'static str {
        match self.kind.as_str() {
            "drummer" => "DRUMMER",
            "pad" => "PAD",
            "ostinato" => "OSTINATO",
            "brass" => "BRASS",
            "cymbal" => "CYMBAL",
            "soloist" => "SOLOIST",
            "bass" => "BASS",
            "arpeggiator" => "ARPEGGIO",
            _ => "?",
        }
    }

}

const INSTRUMENT_KINDS: &[&str] = &[
    "drummer", "pad", "ostinato", "brass", "cymbal", "soloist", "bass", "arpeggiator",
];

fn default_instruments() -> Vec<InstrumentConfig> {
    vec![
        ic("Kick",    "drummer",  "kick_normal",      &["samples/glicol/kick1.wav"],  1.0, 4),
        ic("Snare",   "drummer",  "snare_orchestral", &["samples/glicol/snare1.wav"], 0.8, 4),
        ic("Pad",     "pad",      "",                 &["samples/glicol/pad.wav"],    -1.0, 4),
        ic("Strings", "ostinato", "",                 &["samples/glicol/pluck.wav"], -2.0, 4),
        ic("Brass",   "brass",    "",                 &["samples/glicol/hit1.wav",
                                                        "samples/glicol/hit2.wav",
                                                        "samples/glicol/hit3.wav"],  -1.0, 4),
        ic("Cymbals", "cymbal",   "",                 &["samples/glicol/crash.wav",
                                                        "samples/glicol/ride.wav"],   0.0, 4),
        ic("Lead",    "soloist",  "",                 &["samples/glicol/sax.wav"],   -1.0, 4),
        ic("Bass",    "ostinato", "",                 &["samples/glicol/moog.wav"],  -1.0, 4),
    ]
}

fn ic(name: &str, kind: &str, pattern: &str, samples: &[&str], volume: f32, record_bars: u32) -> InstrumentConfig {
    InstrumentConfig {
        name: name.into(),
        kind: kind.into(),
        samples: samples.iter().map(|s| s.to_string()).collect(),
        volume,
        muted: false,
        pattern: pattern.into(),
        record_bars,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  App state
// ─────────────────────────────────────────────────────────────────────────────

#[derive(PartialEq)]
enum ChordChoice {
    DMinor,
    AMinor,
    CMajor,
    GMajor,
}

impl ChordChoice {
    fn name(&self) -> &'static str {
        match self {
            Self::DMinor => "d_minor",
            Self::AMinor => "a_minor",
            Self::CMajor => "c_major",
            Self::GMajor => "g_major",
        }
    }

    fn display(&self) -> &'static str {
        match self {
            Self::DMinor => "D minor  (i→VII→VI→VII)",
            Self::AMinor => "A minor  (i→III→VII→VI)",
            Self::CMajor => "C major  (I→V→vi→IV)",
            Self::GMajor => "G major  (I→IV→V→I)",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "a_minor" => Self::AMinor,
            "c_major" => Self::CMajor,
            "g_major" => Self::GMajor,
            _ => Self::DMinor,
        }
    }

    fn next(&self) -> Self {
        match self {
            Self::DMinor => Self::AMinor,
            Self::AMinor => Self::CMajor,
            Self::CMajor => Self::GMajor,
            Self::GMajor => Self::DMinor,
        }
    }
}

enum AppMode {
    Normal,
    Browser(BrowserState),
    AddNew { selected: usize },
    Save { input: String },
    Load { input: String },
}

struct BrowserState {
    root: PathBuf,
    dir: PathBuf,
    entries: Vec<BrowserEntry>,
    selected: usize,
    target_instrument: usize,
    append_sample: bool,
}

struct BrowserEntry {
    path: PathBuf,
    name: String,
    is_dir: bool,
}

impl BrowserState {
    fn refresh(&mut self) {
        self.entries.clear();
        if self.dir != self.root {
            self.entries.push(BrowserEntry { path: self.dir.parent().unwrap_or(&self.root).to_path_buf(), name: "..".into(), is_dir: true });
        }
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            let mut dirs = Vec::new();
            let mut files = Vec::new();
            for entry in rd.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') { continue; }
                if path.is_dir() {
                    dirs.push(BrowserEntry { path, name, is_dir: true });
                } else if name.to_lowercase().ends_with(".wav") || name.to_lowercase().ends_with(".mp3") {
                    files.push(BrowserEntry { path, name, is_dir: false });
                }
            }
            dirs.sort_by(|a, b| a.name.cmp(&b.name));
            files.sort_by(|a, b| a.name.cmp(&b.name));
            self.entries.extend(dirs);
            self.entries.extend(files);
        }
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
    }

    fn relative_path(&self, abs: &std::path::Path) -> String {
        abs.strip_prefix(&self.root)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| abs.to_string_lossy().into_owned())
    }
}

#[derive(Resource)]
struct AppState {
    instruments: Vec<InstrumentConfig>,
    bpm: f32,
    chord: ChordChoice,
    selected: usize,
    mode: AppMode,
    status: String,
    pending: bool,
    entities: Vec<Entity>,
    do_rebuild: bool,
    do_chord_update: bool,
    last_draw: Option<Instant>,
}

// ─────────────────────────────────────────────────────────────────────────────
//  Bevy resource wrappers
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Resource)]
struct TuiTerminal(Terminal<CrosstermBackend<Stdout>>);

// ─────────────────────────────────────────────────────────────────────────────
//  Spawn musician from config
// ─────────────────────────────────────────────────────────────────────────────

fn spawn_musicians(
    state: &mut AppState,
    commands: &mut Commands,
    asset_server: &AssetServer,
) {
    for entity in state.entities.drain(..) {
        commands.entity(entity).despawn();
    }

    for cfg in &state.instruments {
        let load = |path: String| -> Handle<AudioSample> { asset_server.load(path) };
        let s = |idx: usize| -> Sampler {
            Sampler {
                handle: load(cfg.samples.get(idx).cloned().unwrap_or_default()),
                volume: cfg.volume as f64,
            }
        };

        macro_rules! spawn {
            ($player:expr) => {{
                if cfg.muted {
                    commands.spawn((Musician::new(cfg.name.clone(), $player), Muted)).id()
                } else {
                    commands.spawn(Musician::new(cfg.name.clone(), $player)).id()
                }
            }};
        }

        let entity = match cfg.kind.as_str() {
            "drummer"    => spawn!(Drummer::new(s(0), drum_pattern(&cfg.pattern))),
            "pad"        => spawn!(OrchestraPad { sampler: s(0) }),
            "ostinato" | "bass" => spawn!(OstinatoStrings { sampler: s(0), seq_idx: 0 }),
            "brass"      => spawn!(BrassSection {
                samples: cfg.samples.iter().cloned().map(load).collect(),
                volume: cfg.volume,
            }),
            "cymbal"     => spawn!(CymbalLayer {
                crash: load(cfg.samples.get(0).cloned().unwrap_or_default()),
                ride:  load(cfg.samples.get(1).cloned().unwrap_or_default()),
                volume: cfg.volume,
            }),
            "soloist"    => spawn!(Soloist::new(s(0), cfg.record_bars)),
            "arpeggiator"=> spawn!(Arpeggiator::new(s(0))),
            _            => spawn!(Bassist::new(s(0))),
        };
        state.entities.push(entity);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Systems
// ─────────────────────────────────────────────────────────────────────────────

fn setup(mut commands: Commands, mut state: ResMut<AppState>, asset_server: Res<AssetServer>) {
    let (chords, len) = chords_for(state.chord.name());
    commands.insert_resource(Conductor { chords, chord_length_bars: len });
    spawn_musicians(&mut state, &mut commands, &asset_server);
}

fn handle_input(
    mut state: ResMut<AppState>,
    mut clock: ResMut<Clock>,
    mut intensity: ResMut<Intensity>,
) {
    while event::poll(Duration::ZERO).unwrap_or(false) {
        let Ok(Event::Key(key)) = event::read() else { continue; };
        if key.kind != KeyEventKind::Press { continue; }

        match &mut state.mode {
            AppMode::Normal => handle_normal_input(&mut state, key.code, key.modifiers, &mut clock, &mut intensity),
            AppMode::Browser(_) => handle_browser_input(&mut state, key.code),
            AppMode::AddNew { .. } => handle_add_input(&mut state, key.code),
            AppMode::Save { .. } => handle_save_input(&mut state, key.code),
            AppMode::Load { .. } => handle_load_input(&mut state, key.code),
        }
    }
}

fn handle_normal_input(
    state: &mut AppState,
    code: KeyCode,
    mods: KeyModifiers,
    clock: &mut Clock,
    intensity: &mut Intensity,
) {
    let n = state.instruments.len();
    match code {
        KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
            std::process::exit(0);
        }
        KeyCode::Up => {
            if n > 0 { state.selected = state.selected.saturating_sub(1); }
        }
        KeyCode::Down => {
            if n > 0 { state.selected = (state.selected + 1).min(n - 1); }
        }
        // Volume
        KeyCode::Char('v') | KeyCode::Char('V') => {
            if let Some(i) = state.instruments.get_mut(state.selected) {
                i.volume = (i.volume - 1.0).max(-24.0);
                state.pending = true;
            }
        }
        KeyCode::Char('b') | KeyCode::Char('B') => {
            if let Some(i) = state.instruments.get_mut(state.selected) {
                i.volume = (i.volume + 1.0).min(6.0);
                state.pending = true;
            }
        }
        // Mute
        KeyCode::Char('m') | KeyCode::Char('M') => {
            if let Some(i) = state.instruments.get_mut(state.selected) {
                i.muted = !i.muted;
                state.pending = true;
            }
        }
        // Cycle drum pattern
        KeyCode::Char('p') | KeyCode::Char('P') => {
            if let Some(i) = state.instruments.get_mut(state.selected) {
                if i.kind == "drummer" {
                    let idx = DRUM_PATTERNS.iter().position(|&p| p == i.pattern).unwrap_or(0);
                    i.pattern = DRUM_PATTERNS[(idx + 1) % DRUM_PATTERNS.len()].into();
                    state.pending = true;
                }
            }
        }
        // Browse primary sample
        KeyCode::Char('f') | KeyCode::Char('F') => {
            let root = std::env::current_dir().unwrap_or_default().join("assets");
            let start = if root.exists() { root.clone() } else { std::env::current_dir().unwrap_or_default() };
            let mut browser = BrowserState {
                root, dir: start, entries: Vec::new(), selected: 0,
                target_instrument: state.selected, append_sample: false,
            };
            browser.refresh();
            state.mode = AppMode::Browser(browser);
        }
        // Browse secondary sample / add brass sample
        KeyCode::Char('g') | KeyCode::Char('G') => {
            let instr_kind = state.instruments.get(state.selected).map(|i| i.kind.as_str().to_owned());
            if matches!(instr_kind.as_deref(), Some("brass") | Some("cymbal")) {
                let root = std::env::current_dir().unwrap_or_default().join("assets");
                let start = if root.exists() { root.clone() } else { std::env::current_dir().unwrap_or_default() };
                let append = instr_kind.as_deref() == Some("brass");
                let mut browser = BrowserState {
                    root, dir: start, entries: Vec::new(), selected: 0,
                    target_instrument: state.selected, append_sample: append,
                };
                browser.refresh();
                state.mode = AppMode::Browser(browser);
            }
        }
        // Remove last sample (brass)
        KeyCode::Char('x') | KeyCode::Char('X') if mods.contains(KeyModifiers::SHIFT) => {
            if let Some(i) = state.instruments.get_mut(state.selected) {
                if i.samples.len() > 1 { i.samples.pop(); state.pending = true; }
            }
        }
        // Add instrument
        KeyCode::Char('n') | KeyCode::Char('N') => {
            state.mode = AppMode::AddNew { selected: 0 };
        }
        // Remove instrument
        KeyCode::Delete => {
            if n > 0 {
                state.instruments.remove(state.selected);
                state.selected = state.selected.min(state.instruments.len().saturating_sub(1));
                state.pending = true;
            }
        }
        // Apply
        KeyCode::Char('a') | KeyCode::Char('A') => {
            state.do_rebuild = true;
            state.do_chord_update = true;
            state.pending = false;
            state.status = "Applied.".into();
        }
        // Intensity
        KeyCode::Char('i') => intensity.0 = (intensity.0 + 0.05).min(1.0),
        KeyCode::Char('o') => intensity.0 = (intensity.0 - 0.05).max(0.0),
        // BPM
        KeyCode::Char('+') | KeyCode::Char('=') => {
            clock.tempo_bpm = (clock.tempo_bpm + 5.0).min(240.0);
            clock.beat_length = 60.0 / (clock.tempo_bpm * clock.beats);
            state.bpm = clock.tempo_bpm;
        }
        KeyCode::Char('-') => {
            clock.tempo_bpm = (clock.tempo_bpm - 5.0).max(40.0);
            clock.beat_length = 60.0 / (clock.tempo_bpm * clock.beats);
            state.bpm = clock.tempo_bpm;
        }
        // Cycle chord
        KeyCode::Char('c') | KeyCode::Char('C') => {
            state.chord = state.chord.next();
            state.do_chord_update = true;
            state.status = format!("Chord: {}", state.chord.display());
        }
        // Save
        KeyCode::Char('s') | KeyCode::Char('S') => {
            state.mode = AppMode::Save { input: "band.toml".into() };
        }
        // Load
        KeyCode::Char('l') | KeyCode::Char('L') => {
            state.mode = AppMode::Load { input: "band.toml".into() };
        }
        _ => {}
    }
}

fn handle_browser_input(state: &mut AppState, code: KeyCode) {
    let AppMode::Browser(ref mut browser) = state.mode else { return; };
    match code {
        KeyCode::Esc => { state.mode = AppMode::Normal; }
        KeyCode::Up => {
            if browser.selected > 0 { browser.selected -= 1; }
        }
        KeyCode::Down => {
            if browser.selected + 1 < browser.entries.len() { browser.selected += 1; }
        }
        KeyCode::Enter => {
            if let Some(entry) = browser.entries.get(browser.selected) {
                if entry.is_dir {
                    let new_dir = entry.path.clone();
                    browser.dir = new_dir;
                    browser.selected = 0;
                    browser.refresh();
                } else {
                    // File selected
                    let path_str = browser.relative_path(&entry.path);
                    let target = browser.target_instrument;
                    let append = browser.append_sample;
                    state.mode = AppMode::Normal;
                    if let Some(instr) = state.instruments.get_mut(target) {
                        if append {
                            instr.samples.push(path_str);
                        } else {
                            if instr.samples.is_empty() { instr.samples.push(path_str); }
                            else { instr.samples[0] = path_str; }
                        }
                        state.pending = true;
                        state.status = format!("Sample updated for '{}'", instr.name);
                    }
                }
            }
        }
        KeyCode::Backspace | KeyCode::Left => {
            // Go up
            let AppMode::Browser(ref mut browser) = state.mode else { return; };
            if browser.dir != browser.root {
                if let Some(parent) = browser.dir.parent().map(|p| p.to_path_buf()) {
                    browser.dir = parent;
                    browser.selected = 0;
                    browser.refresh();
                }
            }
        }
        _ => {}
    }
}

fn handle_add_input(state: &mut AppState, code: KeyCode) {
    let AppMode::AddNew { ref mut selected } = state.mode else { return; };
    match code {
        KeyCode::Esc => { state.mode = AppMode::Normal; }
        KeyCode::Up => { if *selected > 0 { *selected -= 1; } }
        KeyCode::Down => { if *selected + 1 < INSTRUMENT_KINDS.len() { *selected += 1; } }
        KeyCode::Enter => {
            let kind = INSTRUMENT_KINDS[*selected].to_string();
            let pattern = if kind == "drummer" { "kick_normal" } else { "" }.to_string();
            let new_instr = InstrumentConfig {
                name: format!("New {}", kind[..1].to_uppercase() + &kind[1..]),
                kind,
                samples: Vec::new(),
                volume: 0.0,
                muted: false,
                pattern,
                record_bars: 4,
            };
            state.instruments.push(new_instr);
            state.selected = state.instruments.len() - 1;
            state.pending = true;
            state.mode = AppMode::Normal;
            state.status = "Instrument added — set a sample with [F], then [A] to apply.".into();
        }
        _ => {}
    }
}

fn handle_save_input(state: &mut AppState, code: KeyCode) {
    let AppMode::Save { ref mut input } = state.mode else { return; };
    match code {
        KeyCode::Esc => { state.mode = AppMode::Normal; }
        KeyCode::Backspace => { input.pop(); }
        KeyCode::Char(c) => { input.push(c); }
        KeyCode::Enter => {
            let filename = input.clone();
            let file = to_machine_file(state);
            match toml::to_string_pretty(&file) {
                Ok(toml_str) => match std::fs::write(&filename, toml_str) {
                    Ok(_) => { state.status = format!("Saved to '{filename}'"); }
                    Err(e) => { state.status = format!("Save error: {e}"); }
                },
                Err(e) => { state.status = format!("Serialise error: {e}"); }
            }
            state.mode = AppMode::Normal;
        }
        _ => {}
    }
}

fn handle_load_input(state: &mut AppState, code: KeyCode) {
    let AppMode::Load { ref mut input } = state.mode else { return; };
    match code {
        KeyCode::Esc => { state.mode = AppMode::Normal; }
        KeyCode::Backspace => { input.pop(); }
        KeyCode::Char(c) => { input.push(c); }
        KeyCode::Enter => {
            let filename = input.clone();
            match std::fs::read_to_string(&filename) {
                Ok(content) => match toml::from_str::<MachineFile>(&content) {
                    Ok(mf) => {
                        from_machine_file(state, mf);
                        state.do_rebuild = true;
                        state.do_chord_update = true;
                        state.status = format!("Loaded '{filename}'");
                    }
                    Err(e) => { state.status = format!("Parse error: {e}"); }
                },
                Err(e) => { state.status = format!("Load error: {e}"); }
            }
            state.mode = AppMode::Normal;
        }
        _ => {}
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  TOML conversion helpers
// ─────────────────────────────────────────────────────────────────────────────

fn to_machine_file(state: &AppState) -> MachineFile {
    MachineFile {
        settings: SettingsFile {
            bpm: state.bpm,
            beats: 4,
            note_type: 4,
            chord: state.chord.name().into(),
        },
        instruments: state.instruments.iter().map(|i| InstrumentFile {
            name: i.name.clone(),
            kind: i.kind.clone(),
            samples: i.samples.clone(),
            volume: i.volume,
            muted: i.muted,
            pattern: i.pattern.clone(),
            record_bars: i.record_bars,
        }).collect(),
    }
}

fn from_machine_file(state: &mut AppState, mf: MachineFile) {
    state.bpm = mf.settings.bpm;
    state.chord = ChordChoice::from_str(&mf.settings.chord);
    state.instruments = mf.instruments.iter().map(|f| InstrumentConfig {
        name: f.name.clone(),
        kind: f.kind.clone(),
        samples: f.samples.clone(),
        volume: f.volume,
        muted: f.muted,
        pattern: f.pattern.clone(),
        record_bars: f.record_bars,
    }).collect();
    state.selected = state.selected.min(state.instruments.len().saturating_sub(1));
    state.pending = false;
}

// ─────────────────────────────────────────────────────────────────────────────
//  Rebuild / chord-update systems
// ─────────────────────────────────────────────────────────────────────────────

fn apply_changes(
    mut state: ResMut<AppState>,
    mut commands: Commands,
    mut clock: ResMut<Clock>,
    asset_server: Res<AssetServer>,
) {
    if state.do_chord_update {
        state.do_chord_update = false;
        let (chords, len) = chords_for(state.chord.name());
        commands.insert_resource(Conductor { chords, chord_length_bars: len });
    }
    if state.do_rebuild {
        state.do_rebuild = false;
        clock.tempo_bpm = state.bpm;
        clock.beat_length = 60.0 / (state.bpm * clock.beats);
        spawn_musicians(&mut state, &mut commands, &asset_server);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  TUI rendering
// ─────────────────────────────────────────────────────────────────────────────

fn draw_tui(
    mut terminal: ResMut<TuiTerminal>,
    mut state: ResMut<AppState>,
    intensity: Res<Intensity>,
    clock: Res<Clock>,
) {
    let now = Instant::now();
    if state.last_draw.map_or(false, |t| now.duration_since(t).as_millis() < 33) {
        return;
    }
    state.last_draw = Some(now);

    let intensity_val = intensity.0;
    let bpm = clock.tempo_bpm;
    let bar = clock.bar_count;
    let beat = clock.beat;

    let _ = terminal.0.draw(|frame| {
        let area = frame.area();
        let rows = Layout::vertical([
            Constraint::Length(3),  // header
            Constraint::Min(10),    // body
            Constraint::Length(5),  // footer
        ]).split(area);

        let pending_tag = if state.pending {
            Span::styled(" * PENDING ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
        } else {
            Span::raw("           ")
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" RUSTY MUSIC MACHINE ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw("│  "),
                Span::styled("BPM ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:.0}", bpm), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("  │  "),
                Span::styled(state.chord.display(), Style::default().fg(Color::Magenta)),
                Span::raw("  │  Bar "),
                Span::styled(format!("{:04}", bar + 1), Style::default().fg(Color::Green)),
                Span::raw(" Beat "),
                Span::styled((beat + 1).to_string(), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw("  "),
                pending_tag,
            ])).block(Block::new().borders(Borders::ALL)),
            rows[0],
        );

        // Body: instruments left, detail right
        let cols = Layout::horizontal([Constraint::Percentage(52), Constraint::Percentage(48)])
            .split(rows[1]);

        // ── Instrument list ───────────────────────────────────────────────────
        let items: Vec<ListItem> = state.instruments.iter().enumerate().map(|(i, instr)| {
            let selected = i == state.selected;
            let mute_tag = if instr.muted { "[M]" } else { "   " };
            let vol_str = format!("{:+.0}dB", instr.volume);
            let line = Line::from(vec![
                Span::styled(
                    format!(" {}", if selected { "▶" } else { " " }),
                    if selected { Style::default().fg(Color::Yellow) } else { Style::default() },
                ),
                Span::styled(
                    format!(" {:<14}", &instr.name),
                    if instr.muted {
                        Style::default().fg(Color::DarkGray)
                    } else if selected {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ),
                Span::styled(
                    format!("[{:<8}]", instr.kind_label()),
                    Style::default().fg(kind_color(&instr.kind)),
                ),
                Span::styled(format!(" {:>6}", vol_str), Style::default().fg(Color::DarkGray)),
                Span::styled(format!(" {}", mute_tag), Style::default().fg(Color::Red)),
            ]);
            ListItem::new(line)
        }).collect();

        let mut list_state = ListState::default().with_selected(Some(state.selected));
        frame.render_stateful_widget(
            List::new(items)
                .block(Block::new().borders(Borders::ALL).title(" Instruments  [N]Add  [Del]Remove "))
                .highlight_style(Style::default()),
            cols[0],
            &mut list_state,
        );

        // ── Detail panel ──────────────────────────────────────────────────────
        let detail_block = Block::new().borders(Borders::ALL).title(" Selected Instrument ");
        if let Some(instr) = state.instruments.get(state.selected) {
            let mut lines = vec![
                Line::from(vec![
                    Span::styled(" Name:   ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&instr.name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled(" Kind:   ", Style::default().fg(Color::DarkGray)),
                    Span::styled(instr.kind_label(), Style::default().fg(kind_color(&instr.kind)).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled(" Volume: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{:+.1} dB  [V] − [B] +", instr.volume), Style::default().fg(Color::Cyan)),
                ]),
                Line::from(vec![
                    Span::styled(" Muted:  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(if instr.muted { "Yes  [M]" } else { "No   [M]" },
                        if instr.muted { Style::default().fg(Color::Red) } else { Style::default().fg(Color::Green) }),
                ]),
                Line::from(""),
            ];

            if instr.kind == "drummer" {
                lines.push(Line::from(vec![
                    Span::styled(" Pattern:", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!(" {}  [P]cycle", instr.pattern), Style::default().fg(Color::Yellow)),
                ]));
                lines.push(Line::from(""));
            }

            // Samples
            for (idx, sample) in instr.samples.iter().enumerate() {
                let label = match (instr.kind.as_str(), idx) {
                    ("cymbal", 0) => " Crash:  ",
                    ("cymbal", 1) => " Ride:   ",
                    ("brass", 0) => " Sample1:",
                    ("brass", i) => if i == 1 { " Sample2:" } else if i == 2 { " Sample3:" } else { " Sample+:" },
                    _ => " Sample: ",
                };
                let fname = std::path::Path::new(sample)
                    .file_name().map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "(none)".into());
                lines.push(Line::from(vec![
                    Span::styled(label, Style::default().fg(Color::DarkGray)),
                    Span::styled(fname, Style::default().fg(Color::White)),
                ]));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(" [F] ", Style::default().fg(Color::Yellow)),
                Span::styled("primary sample  ", Style::default().fg(Color::DarkGray)),
                if matches!(instr.kind.as_str(), "brass" | "cymbal") {
                    Span::styled("[G] add/2nd sample", Style::default().fg(Color::Yellow))
                } else {
                    Span::raw("")
                },
            ]));
            if instr.kind == "brass" {
                lines.push(Line::from(vec![
                    Span::styled(" [Shift+X] ", Style::default().fg(Color::Yellow)),
                    Span::styled("remove last sample", Style::default().fg(Color::DarkGray)),
                ]));
            }

            frame.render_widget(Paragraph::new(lines).block(detail_block), cols[1]);
        } else {
            frame.render_widget(
                Paragraph::new(" No instruments — press [N] to add one.")
                    .block(detail_block),
                cols[1],
            );
        }

        // ── Footer ────────────────────────────────────────────────────────────
        let footer_inner = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Length(3)])
            .split(rows[2]);

        frame.render_widget(
            Gauge::default()
                .block(Block::new().borders(Borders::LEFT | Borders::RIGHT | Borders::TOP).title(" Intensity [I][O] "))
                .gauge_style(Style::default().fg(Color::Yellow).bg(Color::DarkGray))
                .ratio(intensity_val as f64)
                .label(format!("{:.2}", intensity_val)),
            footer_inner[0],
        );

        let status_color = if state.pending { Color::Red } else { Color::DarkGray };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" Status: ", Style::default().fg(Color::DarkGray)),
                Span::styled(&state.status, Style::default().fg(status_color)),
            ])).block(Block::new().borders(Borders::LEFT | Borders::RIGHT)),
            footer_inner[1],
        );

        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    Span::styled(" [↑↓]", Style::default().fg(Color::Yellow)), Span::raw(" select  "),
                    Span::styled("[V][B]", Style::default().fg(Color::Yellow)), Span::raw(" vol −/+  "),
                    Span::styled("[M]", Style::default().fg(Color::Yellow)), Span::raw(" mute  "),
                    Span::styled("[P]", Style::default().fg(Color::Yellow)), Span::raw(" pattern  "),
                    Span::styled("[F][G]", Style::default().fg(Color::Yellow)), Span::raw(" sample  "),
                    Span::styled("[N]", Style::default().fg(Color::Yellow)), Span::raw(" add  "),
                    Span::styled("[Del]", Style::default().fg(Color::Yellow)), Span::raw(" remove"),
                ]),
                Line::from(vec![
                    Span::styled(" [I][O]", Style::default().fg(Color::Cyan)), Span::raw(" intensity  "),
                    Span::styled("[+][-]", Style::default().fg(Color::Cyan)), Span::raw(" BPM  "),
                    Span::styled("[C]", Style::default().fg(Color::Magenta)), Span::raw(" chord  "),
                    Span::styled("[A]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)), Span::raw(" APPLY  "),
                    Span::styled("[S]", Style::default().fg(Color::Yellow)), Span::raw(" save  "),
                    Span::styled("[L]", Style::default().fg(Color::Yellow)), Span::raw(" load  "),
                    Span::styled("[Q]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)), Span::raw(" quit"),
                ]),
            ]).block(Block::new().borders(Borders::ALL)),
            footer_inner[2],
        );

        // ── Overlays ──────────────────────────────────────────────────────────
        match &state.mode {
            AppMode::Browser(browser) => render_browser(frame, area, browser),
            AppMode::AddNew { selected } => render_add_new(frame, area, *selected),
            AppMode::Save { input } => render_dialog(frame, area, "Save to file", input, "Enter filename and press Enter"),
            AppMode::Load { input } => render_dialog(frame, area, "Load from file", input, "Enter filename and press Enter"),
            AppMode::Normal => {}
        }
    });
}

fn kind_color(kind: &str) -> Color {
    match kind {
        "drummer" => Color::Red,
        "pad" => Color::Blue,
        "ostinato" => Color::Cyan,
        "bass" => Color::LightBlue,
        "brass" => Color::Yellow,
        "cymbal" => Color::Magenta,
        "soloist" => Color::Green,
        "arpeggiator" => Color::LightGreen,
        _ => Color::White,
    }
}

fn render_browser(frame: &mut ratatui::Frame, area: Rect, browser: &BrowserState) {
    let popup = centered_rect(70, 75, area);
    frame.render_widget(Clear, popup);

    let dir_str = browser.dir.to_string_lossy();
    let title = format!(" Sample Browser — {dir_str} ");

    let items: Vec<ListItem> = browser.entries.iter().enumerate().map(|(i, e)| {
        let selected = i == browser.selected;
        let color = if e.is_dir { Color::Yellow } else { Color::White };
        let icon = if e.is_dir { "📁 " } else { "🔉 " };
        let style = if selected {
            Style::default().fg(color).add_modifier(Modifier::BOLD).bg(Color::DarkGray)
        } else {
            Style::default().fg(color)
        };
        ListItem::new(Line::from(Span::styled(format!(" {} {}", icon, e.name), style)))
    }).collect();

    let rows = Layout::vertical([Constraint::Min(5), Constraint::Length(3)]).split(popup);
    let mut list_state = ListState::default().with_selected(Some(browser.selected));
    frame.render_stateful_widget(
        List::new(items).block(Block::new().borders(Borders::ALL).title(title.as_str())),
        rows[0],
        &mut list_state,
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" [↑↓] ", Style::default().fg(Color::Yellow)), Span::raw("navigate  "),
            Span::styled("[Enter] ", Style::default().fg(Color::Green)), Span::raw("select/enter dir  "),
            Span::styled("[Backspace] ", Style::default().fg(Color::Yellow)), Span::raw("up  "),
            Span::styled("[Esc] ", Style::default().fg(Color::Red)), Span::raw("cancel"),
        ])).block(Block::new().borders(Borders::ALL)),
        rows[1],
    );
}

fn render_add_new(frame: &mut ratatui::Frame, area: Rect, selected: usize) {
    let popup = centered_rect(36, 50, area);
    frame.render_widget(Clear, popup);

    let items: Vec<ListItem> = INSTRUMENT_KINDS.iter().enumerate().map(|(i, &kind)| {
        let sel = i == selected;
        let style = if sel {
            Style::default().fg(kind_color(kind)).add_modifier(Modifier::BOLD).bg(Color::DarkGray)
        } else {
            Style::default().fg(kind_color(kind))
        };
        ListItem::new(Line::from(Span::styled(
            format!(" {} {}", if sel { "▶" } else { " " }, kind),
            style,
        )))
    }).collect();

    let rows = Layout::vertical([Constraint::Min(5), Constraint::Length(3)]).split(popup);
    let mut list_state = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(
        List::new(items).block(Block::new().borders(Borders::ALL).title(" Add Instrument ")),
        rows[0],
        &mut list_state,
    );
    frame.render_widget(
        Paragraph::new(" [↑↓] select  [Enter] add  [Esc] cancel")
            .block(Block::new().borders(Borders::ALL)),
        rows[1],
    );
}

fn render_dialog(frame: &mut ratatui::Frame, area: Rect, title: &str, input: &str, hint: &str) {
    let popup = centered_rect(50, 20, area);
    frame.render_widget(Clear, popup);
    let rows = Layout::vertical([Constraint::Length(3), Constraint::Length(3)]).split(popup);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw(" "),
            Span::styled(input, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("█", Style::default().fg(Color::Yellow)), // cursor
        ])).block(Block::new().borders(Borders::ALL).title(format!(" {title} "))),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {hint}"), Style::default().fg(Color::DarkGray)),
        ])).block(Block::new().borders(Borders::ALL)),
        rows[1],
    );
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ]).split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ]).split(vertical[1])[1]
}

// ─────────────────────────────────────────────────────────────────────────────
//  Cleanup guard
// ─────────────────────────────────────────────────────────────────────────────

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Entry point
// ─────────────────────────────────────────────────────────────────────────────

fn main() {
    enable_raw_mode().expect("enable raw mode");
    execute!(io::stdout(), EnterAlternateScreen).expect("enter alternate screen");
    let _guard = TerminalGuard;

    let terminal = Terminal::new(CrosstermBackend::new(io::stdout())).expect("terminal");

    // Optional: load a file passed as CLI arg
    let file_arg = std::env::args().nth(1);
    let (instruments, bpm, chord) = if let Some(path) = &file_arg {
        match std::fs::read_to_string(path).and_then(|s| {
            toml::from_str::<MachineFile>(&s).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        }) {
            Ok(mf) => {
                let instruments = mf.instruments.iter().map(|f| InstrumentConfig {
                    name: f.name.clone(), kind: f.kind.clone(), samples: f.samples.clone(),
                    volume: f.volume, muted: f.muted, pattern: f.pattern.clone(), record_bars: f.record_bars,
                }).collect();
                (instruments, mf.settings.bpm, ChordChoice::from_str(&mf.settings.chord))
            }
            Err(e) => {
                eprintln!("Could not load '{path}': {e}");
                (default_instruments(), 88.0, ChordChoice::DMinor)
            }
        }
    } else {
        (default_instruments(), 88.0, ChordChoice::DMinor)
    };

    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(1))),
            AssetPlugin::default(),
        ))
        .add_plugins(MusicPlugin { beats: 4, note_type: 4, bpm })
        .insert_resource(TuiTerminal(terminal))
        .insert_resource(Intensity(0.3))
        .insert_resource(AppState {
            instruments,
            bpm,
            chord,
            selected: 0,
            mode: AppMode::Normal,
            status: "Ready — [A] to apply, [S] to save, [L] to load.".into(),
            pending: false,
            entities: Vec::new(),
            do_rebuild: true,
            do_chord_update: true,
            last_draw: None,
        })
        .add_systems(Startup, setup)
        .add_systems(Update, (handle_input, apply_changes.after(handle_input), draw_tui.after(apply_changes)))
        .run();
}
