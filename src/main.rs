mod clock;
mod player;
mod conductor;

use bevy_kira_audio::{AudioPlugin, AudioSource};
use bevy::prelude::*;
use bevy::utils::hashbrown::HashMap;
use player::play_sound_on_the_beat;
use crate::clock::{Beat, Clock, Conductor, Drummer, Note, progress_clock_system, Sampler};

fn main() {
    let beats = 4;
    let note_type = 4;
    let bpm = 80.0;
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(AudioPlugin)
        .insert_resource(Clock::new(beats, note_type, bpm))
        // .add_systems(FixedUpdate, progress_clock_system)
        // configure our fixed timestep schedule to run twice a second
        // .insert_resource(Time::<Fixed>::from_seconds((60.0 / bpm as f64 / beats as f64) / (beats as f64 / beats as f64)))
        .add_event::<Beat>()
        .add_systems(Startup, setup)
        .add_systems(Update, (
            progress_clock_system,
            play_sound_on_the_beat))
        .run();
}

#[derive(Component, Clone, PartialEq)]
pub struct Sample {
    pub play_on_every_n_beats: u32,
    pub play_at_offset: u32,
    pub handle: Handle<AudioSource>,
}

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {

    commands.insert_resource((Conductor {
        musicians: vec![
            Box::new(Drummer {
                name: "Kick".to_string(),
                sampler: Sampler {
                    handle: asset_server.load("samples/drums/kit-d/80PD_KitD-Kick.wav")
                },
                notes: HashMap::from([
                    (0, Note {
                        midi_note_diff: 0,
                        strength: 0.5,
                    }),
                    (2, Note {
                        midi_note_diff: 0,
                        strength: 1.0,
                    }),
                    (4, Note {
                        midi_note_diff: 0,
                        strength: 0.5,
                    }),
                    (6, Note {
                        midi_note_diff: 0,
                        strength: 1.0,
                    }),
                    (8, Note {
                        midi_note_diff: 0,
                        strength: 0.5,
                    }),
                    (10, Note {
                        midi_note_diff: 0,
                        strength: 1.0,
                    }),
                    (12, Note {
                        midi_note_diff: 0,
                        strength: 0.5,
                    }),
                    (15, Note {
                        midi_note_diff: 0,
                        strength: 1.0,
                    })
                ]),
            }), Box::new(Drummer {
                name: "HiHat".to_string(),
                sampler: Sampler {
                    handle: asset_server.load("samples/drums/kit-d/80PD_KitD-ClHat.wav")
                },
                notes: HashMap::from((0..=15).step_by(2).map(|i| {
                    let mut note_index = i + 2;
                    if note_index > 15 {
                        note_index = 1;
                    }
                    (note_index, Note {
                        midi_note_diff: 0,
                        strength: 0.5,
                    })
                }).collect::<HashMap<u32, Note>>()),
            }),
        ]
    }));
    // commands.spawn((
    //     Sample {
    //         play_on_every_n_beats: 4,
    //         play_at_offset: 0,
    //         handle: asset_server.load("samples/drums/kit-d/80PD_KitD-Kick.wav")
    //     },)
    // );
    //
    // commands.spawn((
    //     Sample {
    //         play_on_every_n_beats: 1,
    //         play_at_offset: 2,
    //         handle: asset_server.load("samples/drums/kit-d/80PD_KitD-ClHat.wav")
    //     },)
    // );
    //
    // commands.spawn((
    //     Sample {
    //         play_on_every_n_beats: 2,
    //         play_at_offset: 2,
    //         handle: asset_server.load("samples/drums/kit-d/80PD_KitD-Tom[Mid].wav")
    //     },)
    // );
}
