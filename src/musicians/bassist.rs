use bevy::prelude::Res;
use bevy_kira_audio::Audio;
use crate::clock::Beat;
use crate::musicians::{Chord, MusicPlayer, Note, Sampler};
use rand::seq::IteratorRandom;

pub struct Bassist {
    pub name: String,
}

impl Bassist {
    pub fn new(name: String) -> Self {
        Self {
            name,
        }
    }
}

impl MusicPlayer for Bassist {
    fn get_note(&mut self, beat: Beat, base_intensity: f32, chord: &Chord) -> Option<Note> {
        return if beat.beat == 0 {
            chord
                .chord_notes
                .iter()
                .filter(|n| n.strength >= 1.0)
                .collect::<Vec<&Note>>()
                .iter()
                .choose(&mut rand::thread_rng()).copied().copied()
        } else if beat.beat % 2 == 0 {
            chord
                .chord_notes
                .iter()
                .filter(|n| n.strength <= 0.5)
                .collect::<Vec<&Note>>()
                .iter()
                .choose(&mut rand::thread_rng()).copied().copied()
        } else if beat.beat % 4 == 0 {
            chord
                .chord_notes
                .iter()
                .filter(|n| n.strength <= 0.25)
                .collect::<Vec<&Note>>()
                .iter()
                .choose(&mut rand::thread_rng())
                .copied()
                .copied() // Nice.
        } else {
            None
        }
    }
}
