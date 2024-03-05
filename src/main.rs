mod clock;
mod player;
mod conductor;
mod musicians;

use bevy_kira_audio::AudioPlugin;
use bevy::prelude::*;
use bevy::utils::hashbrown::HashMap;
use musicians::{Note, Sampler};
use musicians::conductor::Conductor;
use musicians::drummer::Drummer;
use player::play_sound_on_the_beat;
use crate::clock::{Beat, Clock, progress_clock_system};
use crate::musicians::bassist::{Bassist, Musician};
use crate::musicians::Chord;

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

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    commands.spawn(
        Musician::new(
            "Bassist".to_string(),
            Bassist {
                name: "Bass".to_string(),
                sampler: Sampler {
                    handle: asset_server.load("samples/lo-fi/construction/120/bass/c.wav")
                },
            })
    );

    commands.spawn(
        Musician::new(
            "Drummer".to_string(),
            Drummer {
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
            })
    );

    commands.spawn(Musician::new("Hihat".to_string(), Drummer {
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
    }));

    commands.insert_resource((Conductor {
        chords: vec![
            Chord::new(0, vec![
                Note::new(0, 1.0),
                Note::new(0, 0.5),
            ], vec![]),
            Chord::new(1, vec![
                Note::new(-2, 1.0),
                Note::new(1, 0.5),
            ], vec![]),
        ]
    }));
}
