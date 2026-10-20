//! synth_presets — audition every built-in synth preset.
//!
//! Each preset plays a short phrase: a rising arpeggio whose notes overlap
//! (so mono / legato patches show off their glide), then a held chord. Drum
//! presets play a little pattern instead. Pass a name to hear just one:
//!
//! `cargo run --example synth_presets --features synth [-- acid]`

use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use bevy_seedling::prelude::*;

use rusty_music::synth::{send_note, synth, NoteMsg, SynthPatch, SynthPlugin};

/// Seconds each preset gets.
const SLOT: f64 = 4.5;

#[derive(Component)]
struct Slot(usize);

fn main() {
    let filter = std::env::args().nth(1).filter(|a| !a.starts_with('-'));
    let presets: Vec<SynthPatch> = SynthPatch::presets()
        .into_iter()
        .filter(|p| filter.as_ref().is_none_or(|f| p.name.contains(f.as_str())))
        .collect();
    if presets.is_empty() {
        let names: Vec<_> = SynthPatch::presets().into_iter().map(|p| p.name).collect();
        eprintln!("no preset matches; try one of: {}", names.join(", "));
        return;
    }

    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(4))),
            AssetPlugin::default(),
            SeedlingPlugins,
            SynthPlugin,
        ))
        .insert_resource(Presets(presets))
        .add_systems(Startup, setup)
        .add_systems(Update, play)
        .run();
}

#[derive(Resource)]
struct Presets(Vec<SynthPatch>);

fn setup(mut commands: Commands, presets: Res<Presets>) {
    for (i, patch) in presets.0.iter().enumerate() {
        commands.spawn((Slot(i), synth(patch.clone(), -3.0)));
    }
}

/// `(seconds from slot start, midi, length in seconds, velocity)`
fn phrase(patch: &SynthPatch) -> Vec<(f64, f32, f64, f32)> {
    let drum = |midi: f32, hits: &[(f64, f32)]| hits.iter().map(|&(t, v)| (t, midi, 0.1, v)).collect();
    match patch.name.as_str() {
        "kick" => drum(36.0, &[(0.0, 1.0), (0.5, 1.0), (1.0, 1.0), (1.375, 0.5), (1.5, 1.0), (2.5, 0.7), (3.0, 1.0)]),
        "snare" => drum(55.0, &[(0.5, 1.0), (1.5, 1.0), (2.375, 0.3), (2.5, 1.0), (3.0, 0.6), (3.125, 0.8), (3.25, 1.0)]),
        "hat" => drum(60.0, &(0..24).map(|k| (k as f64 * 0.125, if k % 2 == 0 { 1.0 } else { 0.5 })).collect::<Vec<_>>()),
        name => {
            let root = if name.contains("bass") { 36.0 } else { 60.0 };
            let mut notes: Vec<_> = [0.0, 4.0, 7.0, 12.0, 16.0, 12.0, 7.0, 4.0]
                .iter()
                .enumerate()
                // 0.3 s notes every 0.25 s: overlapping, so legato patches slide.
                .map(|(k, &step)| (k as f64 * 0.25, root + step, 0.3, if k % 4 == 0 { 1.0 } else { 0.7 }))
                .collect();
            for (k, step) in [0.0, 7.0, 16.0].into_iter().enumerate() {
                notes.push((2.25 + k as f64 * 0.01, root + step, 1.6, 0.8));
            }
            notes
        }
    }
}

fn play(
    time: Res<Time<Audio>>,
    mut context: ResMut<AudioContext>,
    presets: Res<Presets>,
    synths: Query<(&Slot, &FirewheelNode)>,
    mut started: Local<Option<f64>>,
    mut announced: Local<usize>,
    mut exit: MessageWriter<AppExit>,
) {
    let now = time.now().0;
    let Some(start) = *started else {
        // Schedule the whole audition once every synth has joined the audio graph.
        if synths.iter().count() < presets.0.len() {
            return;
        }
        let start = now + 0.3;
        for (slot, node) in &synths {
            let patch = &presets.0[slot.0];
            let slot_start = start + slot.0 as f64 * SLOT;
            for (id, (t, midi, length, velocity)) in phrase(patch).into_iter().enumerate() {
                let id = id as u32;
                send_note(&mut context, node, NoteMsg::On { id, midi, velocity }, InstantSeconds(slot_start + t));
                send_note(&mut context, node, NoteMsg::Off { id }, InstantSeconds(slot_start + t + length));
            }
        }
        *started = Some(start);
        return;
    };

    let current = ((now - start) / SLOT).floor().max(0.0) as usize;
    if current < presets.0.len() && *announced <= current {
        *announced = current + 1;
        println!("▶ {}", presets.0[current].name);
    }
    if now > start + presets.0.len() as f64 * SLOT + 1.0 {
        exit.write(AppExit::Success);
    }
}
