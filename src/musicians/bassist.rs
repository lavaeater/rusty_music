use bevy::prelude::Res;
use bevy_kira_audio::Audio;
use rand::Rng;
use crate::clock::Beat;
use crate::musicians::{Chord, MusicPlayer, Note};
use rand::seq::IteratorRandom;

pub struct Bassist {
    pub name: String,
    pub previous_beat: u32,
}

impl Bassist {
    pub fn new(name: String) -> Self {
        Self {
            name,
            previous_beat: 220,
        }
    }
}

impl MusicPlayer for Bassist {
    fn play(&mut self, beat: Beat, audio: &Res<Audio>, base_intensity: f32, chord: &Chord) -> Option<Note> {
        // Strong notes on the downbeat (0, first sixteenth)
        return if beat.beat == 0 && beat.sixteenth == 0 {
            chord
                .chord_notes
                .iter()
                .filter(|n| n.strength >= 1.0)
                .collect::<Vec<&Note>>()
                .iter()
                .choose(&mut rand::thread_rng()).copied().copied()
        } else if beat.sixteenth == 3 && rand::thread_rng().gen_bool(0.5) {
            chord
                .chord_notes
                .iter()
                .filter(|n| n.strength >= 1.0)
                .collect::<Vec<&Note>>()
                .iter()
                .choose(&mut rand::thread_rng()).copied().copied()
        } else if (beat.sixteenth == 2 || beat.sixteenth == 0) && rand::thread_rng().gen_range(0.0..=1.0) < (base_intensity - 0.25) {
            chord
                .chord_notes
                .iter()
                .filter(|n| n.strength >= 1.0)
                .collect::<Vec<&Note>>()
                .iter()
                .choose(&mut rand::thread_rng()).copied().copied()
        } else if rand::thread_rng().gen_range(0.0..=1.0) < (base_intensity - 0.5) {
            chord
                .chord_notes
                .iter()
                .filter(|n| n.strength >= 1.0)
                .collect::<Vec<&Note>>()
                .iter()
                .choose(&mut rand::thread_rng()).copied().copied()
        } else {
            None
        };
    }
}
