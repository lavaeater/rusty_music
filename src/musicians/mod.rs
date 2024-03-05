pub(crate) mod drummer;
pub(crate) mod conductor;
pub(crate) mod bassist;

use bevy::prelude::{Res};
use bevy_kira_audio::{Audio, AudioControl, AudioSource};
use bevy::asset::Handle;
use crate::clock::Beat;
use crate::musicians;

pub trait MusicPlayer: Send + Sync {
    fn signal(&self, audio: &Res<Audio>, beat: Beat, global_intensity: f32, chord: &Chord);
    fn play(&self, audio: &Res<Audio>, _beat: Beat, midi_note_diff: i32, sampler: Handle<AudioSource>) {
        audio.play(sampler)
            .with_playback_rate(musicians::midi_diff_to_pitch(midi_note_diff));
    }
}

fn midi_diff_to_pitch(midi_diff: i32) -> f64 {
    let min_pitch = -12;
    let max_pitch = 12;
    if midi_diff < 0 {
        if midi_diff < min_pitch {
            0.5
        } else {
            midi_diff_to_pitch_what(midi_diff)
        }
    } else if midi_diff > 0 {
        if midi_diff > max_pitch {
            2.0
        } else {
            midi_diff_to_pitch_what(midi_diff)
        }
    } else {
        1.0
    }
}

fn midi_diff_to_pitch_what(midi_diff: i32) -> f64 {
    let f = 2.0f64.powf(midi_diff as f64 / 12.0);
    f
}

pub struct Sampler {
    pub handle: Handle<AudioSource>,
}


#[derive(Debug, Clone, Copy, PartialEq)]
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
