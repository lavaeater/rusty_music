use bevy::DefaultPlugins;
use bevy::prelude::{App, AssetServer, ButtonInput, Commands, KeyCode, Res, ResMut, Startup, Update};
use rusty_music::musicians::conductor::Conductor;
use rusty_music::musicians::drummer::{generate_hihat_beat, generate_kick_beat, generate_snare_beat};
use rusty_music::{create_arpeggiator, create_bassist, create_drummer, create_soloist, generate_chords, MusicPlugin};
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
        create_soloist("Melody".to_string(),
                       asset_server.load("samples/lo-fi/construction/120/acid/short/c.wav"),
                       0.251188643150958,
                       2,
                       4,
                       2));
    commands.spawn(
        create_arpeggiator("Arpeggio".to_string(), asset_server.load("samples/lo-fi/construction/120/acid/long/c.wav"), 0.251188643150958));

    commands.spawn(
        create_bassist("Bassist".to_string(), asset_server.load("samples/lo-fi/construction/120/bass/c.wav"), 0.7));

    commands.spawn(
        create_drummer("Kick".to_string(), asset_server.load("samples/drums/kit-d/kick.wav"), 1.0, generate_kick_beat())
    );
    commands.spawn(
        create_drummer("Snare".to_string(), asset_server.load("samples/drums/kit-d/snare.wav"), 1.0, generate_snare_beat())
    );


    commands.spawn(create_drummer("Hihat".to_string(), asset_server.load("samples/drums/kit-d/hihat.wav"), 1.0, generate_hihat_beat()));

    commands.insert_resource(Conductor {
        chords: generate_chords()
    });
}
