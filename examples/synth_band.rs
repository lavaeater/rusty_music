//! synth_band — a whole band of synths, driven by one intensity knob.
//!
//! Musicians only decide notes; instruments decide sound. Here:
//! - an **acid bassline** (a small generative player in this file) on the
//!   legato `acid_bass` patch: overlapping notes *slide* instead of re-triggering,
//! - the stock `Arpeggiator` on a `pluck`,
//! - the stock `Soloist` on the monophonic `glide_lead`,
//! - a pad player on `pad`, and FM `bell` sparkles that only appear when things heat up,
//! - drums from a **synth kit** (`kick`, `snare`, `hat`), or the sampled 808 kit
//!   with `-- --samples`.
//!
//! Everything plays through the mixer (`MixPlugin`): a bus per section, with
//! shared reverb and a tempo-synced ping-pong echo on the lead.
//!
//! Intensity sweeps up and down over 32 bars and is mapped onto the synths'
//! filters: the "situational knob" a game would drive from danger, speed, tension…
//!
//! `cargo run --example synth_band --features synth [-- --samples]` — Ctrl-C to stop.

use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use rusty_music::clock::{Beat, Clock};
use rusty_music::instrument::NoteEvent;
use rusty_music::mix::{MixPlugin, Section};
use rusty_music::musicians::arpeggiator::Arpeggiator;
use rusty_music::musicians::conductor::Conductor;
use rusty_music::musicians::drummer::{generate_hihat_beat, generate_kick_beat, generate_snare_beat, Drummer, SuperDrummer};
use rusty_music::musicians::soloist::Soloist;
use rusty_music::musicians::{Chord, MusicPlayer, Musician, Note};
use rusty_music::player::Intensity;
use rusty_music::synth::{spawn_synth_kit, synth, synth_musician, SynthNode, SynthPatch, SynthPlugin};
use rusty_music::{create_drummer_only, MusicPlugin};

/// A minor: Am – F – C – G, one chord per bar. Offsets are from C4.
fn chords() -> Vec<Chord> {
    let scale: Vec<Note> = [(-3, 1.0), (-1, 0.4), (0, 0.8), (2, 0.5), (4, 0.8), (5, 0.4), (7, 0.6), (9, 0.9), (11, 0.3), (12, 0.6)]
        .into_iter()
        .map(|(d, s)| Note::new(d, s))
        .collect();
    let chord = |pos: f32, tones: [i32; 5]| {
        let strengths = [1.0, 0.9, 0.8, 0.6, 0.4];
        Chord::new(pos, tones.iter().zip(strengths).map(|(&d, s)| Note::new(d, s)).collect(), scale.clone())
    };
    vec![
        chord(0.0, [-3, 0, 4, 9, 12]),  // Am
        chord(1.0, [-7, -3, 0, 5, 9]),  // F
        chord(2.0, [-12, -5, 0, 4, 7]), // C
        chord(3.0, [-5, -1, 2, 7, 11]), // G
    ]
}

// ── Players ──────────────────────────────────────────────────────────────────

/// One step of an acid pattern: an interval above the chord root, an accent,
/// and whether it slides into the next note.
#[derive(Clone, Copy)]
struct AcidStep {
    interval: i32,
    accent: bool,
    slide: bool,
}

/// A 303-style bassline: a seeded 16-step pattern over the chord root,
/// re-rolled every 4 bars. Higher intensity fills in more steps.
struct AcidBassist {
    rng: StdRng,
    pattern: [Option<AcidStep>; 16],
    /// Per-step threshold: a step plays when intensity is above it.
    density: [f32; 16],
}

impl AcidBassist {
    fn new(seed: u64) -> Self {
        let mut bass = Self { rng: StdRng::seed_from_u64(seed), pattern: [None; 16], density: [0.0; 16] };
        bass.reroll();
        bass
    }

    fn reroll(&mut self) {
        const INTERVALS: [(i32, f32); 6] = [(0, 6.0), (12, 3.0), (7, 2.0), (10, 1.0), (3, 1.0), (-2, 0.5)];
        let total: f32 = INTERVALS.iter().map(|i| i.1).sum();
        for step in 0..16 {
            let mut pick = self.rng.random::<f32>() * total;
            let interval = INTERVALS.iter().find(|(_, w)| { pick -= w; pick <= 0.0 }).map_or(0, |i| i.0);
            self.pattern[step] = Some(AcidStep {
                interval,
                accent: self.rng.random::<f32>() < 0.3,
                slide: self.rng.random::<f32>() < 0.25,
            });
            // Downbeats are always there; off-beats need more intensity.
            self.density[step] = match step % 4 {
                0 => 0.0,
                2 => 0.35,
                _ => 0.5 + self.rng.random::<f32>() * 0.4,
            };
        }
    }
}

impl MusicPlayer for AcidBassist {
    fn play(&mut self, beat: Beat, notes: &mut Vec<NoteEvent>, intensity: f32, chord: &Chord) {
        let step = (beat.beat * 4 + beat.sixteenth) as usize % 16;
        if step == 0 && beat.bar_count > 0 && beat.bar_count.is_multiple_of(4) {
            self.reroll();
        }
        let plays = |s: usize| intensity >= self.density[s % 16];
        let Some(acid) = self.pattern[step].filter(|_| plays(step)) else { return };
        let root = chord.chord_notes[0].midi_note_diff;
        // A slide holds the note past the next step's start; the legato patch
        // then glides into it instead of re-triggering.
        let slide = acid.slide && plays(step + 1);
        notes.push(
            NoteEvent::from_diff(root + acid.interval)
                .velocity(if acid.accent { 1.0 } else { 0.65 })
                .length(if slide { 1.3 } else { 0.5 }),
        );
    }
}

/// Holds the chord's three strongest tones for a whole bar.
struct PadPlayer;

impl MusicPlayer for PadPlayer {
    fn play(&mut self, beat: Beat, notes: &mut Vec<NoteEvent>, intensity: f32, chord: &Chord) {
        if beat.beat != 0 || beat.sixteenth != 0 || intensity < 0.15 {
            return;
        }
        for note in chord.chord_notes.iter().filter(|n| n.strength >= 0.8) {
            notes.push(NoteEvent::from_diff(note.midi_note_diff).velocity(0.5 + intensity * 0.5).length(16.0));
        }
    }
}

/// Random high chord tones on 8th notes, only when intensity is high.
struct Sparkles(StdRng);

impl MusicPlayer for Sparkles {
    fn play(&mut self, beat: Beat, notes: &mut Vec<NoteEvent>, intensity: f32, chord: &Chord) {
        let chance = (intensity - 0.6) * 1.2;
        if !beat.sixteenth.is_multiple_of(2) || self.0.random::<f32>() >= chance {
            return;
        }
        let tone = chord.chord_notes[self.0.random_range(0..chord.chord_notes.len())];
        notes.push(NoteEvent::from_diff(tone.midi_note_diff).velocity(0.4 + self.0.random::<f32>() * 0.5).length(4.0));
    }
}

// ── App ──────────────────────────────────────────────────────────────────────

#[derive(Component)]
struct FilterFollowsIntensity {
    low: f32,
    high: f32,
}

fn main() {
    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(4))),
            AssetPlugin::default(),
        ))
        .add_plugins((MusicPlugin { beats: 4, note_type: 4, bpm: 112.0 }, MixPlugin, SynthPlugin))
        .insert_resource(Conductor { chords: chords(), chord_length_bars: 4.0 })
        .insert_resource(Intensity(0.3))
        .add_systems(Startup, setup)
        .add_systems(Update, (sweep_intensity, filters_follow_intensity, report))
        .run();
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    let (node, config) = synth(SynthPatch::acid_bass(), -6.0);
    commands.spawn((
        synth_musician("Acid", AcidBassist::new(303), -24.0, node, config),
        Section::Bass,
        FilterFollowsIntensity { low: 180.0, high: 1_400.0 },
    ));

    let (node, config) = synth(SynthPatch::pluck(), -12.0);
    commands.spawn((
        synth_musician("Arp", Arpeggiator::default(), 0.0, node, config),
        Section::Harmony,
        FilterFollowsIntensity { low: 300.0, high: 2_500.0 },
    ));

    let (node, config) = synth(SynthPatch::glide_lead(), -11.0);
    commands.spawn((
        synth_musician("Lead", Soloist::with_bars(2), 12.0, node, config),
        Section::Lead,
        FilterFollowsIntensity { low: 700.0, high: 3_500.0 },
    ));

    let (node, config) = synth(SynthPatch::pad(), -4.0);
    commands.spawn((
        synth_musician("Pad", PadPlayer, -12.0, node, config),
        Section::Harmony,
        FilterFollowsIntensity { low: 400.0, high: 2_000.0 },
    ));

    let (node, config) = synth(SynthPatch::bell(), -16.0);
    commands.spawn((synth_musician("Bell", Sparkles(StdRng::seed_from_u64(9)), 24.0, node, config), Section::Fx));

    if std::env::args().any(|a| a == "--samples") {
        // Hybrid: sampled drums under synths. Instruments mix freely within one band.
        let drums = SuperDrummer::new(vec![
            create_drummer_only(assets.load("samples/glicol/808bd.wav"), 0.0, generate_kick_beat()),
            create_drummer_only(assets.load("samples/glicol/808sd.wav"), -3.0, generate_snare_beat()),
            create_drummer_only(assets.load("samples/glicol/808ch.wav"), -10.0, generate_hihat_beat()),
        ]);
        commands.spawn((Musician::new("Drums".into(), drums), Section::Drums));
    } else {
        let drums = SuperDrummer::new(vec![
            Drummer::pattern(generate_kick_beat()),
            Drummer::pattern(generate_snare_beat()),
            Drummer::pattern(generate_hihat_beat()),
        ]);
        let kit = vec![(SynthPatch::kick(), 36.0, -5.0), (SynthPatch::snare(), 55.0, -6.0), (SynthPatch::hat(), 60.0, -14.0)];
        let drums = spawn_synth_kit(&mut commands, "Drums", drums, kit);
        commands.entity(drums).insert(Section::Drums);
    }
}

/// 32-bar rise and fall, like a game encounter building and resolving.
fn sweep_intensity(clock: Res<Clock>, mut intensity: ResMut<Intensity>) {
    let phase = (clock.time_bars() / 32.0) * std::f32::consts::TAU;
    intensity.0 = 0.5 - 0.45 * phase.cos();
}

fn filters_follow_intensity(intensity: Res<Intensity>, mut synths: Query<(&FilterFollowsIntensity, &mut SynthNode)>) {
    if !intensity.is_changed() {
        return;
    }
    for (range, mut synth) in &mut synths {
        // Exponential mapping: equal intensity steps sound like equal brightness steps.
        let cutoff = range.low * (range.high / range.low).powf(intensity.0);
        if (synth.cutoff - cutoff).abs() > 1.0 {
            synth.cutoff = cutoff;
        }
    }
}

fn report(clock: Res<Clock>, intensity: Res<Intensity>, mut last_bar: Local<Option<u32>>) {
    if *last_bar != Some(clock.bar_count) {
        *last_bar = Some(clock.bar_count);
        let filled = (intensity.0 * 30.0) as usize;
        println!("bar {:>3}  intensity {:.2}  {}{}", clock.bar_count + 1, intensity.0, "█".repeat(filled), "·".repeat(30 - filled));
    }
}
