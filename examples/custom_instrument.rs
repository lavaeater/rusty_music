use bevy::DefaultPlugins;
use bevy::prelude::{App, AssetServer, ButtonInput, Commands, KeyCode, Res, ResMut, Resource, Startup, Update};
use bevy::reflect::Enum;
use bevy_kira_audio::{AudioApp, AudioChannel};
use rusty_music::musicians::{drummer, Drums, Musician, MusicianType, Sampler};
use rusty_music::musicians::bassist::Bassist;
use rusty_music::musicians::conductor::Conductor;
use rusty_music::musicians::drummer::{Drummer, generate_hihat_beat, generate_kick_beat, generate_snare_beat};
use rusty_music::musicians::soloist::Soloist;
use rusty_music::{generate_chords, MusicPlugin};
use rusty_music::player::Intensity;

#[derive(Debug, Resource)]
pub struct MyChannel;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(MusicPlugin {
            beats: 4,
            note_type: 4,
            bpm: 120.0,
        })
        .add_audio_channel::<MyChannel>()
        .add_systems(Update, change_intensity)
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

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    commands.spawn(
        Musician::new(
            "Melody".to_string(),
            Sampler {
                handle: asset_server.load("samples/lo-fi/construction/120/acid/short/c.wav"),
                volume: 0.251188643150958,
            },
            Soloist::new("Solo".to_string(), 4, 4, 2),
            MusicianType::Solo,
        ));
    commands.spawn(
        Musician::new(
            "Bassist".to_string(),
            Sampler {
                handle: asset_server.load("samples/lo-fi/construction/120/bass/c.wav"),
                volume: 0.7,
            },
            Bassist::new("Bass".to_string()),
            MusicianType::Bass,
        ));

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
            MusicianType::Drums,
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
            MusicianType::Drums,
        )
    );

    commands.spawn(Musician::new(
        "Hihat".to_string(),
        Sampler {
            handle: asset_server.load("samples/drums/kit-d/hihat.wav"),
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

