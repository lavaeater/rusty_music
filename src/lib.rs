use bevy::app::{App, Plugin, Update};
use bevy_seedling::prelude::{AudioSample, SeedlingPlugins};
use bevy::asset::Handle;
use std::collections::HashMap;
use crate::clock::{Beat, Clock, progress_clock_system};
use crate::musicians::{Chord, Musician, Note, Sampler};
use crate::musicians::arpeggiator::Arpeggiator;
use crate::musicians::bassist::Bassist;
use crate::musicians::conductor::Conductor;
use crate::musicians::drummer::Drummer;
use crate::musicians::soloist::Soloist;
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
        app.add_plugins(SeedlingPlugins);
        app
            .insert_resource(Clock::new(self.beats, self.note_type, self.bpm))
            .insert_resource(Intensity(0.5))
            .add_message::<Beat>()
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
        Note::new(4, 0.5),
        Note::new(3, 0.5),
        Note::new(2, 0.5),
        Note::new(1, 0.5),
        Note::new(0, 0.5),
        Note::new(-1, 0.5),
        Note::new(-2, 0.5),
        Note::new(-3, 0.5),
        Note::new(-4, 0.5),
        Note::new(8, 0.7),
        Note::new(-4, 0.4),
        Note::new(3, 0.1),
    ];
    vec![
        Chord::new(0.0, vec![
            Note::new(-2, 1.0),
            Note::new(0, 0.2),
            Note::new(2, 0.6),
            Note::new(4, 0.5),
            Note::new(3, 0.5),
            Note::new(2, 0.5),
            Note::new(1, 0.5),
            Note::new(0, 0.5),
            Note::new(-1, 0.5),
            Note::new(-2, 0.5),
            Note::new(-3, 0.5),
            Note::new(-4, 0.5),
            Note::new(8, 0.7),
            Note::new(-4, 0.4),
            Note::new(3, 0.1),
        ], scale_notes.clone()),
        Chord::new(1.0, vec![
            Note::new(-2, 1.0),
            Note::new(4, 0.5),
            Note::new(3, 0.5),
            Note::new(2, 0.5),
            Note::new(1, 0.5),
            Note::new(0, 0.5),
            Note::new(-1, 0.5),
            Note::new(-2, 0.5),
            Note::new(-3, 0.5),
            Note::new(-4, 0.5),
            Note::new(3, 0.1),
        ], scale_notes.clone()),
        Chord::new(2.0, vec![
            Note::new(-1, 1.0),
            Note::new(2, 0.7),
            Note::new(4, 0.5),
            Note::new(3, 0.5),
            Note::new(2, 0.5),
            Note::new(1, 0.5),
            Note::new(0, 0.5),
            Note::new(-1, 0.5),
            Note::new(-2, 0.5),
            Note::new(-3, 0.5),
            Note::new(-4, 0.5),
            Note::new(-2, 0.4),
        ], scale_notes.clone()),
        Chord::new(3.0, vec![
            Note::new(-2, 1.0),
            Note::new(0, 0.2),
            Note::new(2, 0.6),
            Note::new(4, 0.5),
            Note::new(3, 0.5),
            Note::new(2, 0.5),
            Note::new(1, 0.5),
            Note::new(0, 0.5),
            Note::new(-1, 0.5),
            Note::new(-2, 0.5),
            Note::new(-3, 0.5),
            Note::new(-4, 0.5),
            Note::new(8, 0.7),
            Note::new(-4, 0.4),
            Note::new(3, 0.1),
        ], scale_notes.clone()),
    ]
}

pub fn create_drummer(
    name: String,
    handle: Handle<AudioSample>,
    volume: f64,
    drum_beat: HashMap<(u32, u32), Note>,
) -> Musician {
    Musician::new(
        name,
        Drummer::new(Sampler { handle, volume }, drum_beat),
    )
}

pub fn create_drummer_only(
    handle: Handle<AudioSample>,
    volume: f64,
    drum_beat: HashMap<(u32, u32), Note>,
) -> Drummer {
    Drummer::new(Sampler { handle, volume }, drum_beat)
}

pub fn create_arpeggiator(
    name: String,
    handle: Handle<AudioSample>,
    volume: f64,
) -> Musician {
    Musician::new(
        name,
        Arpeggiator::new(Sampler { handle, volume }),
    )
}

pub fn create_soloist(
    name: String,
    handle: Handle<AudioSample>,
    volume: f64,
    record_bars: u32,
) -> Musician {
    Musician::new(
        name,
        Soloist::new(Sampler { handle, volume }, record_bars),
    )
}

pub fn create_bassist(
    name: String,
    handle: Handle<AudioSample>,
    volume: f64,
) -> Musician {
    Musician::new(
        name,
        Bassist::new(Sampler { handle, volume }),
    )
}

/// Build a `Conductor` from the given chords. `chord_length_bars` is the
/// total bar length of the progression before it loops.
pub fn make_conductor(chords: Vec<Chord>, chord_length_bars: f32) -> Conductor {
    Conductor { chords, chord_length_bars }
}
