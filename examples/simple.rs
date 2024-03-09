use bevy::DefaultPlugins;
use bevy::prelude::{App, AssetServer, ButtonInput, Commands, KeyCode, Res, ResMut, Startup, Update};
use rusty_music::musicians::{Musician, Sampler};
use rusty_music::musicians::bassist::Bassist;
use rusty_music::musicians::conductor::Conductor;
use rusty_music::musicians::drummer::{Drummer, generate_hihat_beat, generate_kick_beat, generate_snare_beat};
use rusty_music::musicians::soloist::Soloist;
use rusty_music::{generate_chords, MusicPlugin};
use rusty_music::musicians::arpegggiator::Arpeggiator;
use rusty_music::player::Intensity;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(MusicPlugin {
            beats: 4,
            note_type: 4,
            bpm: 120.0,
        })
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
            Soloist::new(Sampler {
                handle: asset_server.load("samples/lo-fi/construction/120/acid/short/c.wav"),
                volume: 0.251188643150958,
            }, 4, 4, 2),
        ));
    commands.spawn(
        Musician::new(
            "Arpeggio".to_string(),
            Arpeggiator::new(Sampler {
                handle: asset_server.load("samples/lo-fi/construction/120/acid/long/c.wav"),
                volume: 0.251188643150958,
            }),
        ));
    commands.spawn(
        Musician::new(
            "Bassist".to_string(),
            Bassist::new(Sampler {
                handle: asset_server.load("samples/lo-fi/construction/120/bass/c.wav"),
                volume: 0.7,
            }),
        ));

    commands.spawn(
        Musician::new(
            "Kick".to_string(),
            Drummer::new(
                Sampler {
                    handle: asset_server.load("samples/drums/kit-d/kick.wav"),
                    volume: 1.0,
                },
                generate_kick_beat(),
            ),
        )
    );
    commands.spawn(
        Musician::new(
            "Kick".to_string(),
            Drummer::new(
                Sampler {
                    handle: asset_server.load("samples/drums/kit-d/snare.wav"),
                    volume: 1.0,
                },
                generate_snare_beat(),
            ),
        )
    );

    commands.spawn(Musician::new(
        "Hihat".to_string(),
        Drummer::new(
            Sampler {
                handle: asset_server.load("samples/drums/kit-d/hihat.wav"),
                volume: 1.0,
            },
            generate_hihat_beat(),
        ),
    ));

    commands.insert_resource(Conductor {
        chords: generate_chords()
    });
}

