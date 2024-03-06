mod clock;
mod player;
mod conductor;
mod musicians;

use bevy_kira_audio::{AudioApp, AudioPlugin};
use bevy::prelude::*;
use bevy::utils::hashbrown::HashMap;
use musicians::{Musician, Note, Sampler};
use musicians::conductor::Conductor;
use musicians::drummer::Drummer;
use player::play_sound_on_the_beat;
use crate::clock::{Beat, Clock, progress_clock_system};
use crate::musicians::bassist::Bassist;
use crate::musicians::{Chord, MusicianType};
use crate::musicians::soloist::Soloist;

#[derive(Resource)]
pub struct Soloists;
#[derive(Resource)]
pub struct Drums;
#[derive(Resource)]
pub struct Bass;

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
        .add_audio_channel::<Soloists>()
        .add_audio_channel::<Drums>()
        .add_audio_channel::<Bass>()
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
    // commands.spawn(
    //     Musician::new(
    //         "Melody".to_string(),
    //         Sampler {
    //             handle: asset_server.load("samples/lo-fi/construction/120/acid/short/c.wav"),
    //             volume: 0.251188643150958,
    //         },
    //         Soloist::new("Solo".to_string(), 4, 4, 2),
    //         MusicianType::Solo
    //     ));
    // commands.spawn(
    //     Musician::new(
    //         "Bassist".to_string(),
    //         Sampler {
    //             handle: asset_server.load("samples/lo-fi/construction/120/bass/c.wav"),
    //             volume: 0.7
    //         },
    //         Bassist::new("Bass".to_string()),
    //         MusicianType::Bass
    //     ));

    commands.spawn(
        Musician::new(
            "Kick".to_string(),
            Sampler {
                handle: asset_server.load("samples/drums/kit-d/kick.wav"),
                volume: 1.0,
            },
            Drummer {
                name: "Kick".to_string(),
                notes: generate_kick_beat(),
            },
            MusicianType::Drums
        )
    );
    commands.spawn(
        Musician::new(
            "Kick".to_string(),
            Sampler {
                handle: asset_server.load("samples/drums/kit-d/snare.wav"),
                volume: 1.0,
            },
            Drummer {
                name: "Kick".to_string(),
                notes: generate_snare_beat(),
            },
            MusicianType::Drums
        )
    );

    commands.spawn(Musician::new(
        "Hihat".to_string(),
        Sampler {
            handle: asset_server.load("samples/drums/kit-d/hihat.wav"),
            volume: 1.0
        },
        Drummer {
            name: "HiHat".to_string(),
            notes: generate_hihat_beat(),
        },
        MusicianType::Drums
    ));

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
        ], scale_notes.clone()),
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


pub fn generate_kick_beat() -> HashMap<u32, Note> {
    // 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15
    // 0       1       2         3
    HashMap::from([
        (4, Note {
            midi_note_diff: 0,
            strength: 0.5,
        }),
        (12, Note {
            midi_note_diff: 0,
            strength: 0.5,
        }),
    ])
}

pub fn generate_snare_beat() -> HashMap<u32, Note> {
    HashMap::from([
        (0, Note {
            midi_note_diff: 0,
            strength: 0.5,
        }),
        (8, Note {
            midi_note_diff: 0,
            strength: 0.5,
        })
    ])
}

pub fn generate_hihat_beat() -> HashMap<u32, Note> {
    HashMap::from([
        (0, Note {
            midi_note_diff: 0,
            strength: 0.5,
        }),
        (4, Note {
            midi_note_diff: 0,
            strength: 0.5,
        }),
        (8, Note {
            midi_note_diff: 0,
            strength: 0.5,
        }),
        (12, Note {
            midi_note_diff: 0,
            strength: 0.5,
        }),
        ])
}
