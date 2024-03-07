use bevy::utils::HashMap;
use crate::clock::Beat;
use crate::musicians::{Chord, MusicPlayer, Note};

pub struct Drummer {
    pub name: String,
    pub notes: HashMap<(u32, u32), Note>
}

impl MusicPlayer for Drummer {
    fn get_note(&mut self, beat: Beat, base_intensity: f32, _chord: &Chord) -> Option<Note> {
        if let Some(note) = self.notes.get(&(beat.beat, beat.sixteenth)) {
            return if note.strength <= base_intensity {
                Some(*note)
            } else {
                None
            }
        }
        None
    }
}

pub fn generate_kick_beat() -> HashMap<(u32, u32), Note> {
    // 0 1 2 3 0 1 2 3 0 1 2 3 0 1 2 3
    // 0       1       2       3
    HashMap::from([
        ((1, 0), Note {
            midi_note_diff: 0,
            strength: 0.25,
        }),
        ((3, 0), Note {
            midi_note_diff: 0,
            strength: 0.5,
        }),
        ((3, 2), Note {
            midi_note_diff: 0,
            strength: 0.7,
        }),
    ])
}

pub fn generate_snare_beat() -> HashMap<(u32, u32), Note> {
    HashMap::from([
        ((0, 0), Note {
            midi_note_diff: 0,
            strength: 0.5,
        }),
        ((2, 0), Note {
            midi_note_diff: 0,
            strength: 0.5,
        })
    ])
}

pub fn generate_hihat_beat() -> HashMap<(u32, u32), Note> {
    HashMap::from([
        ((0, 0), Note {
            midi_note_diff: 0,
            strength: 0.5,
        }),
        ((1, 0), Note {
            midi_note_diff: 0,
            strength: 0.5,
        }),
        ((2, 0), Note {
            midi_note_diff: 0,
            strength: 0.5,
        }),
        ((3, 0), Note {
            midi_note_diff: 0,
            strength: 0.5,
        }),
    ])
}
