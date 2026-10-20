//! synth_spike — Phase 0 of `synth-roadmap.md`: glicol voices inside Firewheel.
//!
//! Four synth instruments (sub bass, pluck arpeggio, pad, saw lead) play a
//! seeded D-minor groove. Notes are scheduled ahead on the *audio clock*
//! (`Time<Audio>`), not on frame ticks, so timing is sample-accurate however
//! the frame rate wobbles. The pluck's filter sweeps up and down through its
//! `SynthNode` component, as a game would drive it from tension or danger.
//!
//! `cargo run --example synth_spike --features synth` — Ctrl-C to stop.

use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy_seedling::prelude::*;
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use rusty_music::synth::{send_note, NoteMsg, SynthConfig, SynthNode, SynthPatch, SynthPlugin};

const BPM: f64 = 110.0;
/// 16th notes per second.
const STEP: f64 = 60.0 / BPM / 4.0;
/// How far ahead of the audio clock notes are scheduled.
const LOOKAHEAD: f64 = 0.15;
/// One chord per bar: Dm, Bb, F, C (MIDI roots and triads).
const CHORDS: [(i32, [i32; 3]); 4] = [(38, [62, 65, 69]), (34, [62, 65, 70]), (41, [60, 65, 69]), (36, [60, 64, 67])];

#[derive(Component, Clone, Copy, PartialEq)]
enum Part {
    Bass,
    Pluck,
    Pad,
    Lead,
}

#[derive(Resource)]
struct Sequencer {
    start: Option<f64>,
    next_step: u64,
    next_id: u32,
    rng: StdRng,
}

fn main() {
    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(4))),
            AssetPlugin::default(),
            bevy::log::LogPlugin::default(),
            SeedlingPlugins,
            SynthPlugin,
        ))
        .insert_resource(Sequencer { start: None, next_step: 0, next_id: 0, rng: StdRng::seed_from_u64(7) })
        .add_systems(Startup, setup)
        .add_systems(Update, (schedule_notes, sweep_filter))
        .run();
}

fn setup(mut commands: Commands) {
    let synth = |patch, voices, cutoff, db| {
        (
            SynthNode { volume: Volume::Decibels(db), cutoff, resonance: 1.2 },
            SynthConfig { patch, voices },
        )
    };
    commands.spawn((Part::Bass, synth(SynthPatch::SubBass, 4, 600.0, -4.0)));
    commands.spawn((Part::Pluck, synth(SynthPatch::Pluck, 8, 1_200.0, -8.0)));
    commands.spawn((Part::Pad, synth(SynthPatch::Pad, 8, 1_500.0, -10.0)));
    commands.spawn((Part::Lead, synth(SynthPatch::SawLead, 4, 2_500.0, -9.0)));
}

/// Look-ahead scheduler: every frame, queue all 16th steps that fall inside the
/// next `LOOKAHEAD` seconds of audio time, each stamped with its exact instant.
fn schedule_notes(
    time: Res<Time<Audio>>,
    mut seq: ResMut<Sequencer>,
    mut context: ResMut<AudioContext>,
    synths: Query<(&Part, &FirewheelNode)>,
) {
    if synths.iter().count() < 4 {
        return; // wait until every synth has joined the audio graph
    }
    let now = time.now().0;
    let start = *seq.start.get_or_insert(now + 0.2);

    while start + seq.next_step as f64 * STEP < now + LOOKAHEAD {
        let step = seq.next_step;
        let at = start + step as f64 * STEP;
        let s16 = (step % 16) as i32;
        let (root, triad) = CHORDS[((step / 16) % 4) as usize];

        let mut notes: Vec<(Part, f32, f32, f64)> = Vec::new(); // part, midi, velocity, length (s)
        // Bass: root on the beat, octave pops on some off-beat 8ths.
        if s16 % 4 == 0 {
            notes.push((Part::Bass, root as f32, 1.0, STEP * 3.0));
        } else if s16 % 2 == 0 && seq.rng.random_bool(0.4) {
            notes.push((Part::Bass, (root + 12) as f32, 0.7, STEP));
        }
        // Pluck: 16th arpeggio up through two octaves of the triad.
        let tone = triad[(step % 3) as usize] + 12 * ((step / 3) % 2) as i32;
        notes.push((Part::Pluck, tone as f32, if s16 % 4 == 0 { 0.9 } else { 0.55 }, STEP * 0.8));
        // Pad: the triad held for the whole bar.
        if s16 == 0 {
            for &n in &triad {
                notes.push((Part::Pad, (n - 12) as f32, 0.6, STEP * 16.0));
            }
        }
        // Lead: seeded scale wanderings on 8ths, chord tones on strong beats.
        if s16 % 2 == 0 && seq.rng.random_bool(if s16 % 8 == 0 { 0.9 } else { 0.45 }) {
            let pitch = if s16 % 8 == 0 {
                triad[seq.rng.random_range(0..3)] + 12
            } else {
                const SCALE: [i32; 7] = [62, 64, 65, 67, 69, 70, 72];
                SCALE[seq.rng.random_range(0..7)] + 12
            };
            notes.push((Part::Lead, pitch as f32, 0.8, STEP * 2.0));
        }

        for (part, midi, velocity, length) in notes {
            let Some((_, node)) = synths.iter().find(|(p, _)| **p == part) else { continue };
            let id = seq.next_id;
            seq.next_id = seq.next_id.wrapping_add(1);
            send_note(&mut context, node, NoteMsg::On { id, midi, velocity }, InstantSeconds(at));
            send_note(&mut context, node, NoteMsg::Off { id }, InstantSeconds(at + length));
        }
        seq.next_step += 1;
    }
}

/// An 8-bar LFO on the pluck's cutoff: the kind of knob a game would turn.
fn sweep_filter(time: Res<Time<Audio>>, mut synths: Query<(&Part, &mut SynthNode)>) {
    let bars = time.now().0 / (STEP * 16.0);
    let lfo = 0.5 - 0.5 * (bars * std::f64::consts::TAU / 8.0).cos();
    for (part, mut synth) in &mut synths {
        if *part == Part::Pluck {
            synth.cutoff = 400.0 + 4_000.0 * lfo as f32;
        }
    }
}
