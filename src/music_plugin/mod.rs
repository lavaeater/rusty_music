use bevy::app::Update;
use bevy::prelude::{App, Plugin};
use bevy_kira_audio::{AudioApp, AudioPlugin};
use crate::clock::{Beat, Clock, progress_clock_system};
use crate::player::{Intensity, play_sound_on_the_beat};
use crate::{Bass, Drums, Soloists};

pub struct MusicPlugin {
    pub beats: u32,
    pub note_type: u32,
    pub bpm: f32,
}

impl Default for MusicPlugin {
    fn default() -> Self {
        Self {
            beats: 4,
            note_type: 4,
            bpm: 80.0,
        }
    }
}

impl Plugin for MusicPlugin {
    fn build(&self, app: &mut App) {
        app
        .add_plugins(AudioPlugin)
            .insert_resource(Clock::new(self.beats, self.note_type, self.bpm))
            .insert_resource(Intensity(0.5))
            .add_audio_channel::<Soloists>()
            .add_audio_channel::<Drums>()
            .add_audio_channel::<Bass>()
            .add_event::<Beat>()
            .add_systems(Update, (
                progress_clock_system,
                play_sound_on_the_beat));
    }
}