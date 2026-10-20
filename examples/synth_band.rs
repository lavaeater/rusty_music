//! synth_band — the built-in musicians playing *synths* instead of samples.
//!
//! Musicians only decide notes; instruments decide sound. Here the stock
//! `Bassist`, `Arpeggiator` and `Soloist` play glicol synths (sub bass, pluck,
//! saw lead), a tiny custom pad player holds chords on a pad synth, and the
//! drums stay sampled (808 kit). Everything is scheduled on the audio clock.
//!
//! Intensity sweeps up and down over 32 bars, and is also mapped onto the
//! synths' filters: the "situational knob" a game would drive from danger,
//! speed, tension…
//!
//! `cargo run --example synth_band --features synth` — Ctrl-C to stop.

use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use bevy_seedling::prelude::Volume;

use rusty_music::clock::{Beat, Clock};
use rusty_music::instrument::NoteEvent;
use rusty_music::musicians::arpeggiator::Arpeggiator;
use rusty_music::musicians::bassist::Bassist;
use rusty_music::musicians::conductor::Conductor;
use rusty_music::musicians::drummer::{generate_hihat_beat, generate_kick_beat, generate_snare_beat, SuperDrummer};
use rusty_music::musicians::soloist::Soloist;
use rusty_music::musicians::{Chord, MusicPlayer, Musician, Note};
use rusty_music::player::Intensity;
use rusty_music::synth::{synth_musician, SynthConfig, SynthNode, SynthPatch, SynthPlugin};
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

/// Holds the chord's three strongest tones for a whole bar.
struct PadPlayer;

impl MusicPlayer for PadPlayer {
    fn play(&mut self, beat: Beat, notes: &mut Vec<NoteEvent>, intensity: f32, chord: &Chord) {
        if beat.beat != 0 || beat.sixteenth != 0 || intensity < 0.2 {
            return;
        }
        for note in chord.chord_notes.iter().filter(|n| n.strength >= 0.8) {
            notes.push(NoteEvent::from_diff(note.midi_note_diff).velocity(0.4 + intensity * 0.4).length(16.0));
        }
    }
}

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
        .add_plugins((MusicPlugin { beats: 4, note_type: 4, bpm: 104.0 }, SynthPlugin))
        .insert_resource(Conductor { chords: chords(), chord_length_bars: 4.0 })
        .insert_resource(Intensity(0.3))
        .add_systems(Startup, setup)
        .add_systems(Update, (sweep_intensity, filters_follow_intensity, report))
        .run();
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    let synth = |patch, voices, cutoff, db| {
        (SynthNode { volume: Volume::Decibels(db), cutoff, resonance: 1.3 }, SynthConfig { patch, voices })
    };

    let (node, config) = synth(SynthPatch::SubBass, 4, 500.0, -3.0);
    commands.spawn(synth_musician("Bass", Bassist::default(), -24.0, node, config));

    let (node, config) = synth(SynthPatch::Pluck, 8, 1_500.0, -9.0);
    commands.spawn((
        synth_musician("Arp", Arpeggiator::default(), 0.0, node, config),
        FilterFollowsIntensity { low: 500.0, high: 5_000.0 },
    ));

    let (node, config) = synth(SynthPatch::SawLead, 4, 2_000.0, -10.0);
    commands.spawn((
        synth_musician("Lead", Soloist::with_bars(2), 12.0, node, config),
        FilterFollowsIntensity { low: 900.0, high: 6_000.0 },
    ));

    let (node, config) = synth(SynthPatch::Pad, 8, 1_200.0, -12.0);
    commands.spawn((
        synth_musician("Pad", PadPlayer, -12.0, node, config),
        FilterFollowsIntensity { low: 400.0, high: 2_500.0 },
    ));

    // Drums stay sampled: instruments mix freely within one band.
    let drums = SuperDrummer::new(vec![
        create_drummer_only(assets.load("samples/glicol/808bd.wav"), 0.0, generate_kick_beat()),
        create_drummer_only(assets.load("samples/glicol/808sd.wav"), -3.0, generate_snare_beat()),
        create_drummer_only(assets.load("samples/glicol/808ch.wav"), -10.0, generate_hihat_beat()),
    ]);
    commands.spawn(Musician::new("Drums".into(), drums));
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
