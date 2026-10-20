//! improv_orchestra — real_orchestra's multisampled orchestra, but *nothing* is
//! written out: harmony, form, melody, bass lines and drum grooves are all
//! generated from a seed, following the improvisation chapter in `docs/book.md`.
//!
//! The three layers from the book:
//!
//!   Conductor   A seeded composer writes a 32-bar AABA *lead sheet*: chord
//!               progressions are Markov walks over the D-minor scale degrees
//!               with cadences at section ends. Every chorus it *develops* the
//!               sheet a little (re-harmonises a bar, rewrites the bridge).
//!   Musicians   Sequencers that improvise against the lead sheet:
//!                 · Soloist   — invents a 2-bar motif, then develops it through
//!                               the section (repeat, sequence, inversion,
//!                               retrograde, fragmentation, cadence). Motif A is
//!                               remembered and mutated each chorus.
//!                 · Bass      — a remembered rhythm per section, chord tones,
//!                               chromatic approach notes into chord changes.
//!                 · Pads      — voice-led chords (minimal movement between them).
//!                 · Arpeggio  — book "chimes" with seeded modes / index cells.
//!                 · Drums     — Euclidean grooves per section, ghost notes, fills
//!                               generated fresh at every section end.
//!   Instruments The real multisamples from `assets/all-samples`.
//!
//! Everything random is drawn from per-musician RNGs derived from one seed, so
//! the same seed (and the same intensity moves) gives the same performance.
//!
//! Controls:
//!   ↑ / ↓     raise / lower intensity manually
//!   Space      toggle auto-build (64-bar rise-and-fall cycle)
//!   n          new seed (new song, starts on the next bar)
//!   r          restart the current seed from the top
//!   q / Esc    quit
//!
//! Pass a seed to replay a song: `cargo run --example improv_orchestra -- 1234`

use std::collections::VecDeque;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use bevy::app::ScheduleRunnerPlugin;
use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy_seedling::prelude::{AudioSample, PlaybackSettings, SamplePlayer, Volume};
use rand::rngs::StdRng;
use rand::seq::IndexedRandom;
use rand::{RngExt, SeedableRng};

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

use rusty_music::clock::{progress_clock_system, Beat, Clock};
use rusty_music::musicians::conductor::Conductor;
use rusty_music::musicians::{midi_diff_to_pitch, Chord, MusicPlayer, Musician, Note, TonalPlayer};
use rusty_music::player::{play_sound_on_the_beat, Intensity};
use rusty_music::sampler::{Dynamic, MultiSampler, SampleLibrary, VoiceFilter};
use rusty_music::MusicPlugin;

const BPM: f32 = 88.0;
const SECTION_BARS: u32 = 8;
const FORM: [Part; 4] = [Part::A, Part::A, Part::B, Part::A];
const FORM_BARS: u32 = SECTION_BARS * FORM.len() as u32;
/// Absolute MIDI note of the tonic (D4). All "semis" below are relative to it.
const TONIC: i32 = 62;

// ── Music theory ──────────────────────────────────────────────────────────────

/// D natural minor, in semitones above the tonic.
const MINOR: [i32; 7] = [0, 2, 3, 5, 7, 8, 10];
const NUMERALS: [&str; 7] = ["i", "ii°", "III", "iv", "v", "VI", "VII"];
const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];

/// Semitone offset of diatonic scale position `pos` (0 = tonic, wraps octaves).
fn scale_semis(pos: i32) -> i32 {
    MINOR[pos.rem_euclid(7) as usize] + 12 * pos.div_euclid(7)
}

fn midi_name(midi: i32) -> String {
    format!("{}{}", NOTE_NAMES[midi.rem_euclid(12) as usize], midi.div_euclid(12) - 1)
}

/// A diatonic chord in D minor. `major` raises the third (harmonic-minor V).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Degree {
    idx: i32,
    major: bool,
}

impl Degree {
    const fn new(idx: i32) -> Self {
        Self { idx, major: false }
    }

    const fn dominant() -> Self {
        Self { idx: 4, major: true }
    }

    /// Root in semis, folded into -5..=6 so roots stay in one register.
    fn root(self) -> i32 {
        let r = scale_semis(self.idx);
        if r > 6 { r - 12 } else { r }
    }

    /// Root, third, fifth, seventh in semis.
    fn tones(self) -> [i32; 4] {
        let r = self.root();
        let above = |k: i32| scale_semis(self.idx + k) - scale_semis(self.idx);
        [r, r + above(2) + self.major as i32, r + above(4), r + above(6)]
    }

    fn is_tone(self, semis: i32, sevenths: bool) -> bool {
        let n = if sevenths { 4 } else { 3 };
        self.tones()[..n].iter().any(|t| (t - semis).rem_euclid(12) == 0)
    }

    fn numeral(self) -> &'static str {
        if self.major && self.idx == 4 { "V" } else { NUMERALS[self.idx as usize] }
    }

    fn name(self) -> String {
        let [r, t, f, _] = self.tones();
        let quality = match (t - r, f - r) {
            (4, 7) => "",
            (3, 7) => "m",
            _ => "°",
        };
        format!("{}{quality}", NOTE_NAMES[(TONIC + r).rem_euclid(12) as usize])
    }
}

/// Pitch (semis) of scale position `pos` under chord `d` — the V chord borrows
/// the raised leading tone (C#) from harmonic minor.
fn pitch_of(pos: i32, d: Degree) -> i32 {
    let s = scale_semis(pos);
    if d.major && s.rem_euclid(12) == 10 { s + 1 } else { s }
}

/// Nearest scale position to `pos` that is a chord tone of `d`.
fn snap_to_chord(pos: i32, d: Degree) -> i32 {
    [0, 1, -1, 2, -2, 3, -3]
        .into_iter()
        .map(|o| pos + o)
        .find(|&p| d.is_tone(pitch_of(p, d), false))
        .unwrap_or(pos)
}

/// Nearest scale position to `pos` that is the chord root.
fn snap_to_root(pos: i32, d: Degree) -> i32 {
    [0, 1, -1, 2, -2, 3, -3, 4, -4]
        .into_iter()
        .map(|o| pos + o)
        .find(|&p| (pitch_of(p, d) - d.root()).rem_euclid(12) == 0)
        .unwrap_or(pos)
}

/// The rusty_music `Chord` for a degree: strength-weighted chord + scale tones.
fn build_chord(pos_bars: f32, d: Degree) -> Chord {
    let [r, t, f, s] = d.tones();
    let chord_notes = vec![
        Note::new(r - 12, 1.0), Note::new(r, 1.0), Note::new(t, 0.9), Note::new(f, 0.8),
        Note::new(f - 12, 0.7), Note::new(s, 0.45), Note::new(r + 12, 0.4), Note::new(t + 12, 0.3),
    ];
    let scale_notes = (-7..=8)
        .map(|p| {
            let semis = pitch_of(p, d);
            let strength = if (semis - r).rem_euclid(12) == 0 {
                1.0
            } else if d.is_tone(semis, false) {
                0.85
            } else {
                0.4
            };
            Note::new(semis, strength)
        })
        .collect();
    Chord::new(pos_bars, chord_notes, scale_notes)
}

/// How strongly a 16th step (within a 2-bar span) is felt: downbeat > half bar >
/// quarters > eighths > sixteenths. The book's strong/weak beat idea as a number.
fn metric_weight(step: u32) -> f32 {
    let w = match step % 16 {
        0 => 1.0,
        8 => 0.8,
        4 | 12 => 0.6,
        s if s % 2 == 0 => 0.35,
        _ => 0.15,
    };
    if step >= 16 { w * 0.9 } else { w }
}

fn pick<T: Copy>(rng: &mut StdRng, options: &[(T, f32)]) -> T {
    options.choose_weighted(rng, |o| o.1).map(|o| o.0).unwrap_or(options[0].0)
}

// ── Composer: the lead sheet ───────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Part {
    A,
    B,
}

impl Part {
    fn index(self) -> usize {
        self as usize
    }
}

/// Functional-harmony Markov chain over minor-key degrees.
fn next_degree(rng: &mut StdRng, prev: Degree) -> Degree {
    let options: &[(i32, f32)] = match prev.idx {
        0 => &[(3, 3.0), (5, 3.0), (6, 3.0), (2, 2.0), (4, 1.5), (1, 1.0)],
        1 => &[(4, 4.0), (6, 1.0)],
        2 => &[(5, 3.0), (3, 3.0), (6, 2.0)],
        3 => &[(4, 3.0), (6, 2.0), (0, 2.0), (1, 2.0)],
        4 => &[(0, 4.0), (5, 2.0)],
        5 => &[(6, 3.0), (3, 3.0), (1, 2.0), (2, 1.0)],
        _ => &[(2, 3.0), (0, 3.0), (5, 2.0)],
    };
    Degree::new(pick(rng, options))
}

/// One 8-bar section: a Markov walk from a start chord, then predominant → V.
fn compose_section(rng: &mut StdRng, part: Part) -> Vec<Degree> {
    let start = match part {
        Part::A => Degree::new(0),
        // The bridge leans towards the relative major for contrast.
        Part::B => Degree::new(pick(rng, &[(2, 2.0), (5, 2.0), (3, 1.0)])),
    };
    let mut bars = vec![start];
    while bars.len() < 6 {
        let prev = *bars.last().unwrap();
        bars.push(next_degree(rng, prev));
    }
    bars.push(Degree::new(pick(rng, &[(3, 3.0), (1, 2.0), (5, 2.0)])));
    bars.push(Degree::dominant());
    bars
}

/// Text the musicians publish for the TUI.
#[derive(Default)]
struct Board {
    motif_a: String,
    motif_b: String,
    contour: String,
    statement: String,
    melody: VecDeque<String>,
    kick: String,
    snare: String,
    fill: String,
    bass: String,
    arp: String,
    pads: Vec<(&'static str, String)>,
    events: VecDeque<String>,
}

/// The shared lead sheet: what the composer wrote, plus what the band reports back.
struct Song {
    seed: u64,
    /// Bumped on every reseed; musicians re-derive their RNG when it changes.
    epoch: u32,
    /// Absolute bar at which the current form started.
    start_bar: u32,
    chorus: u32,
    rng: StdRng,
    a_prog: Vec<Degree>,
    b_prog: Vec<Degree>,
    pending_seed: Option<u64>,
    board: Board,
}

impl Song {
    fn new(seed: u64, start_bar: u32, epoch: u32) -> Self {
        let mut rng = StdRng::seed_from_u64(seed);
        let a_prog = compose_section(&mut rng, Part::A);
        let b_prog = compose_section(&mut rng, Part::B);
        let mut song = Self {
            seed, epoch, start_bar, chorus: 0, rng, a_prog, b_prog,
            pending_seed: None, board: Board::default(),
        };
        song.event(format!("seed {seed}: new song"));
        song
    }

    fn event(&mut self, text: String) {
        self.board.events.push_front(text);
        self.board.events.truncate(7);
    }

    /// (section index in the form, part, bar within the section)
    fn section(&self, bar: u32) -> (usize, Part, u32) {
        let form_bar = bar.wrapping_sub(self.start_bar) % FORM_BARS;
        let idx = (form_bar / SECTION_BARS) as usize;
        (idx, FORM[idx], form_bar % SECTION_BARS)
    }

    fn section_progression(&self, idx: usize) -> Vec<Degree> {
        match FORM[idx] {
            Part::B => self.b_prog.clone(),
            // The last A resolves (V → i) instead of turning around.
            Part::A if idx == FORM.len() - 1 => {
                let mut p = self.a_prog.clone();
                p[6] = Degree::dominant();
                p[7] = Degree::new(0);
                p
            }
            Part::A => self.a_prog.clone(),
        }
    }

    fn degree_at(&self, bar: u32) -> Degree {
        let (idx, _, bar_in_section) = self.section(bar);
        self.section_progression(idx)[bar_in_section as usize]
    }

    /// The rusty_music conductor for the current form. Chords are placed at
    /// absolute bar positions (mod the form length) so a form can start on any bar.
    fn conductor(&self) -> Conductor {
        let mut chords: Vec<Chord> = (0..FORM_BARS)
            .map(|k| {
                let bar = self.start_bar + k;
                build_chord((bar % FORM_BARS) as f32, self.degree_at(bar))
            })
            .collect();
        chords.sort_by(|a, b| a.pos_bars.total_cmp(&b.pos_bars));
        Conductor { chords, chord_length_bars: FORM_BARS as f32 }
    }

    /// Between choruses the composer revises the sheet: re-harmonise one bar
    /// of A, and write a fresh bridge every other chorus.
    fn develop(&mut self) {
        self.chorus += 1;
        let k = self.rng.random_range(1..6usize);
        let old = self.a_prog[k];
        let new = next_degree(&mut self.rng, self.a_prog[k - 1]);
        if new != old && new != self.a_prog[k + 1] {
            self.a_prog[k] = new;
            self.event(format!("chorus {}: A bar {} {} → {}", self.chorus + 1, k + 1, old.numeral(), new.numeral()));
        }
        if self.chorus.is_multiple_of(2) {
            self.b_prog = compose_section(&mut self.rng, Part::B);
            self.event(format!("chorus {}: new bridge", self.chorus + 1));
        }
    }
}

#[derive(Resource, Clone)]
struct Lead(Arc<Mutex<Song>>);

impl Lead {
    fn get(&self) -> MutexGuard<'_, Song> {
        self.0.lock().unwrap()
    }
}

/// Runs between the clock and the band: applies reseeds and develops the sheet
/// at the top of each form, then hands the conductor the new chords.
fn compose(mut beats: MessageReader<Beat>, lead: Res<Lead>, mut conductor: ResMut<Conductor>) {
    for beat in beats.read() {
        if beat.beat != 0 || beat.sixteenth != 0 {
            continue;
        }
        let mut song = lead.get();
        if let Some(seed) = song.pending_seed.take() {
            let epoch = song.epoch + 1;
            *song = Song::new(seed, beat.bar_count, epoch);
        } else if beat.bar_count != song.start_bar && song.section(beat.bar_count) == (0, Part::A, 0) {
            song.develop();
        } else {
            continue;
        }
        *conductor = song.conductor();
    }
}

/// A musician's private randomness, derived from the song seed + a salt.
struct Mind {
    rng: StdRng,
    epoch: u32,
    chorus: u32,
    salt: u64,
}

impl Mind {
    fn new(salt: u64) -> Self {
        Self { rng: StdRng::seed_from_u64(salt), epoch: u32::MAX, chorus: 0, salt }
    }

    /// True when the song was reseeded: re-derive the RNG and forget everything.
    fn wake(&mut self, song: &Song) -> bool {
        if self.epoch == song.epoch {
            return false;
        }
        self.epoch = song.epoch;
        self.chorus = song.chorus;
        self.rng = StdRng::seed_from_u64(song.seed ^ self.salt.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        true
    }

    /// True once per new chorus.
    fn new_chorus(&mut self, song: &Song) -> bool {
        let changed = self.chorus != song.chorus;
        self.chorus = song.chorus;
        changed
    }
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

fn play_hit(commands: &mut Commands, handle: &Option<Handle<AudioSample>>, db: f32, semis: i32) {
    if let Some(h) = handle {
        commands.spawn((
            SamplePlayer::new(h.clone()).with_volume(Volume::Decibels(db)),
            PlaybackSettings::default().with_speed(midi_diff_to_pitch(semis)),
        ));
    }
}

fn pattern_string(p: &[f32; 16]) -> String {
    p.iter()
        .enumerate()
        .map(|(s, &v)| {
            let c = if v >= 0.8 { 'X' } else if v >= 0.4 { 'x' } else if v > 0.0 { '·' } else { '-' };
            if s % 4 == 3 && s != 15 { format!("{c} ") } else { c.to_string() }
        })
        .collect()
}

// ── Soloist: motif development ──────────────────────────────────────────────────

const MOTIF_STEPS: u32 = 32;
/// Scale-position range the melody may wander in (relative to the tonic).
const MELODY_RANGE: (i32, i32) = (-4, 9);

#[derive(Clone, Copy, Debug)]
struct MotifNote {
    step: u32,
    /// Scale steps from the previous note (the first note's is ignored).
    interval: i32,
    /// 0–1: how essential the note is; low-weight notes drop out at low intensity.
    weight: f32,
}

#[derive(Clone, Debug, Default)]
struct Motif(Vec<MotifNote>);

/// Melodic contour step, favouring stepwise motion and filling gaps after leaps.
fn contour(rng: &mut StdRng, prev: i32) -> i32 {
    if prev.abs() >= 3 {
        return -prev.signum();
    }
    pick(rng, &[(-1, 30.0), (1, 30.0), (-2, 10.0), (2, 10.0), (0, 6.0), (-3, 4.0), (3, 4.0), (4, 2.0), (-4, 2.0)])
}

impl Motif {
    fn generate(rng: &mut StdRng, density: f32) -> Self {
        let mut notes = Vec::new();
        let mut prev = 0;
        // Leave the last beat open so phrases breathe.
        for step in 0..MOTIF_STEPS - 4 {
            let w = metric_weight(step);
            if step != 0 && rng.random::<f32>() >= w * density {
                continue;
            }
            let interval = if notes.is_empty() { 0 } else { contour(rng, prev) };
            prev = interval;
            let weight = if step == 0 { 1.0 } else { (w * rng.random_range(0.7..1.4)).min(1.0) };
            notes.push(MotifNote { step, interval, weight });
        }
        Motif(notes)
    }

    /// Keep only notes matching `keep`, folding dropped intervals into the next
    /// kept note so the contour still lands where it would have.
    fn filtered(&self, keep: impl Fn(&MotifNote) -> bool) -> Self {
        let mut out = Vec::new();
        let mut carry = 0;
        for (i, n) in self.0.iter().enumerate() {
            carry += n.interval;
            if i == 0 || keep(n) {
                out.push(MotifNote { interval: if i == 0 { 0 } else { carry }, ..*n });
                carry = 0;
            }
        }
        Motif(out)
    }

    fn transform(&self, t: Transform, rng: &mut StdRng) -> Self {
        let mut m = self.clone();
        match t {
            Transform::Statement | Transform::Repeat | Transform::Sequence(_) => {}
            Transform::Inversion => m.0.iter_mut().for_each(|n| n.interval = -n.interval),
            Transform::Retrograde => {
                // Same rhythm, contour played backwards.
                let mut ints: Vec<i32> = m.0.iter().skip(1).map(|n| -n.interval).collect();
                ints.reverse();
                m.0.iter_mut().skip(1).zip(ints).for_each(|(n, i)| n.interval = i);
            }
            Transform::Fragment => {
                // First half, then again a step higher or lower.
                let head: Vec<MotifNote> = m.0.iter().filter(|n| n.step < 16).copied().collect();
                let travelled: i32 = head.iter().skip(1).map(|n| n.interval).sum();
                let shift = if rng.random_bool(0.5) { 1 } else { -1 };
                let mut tail: Vec<MotifNote> = head.iter().map(|n| MotifNote { step: n.step + 16, ..*n }).collect();
                if let Some(first) = tail.first_mut() {
                    first.interval = shift - travelled;
                }
                m.0 = head.into_iter().chain(tail).collect();
            }
            Transform::Ornament => {
                // Neighbour notes in the gaps: up-and-back embellishments.
                let mut out = Vec::new();
                for (i, n) in self.0.iter().enumerate() {
                    out.push(*n);
                    let Some(next) = self.0.get(i + 1) else { continue };
                    if next.step - n.step >= 3 && rng.random_bool(0.6) {
                        let dir = if rng.random_bool(0.5) { 1 } else { -1 };
                        out.push(MotifNote { step: n.step + 1, interval: dir, weight: 0.3 });
                        // Undo the neighbour step so the next note lands as before.
                        out.push(MotifNote { interval: next.interval - dir, ..*next });
                    }
                }
                // The loop pushed every "next" twice where it embellished; dedupe by step.
                out.dedup_by_key(|n| n.step);
                m.0 = out;
            }
            Transform::Thin => m = self.filtered(|n| n.weight >= 0.55),
            Transform::Cadence => {
                // Opening of the idea, then a long arrival on beat 3 of bar 2.
                let mut head = self.filtered(|n| n.step < 20);
                head.0.push(MotifNote { step: 24, interval: 0, weight: 1.0 });
                m = head;
            }
        }
        m
    }

    /// Small, memorable change between choruses: one interval or one onset.
    fn mutate(&mut self, rng: &mut StdRng) -> String {
        if self.0.len() > 2 && rng.random_bool(0.6) {
            let k = rng.random_range(1..self.0.len());
            let old = self.0[k].interval;
            self.0[k].interval = contour(rng, 0);
            format!("motif A: note {} {old:+} → {:+}", k + 1, self.0[k].interval)
        } else {
            let step = rng.random_range(1..MOTIF_STEPS - 4);
            if let Some(i) = self.0.iter().position(|n| n.step == step) {
                self.0.remove(i);
                format!("motif A: dropped note at {}", step + 1)
            } else {
                let at = self.0.iter().position(|n| n.step > step).unwrap_or(self.0.len());
                let interval = contour(rng, 0);
                self.0.insert(at, MotifNote { step, interval, weight: metric_weight(step) });
                format!("motif A: added note at {}", step + 1)
            }
        }
    }

    fn rhythm_string(&self) -> String {
        (0..MOTIF_STEPS)
            .map(|s| {
                let c = match self.0.iter().find(|n| n.step == s) {
                    Some(n) if n.weight >= 0.55 => '●',
                    Some(_) => '○',
                    None => '·',
                };
                if s == 15 { format!("{c}│") } else { c.to_string() }
            })
            .collect()
    }

    fn contour_string(&self) -> String {
        self.0.iter().skip(1).map(|n| format!("{:+}", n.interval)).collect::<Vec<_>>().join(" ")
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Transform {
    Statement,
    Repeat,
    Sequence(i32),
    Inversion,
    Retrograde,
    Fragment,
    Ornament,
    Thin,
    Cadence,
}

impl Transform {
    fn label(self) -> String {
        match self {
            Transform::Sequence(n) => format!("sequence {n:+}"),
            t => format!("{t:?}").to_lowercase(),
        }
    }
}

/// Lead melody: violins, doubled by flute at high intensity. Each 8-bar section
/// is four 2-bar statements of a motif: idea → repetition → development → cadence.
struct Soloist {
    lead: Lead,
    mind: Mind,
    violins: MultiSampler,
    flute: MultiSampler,
    root_midi: i32,
    motif_a: Motif,
    motif_b: Motif,
    current: Motif,
    transform: Transform,
    /// Absolute bar / 2 of the statement being played.
    statement_key: u32,
    anchor: i32,
    pos: i32,
}

impl Soloist {
    fn reset(&mut self, song: &mut Song) {
        let density = self.mind.rng.random_range(0.55..1.1);
        self.motif_a = Motif::generate(&mut self.mind.rng, density);
        // The bridge contrasts: busier if A is sparse and vice versa.
        let b_density = if self.motif_a.0.len() > 7 { 0.5 } else { 1.15 };
        self.motif_b = Motif::generate(&mut self.mind.rng, b_density);
        self.statement_key = u32::MAX;
        song.board.motif_a = self.motif_a.rhythm_string();
        song.board.motif_b = self.motif_b.rhythm_string();
    }

    fn plan(&mut self, statement: u32, part: Part, deg: Degree, intensity: f32, song: &mut Song) {
        let rng = &mut self.mind.rng;
        self.transform = match statement {
            0 => Transform::Statement,
            1 => pick(rng, &[
                (Transform::Repeat, 3.0), (Transform::Sequence(1), 2.0),
                (Transform::Sequence(-1), 2.0), (Transform::Sequence(2), 1.0),
            ]),
            2 => pick(rng, &[
                (Transform::Fragment, 3.0), (Transform::Inversion, 2.0), (Transform::Retrograde, 2.0),
                (Transform::Ornament, 0.5 + 3.0 * intensity), (Transform::Thin, 3.0 * (1.0 - intensity)),
            ]),
            _ => Transform::Cadence,
        };
        self.anchor = match self.transform {
            Transform::Statement => snap_to_chord(if part == Part::A { 0 } else { 2 }, deg),
            Transform::Sequence(n) => self.anchor + n,
            _ => self.anchor,
        };
        let motif = if part == Part::A { &self.motif_a } else { &self.motif_b };
        self.current = motif.transform(self.transform, rng);
        song.board.statement = format!("{:?} · statement {}/4 · {}", part, statement + 1, self.transform.label());
        song.board.contour = self.current.contour_string();
    }
}

impl MusicPlayer for Soloist {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, _chord: &Chord) {
        let lead = self.lead.clone();
        let mut song = lead.get();
        if self.mind.wake(&song) {
            self.reset(&mut song);
        }
        if self.mind.new_chorus(&song) {
            let change = self.motif_a.mutate(&mut self.mind.rng);
            song.event(change);
            song.board.motif_a = self.motif_a.rhythm_string();
            if song.chorus.is_multiple_of(2) {
                let density = self.mind.rng.random_range(0.5..1.2);
                self.motif_b = Motif::generate(&mut self.mind.rng, density);
                song.board.motif_b = self.motif_b.rhythm_string();
                song.event("motif B: new bridge idea".into());
            }
        }

        let i = base_intensity;
        let (_, part, bar_in_section) = song.section(beat.bar_count);
        let deg = song.degree_at(beat.bar_count);
        let statement = bar_in_section / 2;
        if beat.bar_count / 2 != self.statement_key {
            self.statement_key = beat.bar_count / 2;
            self.plan(statement, part, deg, i, &mut song);
        }

        let step = (bar_in_section % 2) * 16 + TonalPlayer::flat_step(&beat);
        let Some(idx) = self.current.0.iter().position(|n| n.step == step) else { return };
        let note = self.current.0[idx];

        // Walk the contour even for notes we skip, so the shape is preserved.
        self.pos = if idx == 0 { self.anchor } else { self.pos + note.interval };
        if self.pos > MELODY_RANGE.1 {
            self.pos -= 7;
        } else if self.pos < MELODY_RANGE.0 {
            self.pos += 7;
        }
        let last = idx == self.current.0.len() - 1;
        if last && self.transform == Transform::Cadence {
            self.pos = snap_to_root(self.pos, deg);
        } else if step.is_multiple_of(8) {
            // Strong beats land on chord tones: the motif bends to the harmony.
            self.pos = snap_to_chord(self.pos, deg);
        }

        if i < 0.2 || note.weight < 1.0 - i * 1.2 {
            return;
        }
        let midi = self.root_midi + pitch_of(self.pos, deg);
        let gap = self.current.0.get(idx + 1).map_or(8, |n| n.step - step);
        let role = if gap >= 6 { "sustain" } else { "short" };
        let dynamic = Dynamic::for_intensity(i);
        play_note(commands, &self.violins, midi, role, dynamic, -1.0 + i * 3.0 + note.weight * 2.0);
        if i > 0.65 {
            play_note(commands, &self.flute, midi, role, dynamic, -3.0 + i * 2.0);
        }
        song.board.melody.push_front(midi_name(midi));
        song.board.melody.truncate(12);
    }
}

// ── Bass ──────────────────────────────────────────────────────────────────────

/// Cellos (doubled an octave down by double basses at higher intensity). The
/// rhythm is remembered per section, the pitches are chosen live, and the last
/// eighth before a chord change walks chromatically into the next root.
struct Bass {
    lead: Lead,
    mind: Mind,
    cello: MultiSampler,
    contrabass: MultiSampler,
    rhythm: [[f32; 16]; 2],
}

fn bass_rhythm(rng: &mut StdRng) -> [f32; 16] {
    let mut r = [0.0; 16];
    for (s, v) in r.iter_mut().enumerate() {
        let w = metric_weight(s as u32);
        if rng.random::<f32>() < w {
            *v = (w * rng.random_range(0.5..1.2)).min(1.0);
        }
    }
    r[0] = 1.0;
    r
}

impl MusicPlayer for Bass {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, _chord: &Chord) {
        let lead = self.lead.clone();
        let mut song = lead.get();
        if self.mind.wake(&song) {
            self.rhythm = [bass_rhythm(&mut self.mind.rng), bass_rhythm(&mut self.mind.rng)];
        }
        if self.mind.new_chorus(&song) && song.chorus.is_multiple_of(2) {
            self.rhythm[Part::B.index()] = bass_rhythm(&mut self.mind.rng);
        }
        let i = base_intensity;
        let (_, part, _) = song.section(beat.bar_count);
        let rhythm = &self.rhythm[part.index()];
        song.board.bass = pattern_string(rhythm);
        if i < 0.13 {
            return;
        }

        let s = TonalPlayer::flat_step(&beat) as usize;
        let deg = song.degree_at(beat.bar_count);
        let next = song.degree_at(beat.bar_count + 1);
        let rng = &mut self.mind.rng;
        let [r, t, f, _] = deg.tones();

        let approach = s == 14 && next != deg && i > 0.35 && rng.random_bool(0.6);
        let semis = if approach {
            let target = next.root();
            if rng.random_bool(0.6) { target - 1 } else { target + 1 }
        } else {
            if rhythm[s] == 0.0 || rhythm[s] < 1.0 - i {
                return;
            }
            if s == 0 { r } else { pick(rng, &[(r, 3.0), (f, 3.0), (r + 12, 2.0), (t, 1.0)]) }
        };

        let gap = (s + 1..16).find(|&k| rhythm[k] > 0.0).map_or(16 - s, |k| k - s);
        let role = if gap >= 6 { "sustain" } else { "short" };
        let dynamic = Dynamic::for_intensity(i);
        play_note(commands, &self.cello, 50 + semis, role, dynamic, -1.0 + i * 3.0);
        if i > 0.45 {
            play_note(commands, &self.contrabass, 38 + semis, role, dynamic, -3.0 + i * 3.0);
        }
    }
}

// ── Pads: voice-led harmony ─────────────────────────────────────────────────────

/// Move each voice of `prev` to the nearest unused chord tone of `deg` —
/// classic voice leading, so harmony glides instead of jumping.
fn voice_lead(prev: &[i32], deg: Degree, n: usize, center: i32, sevenths: bool) -> Vec<i32> {
    let tones = deg.tones();
    let pcs: Vec<i32> = tones[..if sevenths { 4 } else { 3 }].iter().map(|t| (TONIC + t).rem_euclid(12)).collect();
    let nearest = |pc: i32, to: i32| {
        let up = to + (pc - to).rem_euclid(12);
        if up - to > 6 { up - 12 } else { up }
    };
    let seeds: Vec<i32> = if prev.len() == n {
        prev.to_vec()
    } else {
        (0..n as i32).map(|k| center + k * 4).collect()
    };
    // Voices stay in a comfortable band around the section's centre.
    let in_band = |q: i32| (center - 8..=center + 12).contains(&q);
    let mut used: Vec<i32> = Vec::new();
    seeds
        .iter()
        .map(|&p| {
            let best = pcs
                .iter()
                .copied()
                .filter(|pc| !used.contains(pc))
                .flat_map(|pc| {
                    let q = nearest(pc, p);
                    [q - 12, q, q + 12]
                })
                .filter(|&q| in_band(q))
                .min_by_key(|q| (q - p).abs())
                .unwrap_or_else(|| nearest(pcs[0], p));
            used.push(best.rem_euclid(12));
            best
        })
        .collect()
}

/// A sustained or stabbing chord section (clarinets, horns, trombones, oboe).
struct Pad {
    lead: Lead,
    mind: Mind,
    label: &'static str,
    sampler: MultiSampler,
    center: i32,
    enter: f32,
    max_voices: usize,
    /// Brass plays generated rhythmic stabs instead of held chords.
    stabs: bool,
    rhythm: [[f32; 16]; 2],
    voicing: Vec<i32>,
    voiced_bar: u32,
}

fn stab_rhythm(rng: &mut StdRng) -> [f32; 16] {
    let mut r = [0.0; 16];
    let hits = rng.random_range(2..=5);
    for s in 0..16 {
        // Euclidean spread, rotated by a random 8th so stabs syncopate.
        if (s * hits) % 16 < hits {
            r[(s + 2 * rng.random_range(0..2)) % 16] = metric_weight(s as u32).max(0.3);
        }
    }
    r[0] = 1.0;
    r
}

impl MusicPlayer for Pad {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, _chord: &Chord) {
        let lead = self.lead.clone();
        let mut song = lead.get();
        if self.mind.wake(&song) {
            self.rhythm = [stab_rhythm(&mut self.mind.rng), stab_rhythm(&mut self.mind.rng)];
            self.voicing.clear();
        }
        let i = base_intensity;
        if i < self.enter || self.sampler.is_empty() {
            return;
        }
        let deg = song.degree_at(beat.bar_count);
        let span = ((i - self.enter) / (1.0 - self.enter)).clamp(0.0, 1.0);
        let voices = 1 + (span * (self.max_voices - 1) as f32).round() as usize;
        let dynamic = Dynamic::for_intensity(i);
        let s = TonalPlayer::flat_step(&beat) as usize;

        let new_bar = beat.bar_count != self.voiced_bar || self.voicing.len() != voices;
        if new_bar {
            self.voiced_bar = beat.bar_count;
            self.voicing = voice_lead(&self.voicing, deg, voices, self.center, i > 0.6);
            let names: Vec<String> = self.voicing.iter().map(|&m| midi_name(m)).collect();
            let label = self.label;
            match song.board.pads.iter_mut().find(|(l, _)| *l == label) {
                Some(entry) => entry.1 = names.join(" "),
                None => song.board.pads.push((label, names.join(" "))),
            }
        }

        let (_, part, _) = song.section(beat.bar_count);
        let play = if self.stabs {
            let v = self.rhythm[part.index()][s];
            v > 0.0 && v >= 1.0 - span
        } else {
            new_bar
        };
        if !play {
            return;
        }
        let role = if self.stabs { "stab" } else { "sustain" };
        for (k, &m) in self.voicing.iter().enumerate() {
            play_note(commands, &self.sampler, m, role, dynamic, -1.0 - k as f32 * 1.5 + i * 2.0);
        }
    }
}

// ── Arpeggio (the book's "spacey chimes") ───────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
enum ArpMode {
    Up,
    Down,
    UpDown,
    Random,
    /// A remembered index cell, e.g. [0, 2, 1, 3] — a tiny motif over chord tones.
    Cell,
}

struct Arpeggio {
    lead: Lead,
    mind: Mind,
    sampler: MultiSampler,
    low: i32,
    modes: [ArpMode; 2],
    cells: [Vec<usize>; 2],
    idx: usize,
}

impl Arpeggio {
    fn invent(rng: &mut StdRng) -> (ArpMode, Vec<usize>) {
        let mode = pick(rng, &[
            (ArpMode::Up, 2.0), (ArpMode::Down, 1.0), (ArpMode::UpDown, 2.0),
            (ArpMode::Random, 1.0), (ArpMode::Cell, 3.0),
        ]);
        let cell = (0..rng.random_range(3..=6)).map(|_| rng.random_range(0..5)).collect();
        (mode, cell)
    }
}

impl MusicPlayer for Arpeggio {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, _chord: &Chord) {
        let lead = self.lead.clone();
        let mut song = lead.get();
        if self.mind.wake(&song) {
            for p in 0..2 {
                (self.modes[p], self.cells[p]) = Self::invent(&mut self.mind.rng);
            }
        }
        if self.mind.new_chorus(&song) {
            let p = self.mind.rng.random_range(0..2);
            (self.modes[p], self.cells[p]) = Self::invent(&mut self.mind.rng);
        }
        let (_, part, _) = song.section(beat.bar_count);
        let (mode, cell) = (self.modes[part.index()], &self.cells[part.index()]);
        song.board.arp = if mode == ArpMode::Cell { format!("Cell {cell:?}") } else { format!("{mode:?}") };

        let i = base_intensity;
        if i < 0.3 || self.sampler.is_empty() {
            return;
        }
        let rate = if i < 0.5 { 4 } else if i < 0.8 { 2 } else { 1 };
        if !TonalPlayer::flat_step(&beat).is_multiple_of(rate) {
            return;
        }
        let deg = song.degree_at(beat.bar_count);
        let pitches: Vec<i32> = (self.low..self.low + 16).filter(|m| deg.is_tone(m - TONIC, i > 0.6)).collect();
        let len = pitches.len();
        if len == 0 {
            return;
        }
        let k = self.idx;
        self.idx = self.idx.wrapping_add(1);
        let at = match mode {
            ArpMode::Up => k % len,
            ArpMode::Down => len - 1 - k % len,
            ArpMode::UpDown => {
                let period = (2 * len - 2).max(1);
                let p = k % period;
                if p < len { p } else { period - p }
            }
            ArpMode::Random => self.mind.rng.random_range(0..len),
            ArpMode::Cell => cell[k % cell.len()] % len,
        };
        let role = if i < 0.5 { "pizz" } else { "short" };
        play_note(commands, &self.sampler, pitches[at], role, Dynamic::for_intensity(i), -4.0 + i * 2.0);
    }
}

// ── Drums: generated grooves and fills ────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Hit {
    Snare,
    Tom(i32),
}

struct Kit {
    kick: Option<Handle<AudioSample>>,
    snare: Option<Handle<AudioSample>>,
    tom: Option<Handle<AudioSample>>,
    clash: Option<Handle<AudioSample>>,
    susp_roll: Option<Handle<AudioSample>>,
}

struct Drums {
    lead: Lead,
    mind: Mind,
    kit: Kit,
    kick: [[f32; 16]; 2],
    snare: [[f32; 16]; 2],
    fill: Vec<(usize, Hit)>,
    fill_bar: u32,
}

/// Euclidean kick (k hits spread over 16) + backbeat snare with ghost notes.
fn groove(rng: &mut StdRng, part: Part) -> ([f32; 16], [f32; 16]) {
    let k = rng.random_range(2..=5);
    let mut kick = [0.0; 16];
    for (s, v) in kick.iter_mut().enumerate() {
        if (s * k) % 16 < k {
            *v = (metric_weight(s as u32) + rng.random_range(0.0..0.3)).min(1.0);
        }
    }
    kick[0] = 1.0;
    let mut snare = [0.0; 16];
    if part == Part::B && rng.random_bool(0.5) {
        snare[8] = 1.0; // half-time bridge
    } else {
        snare[4] = 1.0;
        snare[12] = 1.0;
    }
    for s in 0..16 {
        if snare[s] == 0.0 && kick[s] == 0.0 && rng.random_bool(0.3) {
            snare[s] = rng.random_range(0.05..0.3);
        }
    }
    (kick, snare)
}

/// A fill for the last bar of a section: starts earlier and denser as intensity rises.
fn compose_fill(rng: &mut StdRng, intensity: f32) -> (Vec<(usize, Hit)>, String) {
    let start = if intensity > 0.75 { 4 } else if intensity > 0.5 { 8 } else { 12 };
    let toms = rng.random_bool(0.5);
    let shape = pick(rng, &[("ramp", 2.0), ("threes", 1.0), ("straight", 1.0)]);
    let mut hits = Vec::new();
    for s in start..16 {
        let t = (s - start) as f32 / (16 - start) as f32;
        let p = match shape {
            "ramp" => 0.3 + 0.7 * t,
            "threes" => if (s - start) % 3 == 0 { 1.0 } else { 0.15 },
            _ => if s % 2 == 0 { 0.9 } else { 0.4 * intensity },
        };
        if rng.random::<f32>() < p {
            // Toms tumble downwards in pitch across the fill.
            hits.push((s, if toms { Hit::Tom(5 - (t * 10.0) as i32) } else { Hit::Snare }));
        }
    }
    let text = (0..16)
        .map(|s| match hits.iter().find(|(h, _)| *h == s) {
            Some((_, Hit::Snare)) => 's',
            Some((_, Hit::Tom(_))) => 't',
            None => '-',
        })
        .collect::<String>();
    (hits, format!("{shape:<8} {text}"))
}

impl MusicPlayer for Drums {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, _chord: &Chord) {
        let lead = self.lead.clone();
        let mut song = lead.get();
        if self.mind.wake(&song) {
            for part in [Part::A, Part::B] {
                (self.kick[part.index()], self.snare[part.index()]) = groove(&mut self.mind.rng, part);
            }
        }
        if self.mind.new_chorus(&song) && song.chorus.is_multiple_of(2) {
            (self.kick[1], self.snare[1]) = groove(&mut self.mind.rng, Part::B);
        }
        let i = base_intensity;
        let (_, part, bar_in_section) = song.section(beat.bar_count);
        let p = part.index();
        song.board.kick = pattern_string(&self.kick[p]);
        song.board.snare = pattern_string(&self.snare[p]);
        if i < 0.08 {
            return;
        }
        let s = TonalPlayer::flat_step(&beat) as usize;
        let gate = 1.0 - i;

        if self.kick[p][s] > 0.0 && self.kick[p][s] >= gate {
            play_hit(commands, &self.kit.kick, -1.0 + self.kick[p][s] * 2.0, 0);
        }

        let fill_bar = bar_in_section == SECTION_BARS - 1 && i > 0.3;
        if fill_bar && self.fill_bar != beat.bar_count {
            self.fill_bar = beat.bar_count;
            let (hits, text) = compose_fill(&mut self.mind.rng, i);
            self.fill = hits;
            song.board.fill = text;
        }
        let fill_start = self.fill.first().map_or(16, |(s, _)| *s);
        if fill_bar && s >= fill_start {
            if let Some(&(_, hit)) = self.fill.iter().find(|(h, _)| *h == s) {
                let ramp = (s - fill_start) as f32 / 8.0;
                match hit {
                    Hit::Snare => play_hit(commands, &self.kit.snare, -3.0 + ramp * 3.0, 0),
                    Hit::Tom(semis) => play_hit(commands, &self.kit.tom, -1.0 + ramp * 2.0, semis),
                }
            }
        } else if self.snare[p][s] > 0.0 && self.snare[p][s] >= gate {
            let v = self.snare[p][s];
            play_hit(commands, &self.kit.snare, if v >= 0.8 { 0.0 } else { -12.0 + v * 15.0 }, 0);
        }

        // Cymbals mark the form: clash into each section, roll into the next.
        if bar_in_section == 0 && s == 0 && i > 0.5 {
            play_hit(commands, &self.kit.clash, -10.0 + (i - 0.5) * 18.0, 0);
        }
        if bar_in_section == SECTION_BARS - 1 && s == 8 && i > 0.7 {
            play_hit(commands, &self.kit.susp_roll, -9.0 + i * 5.0, 0);
        }
    }
}

// ── Resources / UI state ─────────────────────────────────────────────────────────

#[derive(Resource)]
struct TuiTerminal(Terminal<CrosstermBackend<Stdout>>);

#[derive(Resource, Default)]
struct UiState {
    auto_mode: bool,
    last_draw: Option<Instant>,
}

fn auto_build(clock: Res<Clock>, mut intensity: ResMut<Intensity>, ui: Res<UiState>) {
    if !ui.auto_mode {
        return;
    }
    let cycle = (clock.bar_count % 64) as f32;
    intensity.0 = if cycle < 48.0 {
        (cycle / 48.0).clamp(0.0, 1.0)
    } else {
        (1.0 - (cycle - 48.0) / 16.0).clamp(0.0, 1.0)
    };
}

fn handle_input(mut intensity: ResMut<Intensity>, mut ui: ResMut<UiState>, lead: Res<Lead>) {
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
                ui.auto_mode = false;
                intensity.0 = (intensity.0 + 0.05).min(1.0);
            }
            KeyCode::Down => {
                ui.auto_mode = false;
                intensity.0 = (intensity.0 - 0.05).max(0.0);
            }
            KeyCode::Char(' ') => ui.auto_mode = !ui.auto_mode,
            KeyCode::Char('n') => lead.get().pending_seed = Some(rand::random::<u64>() % 100_000),
            KeyCode::Char('r') => {
                let mut song = lead.get();
                song.pending_seed = Some(song.seed);
            }
            _ => {}
        }
    }
}

// ── TUI rendering ───────────────────────────────────────────────────────────────

const LAYERS: &[(&str, f32)] = &[
    ("Drums", 0.08), ("Cellos / basses", 0.13), ("Melody (violins)", 0.2), ("Clarinets", 0.3),
    ("Violas arpeggio", 0.3), ("Trombone stabs", 0.45), ("Horns", 0.55), ("Flute double", 0.65), ("Oboe", 0.72),
];

fn draw_tui(
    mut terminal: ResMut<TuiTerminal>,
    mut ui: ResMut<UiState>,
    intensity: Res<Intensity>,
    clock: Res<Clock>,
    lead: Res<Lead>,
) {
    let now = Instant::now();
    if ui.last_draw.is_some_and(|t| now.duration_since(t).as_millis() < 33) {
        return;
    }
    ui.last_draw = Some(now);
    let song = lead.get();
    let i = intensity.0;
    let auto = ui.auto_mode;
    let _ = terminal.0.draw(|frame| render(frame, &song, &clock, i, auto));
}

fn label(text: &str) -> Span<'static> {
    Span::styled(format!(" {text:<9}"), Style::default().fg(Color::DarkGray))
}

fn render(frame: &mut ratatui::Frame, song: &Song, clock: &Clock, i: f32, auto: bool) {
    let rows = Layout::vertical([Constraint::Length(3), Constraint::Length(5), Constraint::Min(12), Constraint::Length(3)])
        .split(frame.area());
    let bold = |c: Color| Style::default().fg(c).add_modifier(Modifier::BOLD);

    // Header
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" IMPROV ORCHESTRA ", bold(Color::Yellow)),
            Span::raw("│  Bar "),
            Span::styled(format!("{:04}", clock.bar_count + 1), bold(Color::Green)),
            Span::raw("  Beat "),
            Span::styled((clock.beat + 1).to_string(), bold(Color::Green)),
            Span::raw(format!("  │  BPM {BPM}  ─  D minor  │  seed ")),
            Span::styled(song.seed.to_string(), bold(Color::Magenta)),
            Span::raw("  chorus "),
            Span::styled((song.chorus + 1).to_string(), bold(Color::Cyan)),
        ]))
        .block(Block::new().borders(Borders::ALL)),
        rows[0],
    );

    // Lead sheet: the form and the current section's progression.
    let (sec_idx, _, bar_in_section) = song.section(clock.bar_count);
    let mut form = vec![label("Form")];
    for (k, part) in FORM.iter().enumerate() {
        let style = if k == sec_idx { bold(Color::Black).bg(Color::Yellow) } else { Style::default().fg(Color::DarkGray) };
        form.push(Span::styled(format!(" {part:?} "), style));
        form.push(Span::raw(" "));
    }
    let mut chords = vec![label("Chords")];
    for (k, d) in song.section_progression(sec_idx).iter().enumerate() {
        let style = if k as u32 == bar_in_section { bold(Color::Black).bg(Color::Cyan) } else { Style::default().fg(Color::Cyan) };
        chords.push(Span::styled(format!(" {:<4}{:<4}", d.name(), d.numeral()), style));
    }
    frame.render_widget(
        Paragraph::new(vec![Line::from(form), Line::from(chords)])
            .block(Block::new().borders(Borders::ALL).title(" Lead sheet (composed from the seed) ")),
        rows[1],
    );

    // Body
    let cols = Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).split(rows[2]);
    let left = Layout::vertical([Constraint::Length(8), Constraint::Min(5)]).split(cols[0]);
    let b = &song.board;
    let melody: Vec<String> = b.melody.iter().rev().cloned().collect();
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![label("Motif A"), Span::styled(b.motif_a.clone(), Style::default().fg(Color::Green))]),
            Line::from(vec![label("Motif B"), Span::styled(b.motif_b.clone(), Style::default().fg(Color::Yellow))]),
            Line::from(vec![label("Now"), Span::styled(b.statement.clone(), bold(Color::White))]),
            Line::from(vec![label("Contour"), Span::raw(b.contour.clone())]),
            Line::from(vec![label("Played"), Span::styled(melody.join(" "), Style::default().fg(Color::Cyan))]),
        ])
        .block(Block::new().borders(Borders::ALL).title(" Soloist: motif development ")),
        left[0],
    );
    let events: Vec<Line> = b.events.iter().map(|e| Line::from(format!(" {e}"))).collect();
    frame.render_widget(
        Paragraph::new(events).block(Block::new().borders(Borders::ALL).title(" Development log ")),
        left[1],
    );

    let right = Layout::vertical([Constraint::Length(3), Constraint::Length(8), Constraint::Min(5)]).split(cols[1]);
    frame.render_widget(
        Gauge::default()
            .block(Block::new().borders(Borders::ALL).title(if auto { " Intensity [AUTO] " } else { " Intensity [↑][↓] " }))
            .gauge_style(Style::default().fg(Color::Magenta).bg(Color::DarkGray))
            .ratio(i as f64)
            .label(format!("{i:.2}")),
        right[0],
    );
    let mut rhythm = vec![
        Line::from(vec![label("Kick"), Span::raw(b.kick.clone())]),
        Line::from(vec![label("Snare"), Span::raw(b.snare.clone())]),
        Line::from(vec![label("Last fill"), Span::raw(b.fill.clone())]),
        Line::from(vec![label("Bass"), Span::raw(b.bass.clone())]),
        Line::from(vec![label("Arp"), Span::raw(b.arp.clone())]),
    ];
    for (name, voicing) in &b.pads {
        rhythm.push(Line::from(vec![label(name), Span::raw(voicing.clone())]));
    }
    frame.render_widget(
        Paragraph::new(rhythm).block(Block::new().borders(Borders::ALL).title(" Band ")),
        right[1],
    );
    let layers: Vec<Line> = LAYERS
        .iter()
        .map(|&(name, enter)| {
            let on = i >= enter;
            Line::from(vec![
                Span::styled(if on { " ● " } else { " ○ " }, Style::default().fg(if on { Color::Green } else { Color::DarkGray })),
                Span::styled(format!("{name:<18}"), Style::default().fg(if on { Color::White } else { Color::DarkGray })),
                Span::styled(format!(">={:.0}%", enter * 100.0), Style::default().fg(Color::DarkGray)),
            ])
        })
        .collect();
    frame.render_widget(
        Paragraph::new(layers).block(Block::new().borders(Borders::ALL).title(" Layers ")),
        right[2],
    );

    // Footer
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" [↑][↓]", Style::default().fg(Color::Yellow)),
            Span::raw(" intensity  │  "),
            Span::styled("[Space]", Style::default().fg(Color::Magenta)),
            Span::raw(" auto-build  │  "),
            Span::styled("[n]", Style::default().fg(Color::Green)),
            Span::raw(" new seed  │  "),
            Span::styled("[r]", Style::default().fg(Color::Green)),
            Span::raw(" restart seed  │  "),
            Span::styled("[q]", bold(Color::Red)),
            Span::raw(" quit"),
        ]))
        .block(Block::new().borders(Borders::ALL)),
        rows[3],
    );
}

// ── Startup ─────────────────────────────────────────────────────────────────────

const DYNAMICS: &[Dynamic] = &[Dynamic::Piano, Dynamic::MezzoForte, Dynamic::Fortissimo];
const SHORT: &[&str] = &["025", "05"];
const LONG: &[&str] = &["1", "15", "long"];

fn voice<'a>(role: &'a str, articulations: &'a [&'a str], durations: &'a [&'a str], midi_range: (i32, i32)) -> VoiceFilter<'a> {
    VoiceFilter { role, articulations, durations, midi_range }
}

fn setup(mut commands: Commands, asset_server: Res<AssetServer>, lead: Res<Lead>) {
    let library = SampleLibrary::scan(&PathBuf::from("assets"), "all-samples");
    let load = |instrument: &str, db: f32, voices: &[VoiceFilter]| {
        let mut s = MultiSampler::new(instrument, db);
        for v in voices {
            s.add_voice(&library, &asset_server, v, DYNAMICS);
        }
        s
    };

    // Melody: violins + flute.
    let violins = load("violin", -3.0, &[
        voice("short", &["arco-normal"], SHORT, (60, 92)),
        voice("sustain", &["arco-legato", "arco-normal"], LONG, (60, 92)),
    ]);
    let flute = load("flute", -7.0, &[
        voice("short", &["normal"], SHORT, (60, 96)),
        voice("sustain", &["normal"], LONG, (60, 96)),
    ]);
    commands.spawn(Musician::new("Soloist".into(), Soloist {
        lead: lead.clone(), mind: Mind::new(1), violins, flute, root_midi: TONIC + 12,
        motif_a: Motif::default(), motif_b: Motif::default(), current: Motif::default(),
        transform: Transform::Statement, statement_key: u32::MAX, anchor: 0, pos: 0,
    }));

    // Bass: cellos + double basses.
    let cello = load("cello", -2.0, &[
        voice("short", &["arco-normal"], SHORT, (36, 68)),
        voice("sustain", &["arco-normal", "arco-legato"], LONG, (36, 68)),
    ]);
    let contrabass = load("double bass", -3.0, &[
        voice("short", &["arco-normal"], SHORT, (24, 56)),
        voice("sustain", &["arco-normal"], LONG, (24, 56)),
    ]);
    commands.spawn(Musician::new("Bass".into(), Bass {
        lead: lead.clone(), mind: Mind::new(2), cello, contrabass, rhythm: [[0.0; 16]; 2],
    }));

    // Arpeggio: violas.
    let violas = load("viola", -5.0, &[
        voice("pizz", &["pizz-normal"], &[], (48, 80)),
        voice("short", &["arco-normal"], SHORT, (48, 80)),
    ]);
    commands.spawn(Musician::new("Arpeggio".into(), Arpeggio {
        lead: lead.clone(), mind: Mind::new(3), sampler: violas, low: 57,
        modes: [ArpMode::Up; 2], cells: [vec![0], vec![0]], idx: 0,
    }));

    // Pads: clarinets, horns, trombone stabs, oboe on top.
    struct PadSpec<'a> {
        label: &'static str,
        instrument: &'a str,
        db: f32,
        voice: VoiceFilter<'a>,
        center: i32,
        enter: f32,
        max_voices: usize,
        stabs: bool,
    }
    let pads = [
        PadSpec { label: "Clarinets", instrument: "clarinet", db: -7.0, voice: voice("sustain", &["normal"], LONG, (50, 90)), center: 64, enter: 0.3, max_voices: 2, stabs: false },
        PadSpec { label: "Horns", instrument: "french horn", db: -6.0, voice: voice("sustain", &["normal"], LONG, (40, 80)), center: 57, enter: 0.55, max_voices: 3, stabs: false },
        PadSpec { label: "Trombones", instrument: "trombone", db: -4.0, voice: voice("stab", &["normal"], &["025", "05", "1"], (40, 76)), center: 52, enter: 0.45, max_voices: 3, stabs: true },
        PadSpec { label: "Oboe", instrument: "oboe", db: -9.0, voice: voice("sustain", &["normal"], LONG, (60, 92)), center: 76, enter: 0.72, max_voices: 1, stabs: false },
    ];
    for (k, p) in pads.into_iter().enumerate() {
        commands.spawn(Musician::new(p.label.into(), Pad {
            lead: lead.clone(), mind: Mind::new(10 + k as u64), label: p.label,
            sampler: load(p.instrument, p.db, &[p.voice]), center: p.center, enter: p.enter,
            max_voices: p.max_voices, stabs: p.stabs,
            rhythm: [[0.0; 16]; 2], voicing: Vec::new(), voiced_bar: u32::MAX,
        }));
    }

    // Drums.
    let kit = Kit {
        kick: library.resolve(&asset_server, "bass drum", &["struck-singly", "bass-drum-mallet", "rhythm"], Dynamic::Forte),
        snare: library.resolve(&asset_server, "snare drum", &["with-snares", "rhythm"], Dynamic::Forte),
        tom: library.resolve(&asset_server, "tom-toms", &["struck-singly"], Dynamic::MezzoForte),
        clash: library.resolve(&asset_server, "clash cymbals", &["struck-together", "undamped"], Dynamic::Fortissimo),
        susp_roll: library.resolve(&asset_server, "suspended cymbal", &["roll", "undamped"], Dynamic::Forte),
    };
    commands.spawn(Musician::new("Drums".into(), Drums {
        lead: lead.clone(), mind: Mind::new(4), kit,
        kick: [[0.0; 16]; 2], snare: [[0.0; 16]; 2], fill: Vec::new(), fill_bar: u32::MAX,
    }));

    commands.insert_resource(lead.get().conductor());
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
    let seed = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| rand::random::<u64>() % 100_000);

    enable_raw_mode().expect("enable raw mode");
    execute!(io::stdout(), EnterAlternateScreen).expect("enter alternate screen");
    let _guard = TerminalGuard;

    let terminal = Terminal::new(CrosstermBackend::new(io::stdout())).expect("create ratatui terminal");

    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(1))),
            AssetPlugin::default(),
        ))
        .add_plugins(MusicPlugin { beats: 4, note_type: 4, bpm: BPM })
        .insert_resource(TuiTerminal(terminal))
        .insert_resource(UiState::default())
        .insert_resource(Intensity(0.35))
        .insert_resource(Lead(Arc::new(Mutex::new(Song::new(seed, 0, 0)))))
        .add_systems(Startup, setup)
        .add_systems(Update, compose.after(progress_clock_system).before(play_sound_on_the_beat))
        .add_systems(Update, auto_build)
        .add_systems(Update, handle_input.after(auto_build))
        .add_systems(Update, draw_tui.after(handle_input))
        .run();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(d: Degree) -> Vec<String> {
        d.tones()[..3].iter().map(|t| NOTE_NAMES[(TONIC + t).rem_euclid(12) as usize].to_string()).collect()
    }

    #[test]
    fn diatonic_chords_in_d_minor() {
        assert_eq!(names(Degree::new(0)), ["D", "F", "A"]);
        assert_eq!(names(Degree::new(5)), ["Bb", "D", "F"]);
        assert_eq!(names(Degree::dominant()), ["A", "C#", "E"]);
        assert_eq!(Degree::new(1).name(), "E°");
        assert_eq!(Degree::new(6).name(), "C");
    }

    #[test]
    fn sections_cadence_on_the_dominant() {
        let mut rng = StdRng::seed_from_u64(7);
        for _ in 0..50 {
            for part in [Part::A, Part::B] {
                let p = compose_section(&mut rng, part);
                assert_eq!(p.len(), SECTION_BARS as usize);
                assert_eq!(p[7], Degree::dominant());
            }
        }
    }

    #[test]
    fn same_seed_same_song() {
        let a = Song::new(1234, 0, 0);
        let b = Song::new(1234, 0, 0);
        assert_eq!(a.a_prog, b.a_prog);
        assert_eq!(a.b_prog, b.b_prog);
    }

    #[test]
    fn conductor_follows_the_form_from_any_start_bar() {
        let song = Song::new(99, 13, 0);
        let conductor = song.conductor();
        for bar in 13..13 + FORM_BARS * 2 {
            let expected = build_chord(0.0, song.degree_at(bar)).chord_notes;
            assert_eq!(conductor.current_chord(bar as f32 + 0.5).chord_notes, expected, "bar {bar}");
        }
    }

    #[test]
    fn transforms_keep_steps_sorted_unique_and_in_range() {
        let mut rng = StdRng::seed_from_u64(3);
        let all = [
            Transform::Statement, Transform::Repeat, Transform::Sequence(1), Transform::Inversion,
            Transform::Retrograde, Transform::Fragment, Transform::Ornament, Transform::Thin, Transform::Cadence,
        ];
        for _ in 0..200 {
            let m = Motif::generate(&mut rng, 1.0);
            for t in all {
                let out = m.transform(t, &mut rng);
                assert!(!out.0.is_empty());
                assert!(out.0.windows(2).all(|w| w[0].step < w[1].step), "{t:?}: {:?}", out.0);
                assert!(out.0.iter().all(|n| n.step < MOTIF_STEPS));
            }
        }
    }

    #[test]
    fn ornament_and_thin_keep_original_notes_at_their_pitch() {
        let mut rng = StdRng::seed_from_u64(5);
        let positions = |m: &Motif| {
            let mut pos = 0;
            m.0.iter()
                .enumerate()
                .map(|(i, n)| {
                    pos += if i == 0 { 0 } else { n.interval };
                    (n.step, pos)
                })
                .collect::<Vec<_>>()
        };
        for _ in 0..200 {
            let m = Motif::generate(&mut rng, 1.0);
            let original = positions(&m);
            for t in [Transform::Ornament, Transform::Thin] {
                for (step, pos) in positions(&m.transform(t, &mut rng)) {
                    if let Some(&(_, expected)) = original.iter().find(|(s, _)| *s == step) {
                        assert_eq!(pos, expected, "{t:?} moved the note at step {step}");
                    }
                }
            }
        }
    }

    #[test]
    fn voice_leading_moves_little() {
        let first = voice_lead(&[], Degree::new(0), 3, 60, false);
        let next = voice_lead(&first, Degree::new(5), 3, 60, false);
        let moved: i32 = first.iter().zip(&next).map(|(a, b)| (a - b).abs()).sum();
        assert!(moved <= 4, "{first:?} → {next:?}");
        assert!(next.iter().all(|m| Degree::new(5).is_tone(m - TONIC, false)));
    }
}
