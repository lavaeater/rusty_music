#![allow(clippy::precedence)]

use bevy::utils::HashMap;
use {bevy::prelude::*, bevy_fundsp::prelude::*, bevy_kira_audio::prelude::*};
use rusty_music::musicians::{Chord, Musician, MusicianType, Note, Sampler};
use rusty_music::musicians::bassist::Bassist;
use rusty_music::musicians::conductor::Conductor;
use rusty_music::musicians::drummer::Drummer;
use rusty_music::musicians::soloist::Soloist;
use rusty_music::MusicPlugin;
use rusty_music::player::Intensity;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(MusicPlugin {
            beats: 4,
            note_type: 4,
            bpm: 120.0,
        })
        .add_plugins(DspPlugin::default())
        .add_systems(Update, change_intensity)
        .add_dsp_source(sine_wave, SourceType::Static { duration: 0.25 })
        .add_dsp_source(triangle_wave, SourceType::Static { duration: 0.25 })
        .add_dsp_source(kick_wave, SourceType::Static { duration: 0.25 })
        .add_dsp_source(snare_wave, SourceType::Static { duration: 0.25 })
        .add_dsp_source(hat_wave, SourceType::Static { duration: 0.25 })
        .add_dsp_source(square_wave, SourceType::Static { duration: 0.25 })
        .add_systems(Startup, setup)
        .run();
}

fn change_intensity(
    mut intensity: ResMut<Intensity>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
) {
    if keyboard_input.just_pressed(KeyCode::ArrowUp) {
        intensity.0 += 0.1;
        println!("Intensity: {}", intensity.0);
    }
    if keyboard_input.just_pressed(KeyCode::ArrowDown) {
        intensity.0 -= 0.1;
        println!("Intensity: {}", intensity.0);
    }
    if intensity.0 > 1.0 {
        intensity.0 = 0.0;
    }
    if intensity.0 < 0.0 {
        intensity.0 = 1.0;
    }
}
fn sine_wave() -> impl AudioUnit32 {
    // Note is A4
    sine_hz(440.0) >> split::<U2>() * 0.2
}

fn triangle_wave() -> impl AudioUnit32 {
    // Note is G4
    triangle_hz(392.0) >> split::<U2>() * 0.2
}

fn square_wave() -> impl AudioUnit32 {
    // Note is G4
    square_hz(392.0) >> split::<U2>() * 0.2
}

fn kick_wave() -> impl AudioUnit32 {
    // Note is G4
    square_hz(40.0) >> split::<U2>() * 0.2
}

fn hat_wave() -> impl AudioUnit32 {
    // Note is G4
    square_hz(1200.0) >> split::<U2>() * 0.2
}

fn snare_wave() -> impl AudioUnit32 {
    // Note is G4
    square_hz(800.0) >> split::<U2>() * 0.2
}

fn setup(
    mut commands: Commands,
    mut assets: ResMut<Assets<AudioSource>>,
    dsp_manager: Res<DspManager>,
) {
    let source = dsp_manager
        .get_graph(square_wave)
        .unwrap_or_else(|| panic!("DSP source not found!"));
    let audio_source = DefaultBackend::convert_to_audio_source(source.clone());
    let audio_source = assets.add(audio_source);

    commands.spawn(
        Musician::new(
            "Melody".to_string(),
            Sampler {
                handle: audio_source,
                volume: 0.7,
            },
            Soloist::new("Solo".to_string(), 4, 4, 2),
            MusicianType::Solo,
        ));

    let source = dsp_manager
        .get_graph(triangle_wave)
        .unwrap_or_else(|| panic!("DSP source not found!"));
    let audio_source = DefaultBackend::convert_to_audio_source(source.clone());
    let audio_source = assets.add(audio_source);

    commands.spawn(
        Musician::new(
            "Bassist".to_string(),
            Sampler {
                handle: audio_source,
                volume: 1.0,
            },
            Bassist::new("Bass".to_string()),
            MusicianType::Bass,
        ));

    let source = dsp_manager
        .get_graph(kick_wave)
        .unwrap_or_else(|| panic!("DSP source not found!"));
    let audio_source = DefaultBackend::convert_to_audio_source(source.clone());
    let audio_source = assets.add(audio_source);

    commands.spawn(
        Musician::new(
            "Kick".to_string(),
            Sampler {
                handle: audio_source,
                volume: 1.0,
            },
            Drummer {
                name: "Kick".to_string(),
                notes: generate_kick_beat(),
            },
            MusicianType::Drums,
        )
    );

    let source = dsp_manager
        .get_graph(snare_wave)
        .unwrap_or_else(|| panic!("DSP source not found!"));
    let audio_source = DefaultBackend::convert_to_audio_source(source.clone());
    let audio_source = assets.add(audio_source);

    commands.spawn(
        Musician::new(
            "Snare".to_string(),
            Sampler {
                handle: audio_source,
                volume: 1.0,
            },
            Drummer {
                name: "Snare".to_string(),
                notes: generate_snare_beat(),
            },
            MusicianType::Drums,
        )
    );

    let source = dsp_manager
        .get_graph(hat_wave)
        .unwrap_or_else(|| panic!("DSP source not found!"));
    let audio_source = DefaultBackend::convert_to_audio_source(source.clone());
    let audio_source = assets.add(audio_source);

    commands.spawn(Musician::new(
        "Hihat".to_string(),
        Sampler {
            handle: audio_source,
            volume: 1.0,
        },
        Drummer {
            name: "HiHat".to_string(),
            notes: generate_hihat_beat(),
        },
        MusicianType::Drums,
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


pub fn generate_kick_beat() -> HashMap<(u32, u32), Note> {
    // 0 1 2 3 0 1 2 3 0 1 2 3 0 1 2 3
    // 0       1       2       3
    HashMap::from([
        ((1, 0), Note {
            midi_note_diff: 0,
            strength: 0.25,
        }),
        ((3, 0), Note {
            midi_note_diff: 0,
            strength: 0.5,
        }),
        ((3, 2), Note {
            midi_note_diff: 0,
            strength: 0.7,
        }),
    ])
}

pub fn generate_snare_beat() -> HashMap<(u32, u32), Note> {
    HashMap::from([
        ((0, 0), Note {
            midi_note_diff: 0,
            strength: 0.5,
        }),
        ((2, 0), Note {
            midi_note_diff: 0,
            strength: 0.5,
        })
    ])
}

pub fn generate_hihat_beat() -> HashMap<(u32, u32), Note> {
    HashMap::from([
        ((0, 0), Note {
            midi_note_diff: 0,
            strength: 0.5,
        }),
        ((1, 0), Note {
            midi_note_diff: 0,
            strength: 0.5,
        }),
        ((2, 0), Note {
            midi_note_diff: 0,
            strength: 0.5,
        }),
        ((3, 0), Note {
            midi_note_diff: 0,
            strength: 0.5,
        }),
    ])
}
