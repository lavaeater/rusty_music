mod clock;

use bevy_kira_audio::{Audio, AudioControl, AudioInstance, AudioPlugin, AudioSource};
use {bevy::prelude::*};
use crate::clock::{Beat, beat_system, Clock, play_sound_on_the_beat};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(AudioPlugin)
        .insert_resource(Clock::new(4.0, 120.0))
        .add_event::<Beat>()
        .add_systems(Startup, setup)
        .add_systems(Update, (
            beat_system,
            play_sound_on_the_beat
        ))
        .run();
}

#[derive(Component, Clone, PartialEq)]
pub struct Sample {
    pub play_every_sixteenth: u32,
    pub play_at_offset: u32,
    pub handle: Handle<AudioSource>
}

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    commands.spawn((
        Sample {
            play_every_sixteenth: 32,
            play_at_offset: 1,
            handle: asset_server.load("samples/drums/kit-d/80PD_KitD-Kick.wav")
        },)
    );
}
