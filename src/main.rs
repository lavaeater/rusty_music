mod clock;
mod player;
mod conductor;

use bevy_kira_audio::{AudioPlugin, AudioSource};
use bevy::prelude::*;
use player::play_sound_on_the_beat;
use crate::clock::{Beat, Clock, progress_clock_system};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(AudioPlugin)
        .insert_resource(Clock::new(4, 4, 80.0))
        .add_event::<Beat>()
        .add_systems(Startup, setup)
        .add_systems(Update, (
            progress_clock_system,
            play_sound_on_the_beat
        ))
        .run();
}

#[derive(Component, Clone, PartialEq)]
pub struct Sample {
    pub play_on_every_n_beats: u32,
    pub play_at_offset: u32,
    pub handle: Handle<AudioSource>
}

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    commands.spawn((
        Sample {
            play_on_every_n_beats: 4,
            play_at_offset: 0,
            handle: asset_server.load("samples/drums/kit-d/80PD_KitD-Kick.wav")
        },)
    );

    commands.spawn((
        Sample {
            play_on_every_n_beats: 1,
            play_at_offset: 2,
            handle: asset_server.load("samples/drums/kit-d/80PD_KitD-ClHat.wav")
        },)
    );

    commands.spawn((
        Sample {
            play_on_every_n_beats: 2,
            play_at_offset: 2,
            handle: asset_server.load("samples/drums/kit-d/80PD_KitD-Tom[Mid].wav")
        },)
    );
}
