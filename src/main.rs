mod clock;

use bevy::audio::PlaybackMode;
use {bevy::prelude::*};
use crate::clock::{Beat, beat_system, Clock, play_sound_on_the_beat};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .insert_resource(Clock::new(4.0, 120.0))
        .add_event::<Beat>()
        .add_systems(Startup, setup)
        .add_systems(Update, (
            beat_system,
            play_sound_on_the_beat
        ))
        .run();
}

#[derive(Component, Clone, Copy, PartialEq)]
pub struct Sample {
    pub play_at_sixteenth: u32,
    pub play_at_offset: u32
}

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    commands.spawn((
        Sample {
            play_at_sixteenth: 2,
            play_at_offset: 1
        },
        AudioBundle {
            source: asset_server.load("samples/drums/kit-d/80PD_KitD-Kick.wav"),
            settings: PlaybackSettings {
                paused: true,
                mode: PlaybackMode::Once,
                ..default()
            },
        })
    );
}
