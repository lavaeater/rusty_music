#![allow(clippy::precedence)]

mod clock;

use bevy::audio::PlaybackMode;
use {bevy::prelude::*, bevy_fundsp::prelude::*, uuid::Uuid};
use crate::clock::{Beat, beat_system, Clock, play_sound_on_the_beat};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(DspPlugin::default())
        .insert_resource(Clock::new(120.0))
        .add_event::<Beat>()
        .add_dsp_source(sine_wave, SourceType::Static { duration: 60.0 / 120.0 / 2.0 }) //Fix duration later, bro
        .add_dsp_source(triangle_wave, SourceType::Static { duration: 60.0 / 120.0 / 2.0 })
        .add_systems(Startup, setup)
        .add_systems(Update, (
            beat_system,
            play_sound_on_the_beat
            ))
        .run();
}

fn sine_wave() -> impl AudioUnit32 {
    // Note is A4
    sine_hz(440.0) >> split::<U2>() * 0.2
}

fn triangle_wave() -> impl AudioUnit32 {
    // Note is G4
    triangle_hz(392.0) >> split::<U2>() * 0.2
}


#[derive(Component, Clone, Copy, PartialEq)]
enum Dsp {
    Sine,
    Triangle,
}

fn setup(
    mut commands: Commands,
    mut assets: ResMut<Assets<DspSource>>,
    dsp_manager: Res<DspManager>,
) {
    commands.spawn((
        AudioSourceBundle {
            source: assets.add(dsp_manager.get_graph(sine_wave).unwrap()),
            settings: PlaybackSettings {
                paused: true,
                mode: PlaybackMode::Once,
                ..default()
            },
        },
        Dsp::Sine,
    ));

    commands.spawn((
        AudioSourceBundle {
            source: assets.add(dsp_manager.get_graph(triangle_wave).unwrap()),
            settings: PlaybackSettings {
                paused: true,
                mode: PlaybackMode::Once,
                ..default()
            },
        },
        Dsp::Triangle,
    ));
}
