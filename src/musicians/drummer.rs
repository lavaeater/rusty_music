use bevy::utils::HashMap;
use crate::clock::Beat;
use crate::musicians::{Chord, MusicPlayer, Note};

pub struct Drummer {
    pub name: String,
    pub notes: HashMap<u32, Note>
}

impl MusicPlayer for Drummer {
    fn get_note(&mut self, beat: Beat, base_intensity: f32, _chord: &Chord) -> Option<Note> {
        if let Some(note) = self.notes.get(&beat.beat) {
            return if note.strength <= base_intensity {
                Some(*note)
            } else {
                None
            }
        }
        None
    }
}
