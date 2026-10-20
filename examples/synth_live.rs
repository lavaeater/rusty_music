//! synth_live — a band whose instruments are `.glicol` files you edit while it plays.
//!
//! Every instrument here is a glicol patch in `assets/synths/`. Open one in an
//! editor, change it and save: the running synth switches to the new version
//! on its next note, while held notes ring out. Try:
//!
//! - `acid.glicol`: change `saw ~freq` to `squ ~freq`, or `lpf ~cut 6.0` to `12.0`.
//! - `lead.glicol`: raise `@param vibrato 0.006` to `0.03`.
//! - `kick.glicol`: change `mul 400` (the pitch drop) or the `0.35` decay.
//!
//! Mistakes are reported in the terminal and the synth keeps its last good
//! version. Knobs (`#name`) are live: here intensity sweeps the acid's
//! `#env` and the wobble's `#rate`, and everything's `#cutoff`.
//!
//! `cargo run --example synth_live --features synth` — Ctrl-C to stop.

use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::asset::AssetLoadFailedEvent;
use bevy::prelude::*;
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use rusty_music::clock::{Beat, Clock};
use rusty_music::instrument::{Channels, NoteEvent, SAMPLE_ROOT};
use rusty_music::mix::{MixPlugin, Section};
use rusty_music::musicians::arpeggiator::Arpeggiator;
use rusty_music::musicians::drummer::{generate_hihat_beat, generate_kick_beat, Drummer, SuperDrummer};
use rusty_music::musicians::soloist::Soloist;
use rusty_music::musicians::{Chord, MusicPlayer, Musician};
use rusty_music::situation::{Dim, MusicState, SituationPlugin};
use rusty_music::synth::{glicol_musician, GlicolPatch, GlicolSynth, Responds, SynthInstrument, SynthPlugin};
use rusty_music::MusicPlugin;

/// 16th-note acid line over the chord root; some notes slide into the next.
struct AcidLine(StdRng);

impl MusicPlayer for AcidLine {
    fn play(&mut self, beat: Beat, notes: &mut Vec<NoteEvent>, intensity: f32, chord: &Chord) {
        let downbeat = beat.sixteenth == 0;
        if !downbeat && self.0.random::<f32>() > 0.25 + intensity * 0.6 {
            return;
        }
        let root = chord.chord_notes[0].midi_note_diff;
        let interval = [0, 0, 0, 12, 7, 10, 3][self.0.random_range(0..7)];
        let slide = self.0.random::<f32>() < 0.25;
        notes.push(
            NoteEvent::from_diff(root + interval)
                .velocity(if downbeat { 1.0 } else { 0.7 })
                .length(if slide { 1.3 } else { 0.5 }),
        );
    }
}

/// Holds the chord's main tones for a whole bar.
struct PadPlayer;

impl MusicPlayer for PadPlayer {
    fn play(&mut self, beat: Beat, notes: &mut Vec<NoteEvent>, _: f32, chord: &Chord) {
        if beat.beat == 0 && beat.sixteenth == 0 {
            for note in chord.chord_notes.iter().filter(|n| n.strength >= 0.8) {
                notes.push(NoteEvent::from_diff(note.midi_note_diff).velocity(0.8).length(16.0));
            }
        }
    }
}

/// A sustained root an octave down every two bars, for the wobble bass.
struct Drone;

impl MusicPlayer for Drone {
    fn play(&mut self, beat: Beat, notes: &mut Vec<NoteEvent>, intensity: f32, chord: &Chord) {
        if intensity > 0.55 && beat.beat == 0 && beat.sixteenth == 0 {
            notes.push(NoteEvent::from_diff(chord.chord_notes[0].midi_note_diff).length(15.5));
        }
    }
}

fn main() {
    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(4))),
            AssetPlugin { watch_for_changes_override: Some(true), ..default() },
        ))
        .add_plugins((MusicPlugin { beats: 4, note_type: 4, bpm: 118.0 }, SituationPlugin, MixPlugin, SynthPlugin))
        .insert_resource(MusicState::default().with_response(3.0))
        .add_systems(Startup, setup)
        .add_systems(Update, (sweep, report_reloads, report))
        .run();
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    let patch = |name: &str| assets.load::<GlicolPatch>(format!("synths/{name}.glicol"));

    // Knob indices follow first appearance in each file: acid has #env at 0,
    // wobble has #rate at 0 (#cutoff is separate).
    commands.spawn((
        glicol_musician("Acid", AcidLine(StdRng::seed_from_u64(303)), -24.0, GlicolSynth::new(patch("acid")).volume_db(-6.0)),
        Section::Bass,
        Responds::default().cutoff(Dim::Intensity, 180.0, 900.0).knob(Dim::Tension, 0, 3.0, 9.0),
    ));
    commands.spawn((
        glicol_musician("Wobble", Drone, -36.0, GlicolSynth::new(patch("wobble")).volume_db(-10.0)),
        Section::Bass,
        Responds::default().knob(Dim::Tension, 0, 1.0, 6.0),
    ));
    commands.spawn((
        glicol_musician("Pluck", Arpeggiator::default(), 0.0, GlicolSynth::new(patch("pluck")).volume_db(-12.0)),
        Section::Harmony,
        Responds::default().cutoff(Dim::Intensity, 400.0, 2_000.0),
    ));
    commands.spawn((
        glicol_musician("Pad", PadPlayer, -12.0, GlicolSynth::new(patch("pad")).volume_db(-4.0)),
        Section::Harmony,
        Responds::default().cutoff(Dim::Intensity, 600.0, 3_000.0),
    ));
    commands.spawn((
        glicol_musician("Lead", Soloist::with_bars(2), 12.0, GlicolSynth::new(patch("lead")).volume_db(-10.0)),
        Section::Lead,
    ));

    // A drum kit of glicol synths: each drum is its own entity, played by channel.
    let kick = commands.spawn((GlicolSynth::new(patch("kick")).voices(2).volume_db(-4.0), Section::Drums)).id();
    let hat = commands.spawn((GlicolSynth::new(patch("hat")).voices(2).volume_db(-12.0), Section::Drums)).id();
    let drums = SuperDrummer::new(vec![Drummer::pattern(generate_kick_beat()), Drummer::pattern(generate_hihat_beat())]);
    let kit = Channels::default().with(SynthInstrument::on(kick)).with(SynthInstrument::on(hat).transpose(72.0 - SAMPLE_ROOT));
    commands.spawn(Musician::new("Drums".into(), drums).with_instrument(kit));
}

/// A slow 32-bar swell of intensity and tension.
fn sweep(clock: Res<Clock>, mut state: ResMut<MusicState>) {
    let phase = clock.time_bars() / 32.0 * std::f32::consts::TAU;
    state.target.intensity = 0.5 - 0.4 * phase.cos();
    state.target.tension = 0.5 - 0.45 * (phase * 2.0).cos();
}

fn report_reloads(mut loaded: MessageReader<AssetEvent<GlicolPatch>>, mut failed: MessageReader<AssetLoadFailedEvent<GlicolPatch>>, patches: Res<Assets<GlicolPatch>>) {
    for event in loaded.read() {
        if let AssetEvent::Modified { id } = event
            && let Some(patch) = patches.get(*id)
        {
            println!("♻ reloaded `{}`", patch.0.name);
        }
    }
    for event in failed.read() {
        println!("✗ {} — keeping the last good version\n{}", event.path, event.error);
    }
}

fn report(clock: Res<Clock>, state: Res<MusicState>, mut last_bar: Local<Option<u32>>) {
    if *last_bar != Some(clock.bar_count) {
        *last_bar = Some(clock.bar_count);
        let mood = state.current();
        println!("bar {:>3}  intensity {:.2}  tension {:.2}", clock.bar_count + 1, mood.intensity, mood.tension);
    }
}
