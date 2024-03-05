use bevy::utils::HashMap;
use bevy::prelude::Res;
use bevy_kira_audio::Audio;
use crate::clock::Beat;
use crate::musicians::{Chord, MusicPlayer, Note, Sampler};

pub struct Drummer {
    pub name: String,
    pub sampler: Sampler,
    pub notes: HashMap<u32, Note>
}

impl MusicPlayer for Drummer {
    fn signal(&self, audio: &Res<Audio>, beat: Beat, base_intensity: f32, _chord: &Chord) {
        if let Some(note) = self.notes.get(&beat.beat) {
            if note.strength <= base_intensity {
                self.play(audio, beat, note.midi_note_diff, self.sampler.handle.clone_weak());
            }
        }
    }
}
