use bevy::DefaultPlugins;
use bevy::prelude::*;
use bevy::input::keyboard::KeyCode;
use rusty_music::musicians::conductor::Conductor;
use rusty_music::musicians::drummer::{generate_hihat_beat, generate_kick_beat, generate_snare_beat, SuperDrummer};
use rusty_music::{create_bassist, create_drummer_only, create_soloist, generate_chords, MusicPlugin};
use rusty_music::musicians::Musician;
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
        create_soloist(
            "Melody".to_string(),
            asset_server.load("samples/glicol/pluck.wav"),
            0.5,
            2,  // record_bars (AABA sections are each 2 bars)
        ));

    commands.spawn(
        create_bassist(
            "Bassist".to_string(),
            asset_server.load("samples/glicol/bass3.wav"),
            0.8,
        ));

    commands.spawn(
        Musician::new(
            "Drummer".to_string(),
            SuperDrummer::new(vec![
                create_drummer_only(asset_server.load("samples/glicol/kick1.wav"), 1.0, generate_kick_beat()),
                create_drummer_only(asset_server.load("samples/glicol/snare1.wav"), 0.9, generate_snare_beat()),
                create_drummer_only(asset_server.load("samples/glicol/closedhh.wav"), 0.6, generate_hihat_beat()),
            ]),
        ));

    commands.insert_resource(Conductor {
        chords: generate_chords(),
        chord_length_bars: 4.0,
    });
}
