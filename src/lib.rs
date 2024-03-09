use bevy::app::{App, Plugin, Update};
use bevy_kira_audio::{AudioPlugin};
use crate::clock::{Beat, Clock, progress_clock_system};
use crate::musicians::{Chord, Note};
use crate::player::{Intensity, play_sound_on_the_beat};

pub mod clock;
pub mod player;

pub mod conductor;
pub mod musicians;

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
            .add_event::<Beat>()
            .add_systems(Update, (
                progress_clock_system,
                play_sound_on_the_beat));
    }
}

pub fn generate_chords() -> Vec<Chord> {
    let scale_notes = vec![
        Note::new(-2, 1.0),
        Note::new(0, 0.2),
        Note::new(2, 0.6),
        Note::new(-4, 0.5),
        Note::new(8, 0.7),
        Note::new(-4, 0.4),
        Note::new(3, 0.1),
    ];
    vec![
        Chord::new(0, vec![
            Note::new(-2, 1.0),
            Note::new(0, 0.2),
            Note::new(2, 0.6),
            Note::new(-4, 0.5),
            Note::new(8, 0.7),
            Note::new(-4, 0.4),
            Note::new(3, 0.1),
        ], scale_notes.clone()),
        Chord::new(1, vec![
            Note::new(-2, 1.0),
            Note::new(1, 0.5),
            Note::new(3, 0.1),
        ], scale_notes.clone()),
        Chord::new(2, vec![
            Note::new(-1, 1.0),
            Note::new(2, 0.7),
            Note::new(-2, 0.4),
        ], scale_notes.clone()),
        Chord::new(3, vec![
            Note::new(-2, 1.0),
            Note::new(0, 0.2),
            Note::new(2, 0.6),
            Note::new(-4, 0.5),
            Note::new(8, 0.7),
            Note::new(-4, 0.4),
            Note::new(3, 0.1),
        ], scale_notes.clone()),
    ]
}
