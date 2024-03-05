mod clock;
mod player;
mod conductor;
mod musicians;

use bevy_kira_audio::AudioPlugin;
use bevy::prelude::*;
use bevy::utils::hashbrown::HashMap;
use musicians::{Musician, Note, Sampler};
use musicians::conductor::Conductor;
use musicians::drummer::Drummer;
use player::play_sound_on_the_beat;
use crate::clock::{Beat, Clock, progress_clock_system};
use crate::musicians::bassist::Bassist;
use crate::musicians::Chord;
use crate::musicians::soloist::Soloist;

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
            "Melody".to_string(),
            Soloist::new("Solo".to_string(),
                         Sampler {
                             handle: asset_server.load("samples/lo-fi/construction/120/acid/short/c.wav")
                         },
            4,4, 2),
        ));
    commands.spawn(
        Musician::new(
            "Bassist".to_string(),
            Bassist::new("Bass".to_string(),
                         Sampler {
                             handle: asset_server.load("samples/lo-fi/construction/120/bass/c.wav")
                         }),
        ));

    commands.spawn(
        Musician::new(
            "Drummer".to_string(),
            Drummer {
                name: "Kick".to_string(),
                sampler: Sampler {
                    handle: asset_server.load("samples/drums/kit-d/80PD_KitD-Kick.wav")
                },
                notes: generate_beat(),
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

    commands.insert_resource(Conductor {
        chords: generate_chords()
    });
}

pub fn generate_chords() -> Vec<Chord> {
    let scale_notes = vec![
        Note::new(-1, 1.0),
        Note::new(1, 0.2),
        Note::new(3, 0.6),
        Note::new(4, 0.5),
        Note::new(6, 0.7),
        Note::new(8, 0.4),
        Note::new(9, 0.1),
    ];
    vec![
        Chord::new(0, vec![
            Note::new(0, 1.0),
            Note::new(0, 0.5),
            Note::new(2, 0.2),
        ], scale_notes.clone()),
        Chord::new(1, vec![
            Note::new(-2, 1.0),
            Note::new(1, 0.5),
            Note::new(3, 0.1),
        ] , scale_notes.clone()),
        Chord::new(2, vec![
            Note::new(-1, 1.0),
            Note::new(2, 0.7),
            Note::new(-2, 0.4),
        ], scale_notes.clone()),
        Chord::new(3, vec![
            Note::new(-2, 0.2),
            Note::new(1, 0.5),
            Note::new(-4, 1.0),
        ], scale_notes.clone()),
    ]
}


pub fn generate_beat() -> HashMap<u32, Note> {
    HashMap::from([
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
    ])
}
