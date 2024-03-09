use bevy::prelude::Res;
use bevy_kira_audio::{Audio, AudioControl};
use bevy_kira_audio::prelude::Volume;
use rand::Rng;
use crate::clock::Beat;
use crate::musicians::{Chord, midi_diff_to_pitch, MusicPlayer, Note, Sampler};
use rand::seq::IteratorRandom;

pub struct Bassist {
    pub sampler: Sampler
}

impl Bassist {
    pub fn new(sampler: Sampler) -> Self {
        Self {
            sampler
        }
    }
}

impl MusicPlayer for Bassist {
    fn play(&mut self, beat: Beat, audio: &Res<Audio>, base_intensity: f32, chord: &Chord) {
        // Strong notes on the downbeat (0, first sixteenth)
        if let Some(note) = if beat.beat == 0 && beat.sixteenth == 0 {
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
        } {
            audio.play(self.sampler.handle.clone_weak())
                .with_volume(Volume::from(self.sampler.volume))
                .with_playback_rate(midi_diff_to_pitch(note.midi_note_diff));
        }
    }
}
