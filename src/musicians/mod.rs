pub(crate) mod drummer;
pub(crate) mod conductor;
pub(crate) mod bassist;
pub(crate) mod soloist;

use bevy::prelude::{Component};
use bevy_kira_audio::{AudioSource};
use bevy::asset::Handle;
use crate::clock::Beat;

pub trait MusicPlayer: Send + Sync {
    fn get_note(&mut self, beat: Beat, base_intensity: f32, chord: &Chord) -> Option<Note>;
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

pub enum MusicianType {
    Drums,
    Bass,
    Solo,
}

#[derive(Component)]
pub struct Musician {
    pub name: String,
    pub sampler: Sampler,
    pub player: Box<dyn MusicPlayer>,
    pub musician_type: MusicianType,
}

impl Musician {
    pub fn new(name: String, sampler: Sampler, player: impl MusicPlayer + 'static, musician_type: MusicianType) -> Self {
        Self {
            name,
            sampler,
            player: Box::new(player),
            musician_type,
        }
    }
}
