pub mod drummer;
pub mod conductor;
pub mod bassist;
pub mod soloist;

use std::cmp::Ordering;
use bevy::prelude::{Component, Res};
use bevy_kira_audio::{Audio, AudioSource};
use bevy::asset::Handle;
use crate::clock::Beat;


pub fn midi_diff_to_pitch_what(midi_diff: i32) -> f64 {
    let f = 2.0f64.powf(midi_diff as f64 / 12.0);
    f
}

pub fn midi_diff_to_pitch(midi_diff: i32) -> f64 {
    let min_pitch = -12;
    let max_pitch = 12;
    match midi_diff.cmp(&0) {
        Ordering::Less => {
            if midi_diff < min_pitch {
                0.5
            } else {
                midi_diff_to_pitch_what(midi_diff)
            }
        }
        Ordering::Equal => {
            1.0
        }
        Ordering::Greater => {
            if midi_diff > max_pitch {
                2.0
            } else {
                midi_diff_to_pitch_what(midi_diff)
            }
        }
    }
}

pub trait MusicPlayer: Send + Sync {
    fn play(&mut self, beat: Beat, audio: &Res<Audio>, base_intensity: f32, chord: &Chord);
}

pub struct Sampler {
    pub handle: Handle<AudioSource>,
    pub volume: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Note {
    pub midi_note_diff: i32,
    pub strength: f32,
}

impl Note {
    pub fn new(midi_note_diff: i32, strength: f32) -> Self {
        Self {
            midi_note_diff,
            strength,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chord {
    pub bar: u32,
    pub chord_notes: Vec<Note>,
    pub scale_notes: Vec<Note>,
}

impl Chord {
    pub fn new(bar: u32, chord_notes: Vec<Note>, scale_notes: Vec<Note>) -> Self {
        Self {
            bar,
            chord_notes,
            scale_notes,
        }
    }
}

#[derive(Component)]
pub struct Musician {
    pub name: String,
    pub player: Box<dyn MusicPlayer>
}

impl Musician {
    pub fn new(name: String, player: impl MusicPlayer + 'static) -> Self {
        Self {
            name,
            player: Box::new(player),
        }
    }
}
