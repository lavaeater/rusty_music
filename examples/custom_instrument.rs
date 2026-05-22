use bevy::DefaultPlugins;
use bevy::prelude::{App, AssetServer, ButtonInput, Commands, KeyCode, Res, ResMut, Startup, Update};
use rusty_music::musicians::{Musician, Sampler};
use rusty_music::musicians::bassist::Bassist;
use rusty_music::musicians::conductor::Conductor;
use rusty_music::musicians::drummer::{Drummer, generate_hihat_beat, generate_kick_beat, generate_snare_beat};
use rusty_music::musicians::soloist::Soloist;
use rusty_music::{generate_chords, MusicPlugin};
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
    // Custom soloist: sax one-shot pitched by the sampler
    commands.spawn(Musician::new(
        "Sax".to_string(),
        Soloist::new(
            Sampler {
                handle: asset_server.load("samples/glicol/sax.wav"),
                volume: 0.5,
            },
            4,  // record_bars (AABA sections are each 4 bars)
        ),
    ));

    // Moog bass
    commands.spawn(Musician::new(
        "Moog Bass".to_string(),
        Bassist::new(Sampler {
            handle: asset_server.load("samples/glicol/moog.wav"),
            volume: 0.8,
        }),
    ));

    // 808 drum kit
    commands.spawn(Musician::new(
        "808 Kick".to_string(),
        Drummer::new(
            Sampler { handle: asset_server.load("samples/glicol/808bd.wav"), volume: 1.0 },
            generate_kick_beat(),
        ),
    ));

    commands.spawn(Musician::new(
        "808 Snare".to_string(),
        Drummer::new(
            Sampler { handle: asset_server.load("samples/glicol/808sd.wav"), volume: 0.9 },
            generate_snare_beat(),
        ),
    ));

    commands.spawn(Musician::new(
        "808 Hat".to_string(),
        Drummer::new(
            Sampler { handle: asset_server.load("samples/glicol/808ch.wav"), volume: 0.6 },
            generate_hihat_beat(),
        ),
    ));

    commands.insert_resource(Conductor {
        chords: generate_chords(),
        chord_length_bars: 4.0,
    });
}
