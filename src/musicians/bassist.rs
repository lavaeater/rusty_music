use bevy::utils::HashMap;
use bevy::prelude::{Handle, Res};
use bevy::utils::petgraph::visit::Walker;
use bevy_kira_audio::{Audio, AudioControl, AudioSource};
use crate::clock::Beat;
use crate::musicians;
use crate::musicians::{Chord, MusicPlayer, Note, Sampler};
use rand::seq::IteratorRandom;


pub struct Bassist {
    pub name: String,
    pub sampler: Sampler,
}

impl Bassist {
    pub fn new(name: String, sampler: Sampler) -> Self {
        Self {
            name,
            sampler,
        }
    }
}


impl MusicPlayer for Bassist {
    fn signal(&self, audio: &Res<Audio>, beat: Beat, base_intensity: f32, chord: &Chord) {
        if beat.beat == 0 {
            let notes: Vec<&Note> = chord
                .chord_notes
                .iter()
                .filter(|n| n.strength >= 1.0).collect();
            if let Some(note) = notes.iter().choose(&mut rand::thread_rng()) {
                self.play(audio, beat, note.midi_note_diff, self.sampler.handle.clone_weak());
            }
        }

        if beat.beat % 2 == 0 {
            let notes: Vec<&Note> = chord
                .chord_notes
                .iter()
                .filter(|n| n.strength >= 1.0).collect();
            if let Some(note) = notes.iter().choose(&mut rand::thread_rng()) {
                self.play(audio, beat, note.midi_note_diff, self.sampler.handle.clone_weak());
            }
        }
    }
}
