use bevy::prelude::Res;
use bevy_kira_audio::{Audio, AudioControl};
use bevy_kira_audio::prelude::Volume;
use rand::Rng;
use crate::clock::Beat;
use crate::musicians::{Chord, midi_diff_to_pitch, MusicPlayer, Sampler};

pub enum ArpeggioMode {
    Up,
    Down,
    Random,
}

pub struct Arpeggiator {
    pub sampler: Sampler,
    pub arpeggio_mode: ArpeggioMode,
    pub some_index: u32,
    pub next_sixteenth: u32,
}

impl Arpeggiator {
    pub fn new(sampler: Sampler) -> Self {
        Self {
            sampler,
            arpeggio_mode: ArpeggioMode::Up,
            some_index: 0,
            next_sixteenth: 0,
        }
    }
}

impl MusicPlayer for Arpeggiator {
    fn play(&mut self, beat: Beat, audio: &Res<Audio>, base_intensity: f32, chord: &Chord) {
        let step_size = if base_intensity < 0.4 {
            4
        } else if base_intensity > 0.7 {
            8
        } else {
            16
        };

        if beat.sixteenth_count > self.next_sixteenth {
            self.next_sixteenth = beat.sixteenth_count + step_size;
        } else {
            return
        }

        let chord_note_length = chord.chord_notes.len() as u32;
        let note_index = match self.arpeggio_mode {
            ArpeggioMode::Up => {
                self.some_index = (self.some_index + 1) % chord_note_length;
                self.some_index
            }
            ArpeggioMode::Down => {
                self.some_index = (self.some_index + 1) % chord_note_length;
                chord_note_length - (self.some_index + 1)
            }
            ArpeggioMode::Random => {
                rand::thread_rng().gen_range(0..chord_note_length)
            }
        };

        if let Some(note) = chord.chord_notes.get(note_index as usize) {
            audio.play(self.sampler.handle.clone_weak())
                .with_volume(Volume::from(self.sampler.volume))
                .with_playback_rate(midi_diff_to_pitch(note.midi_note_diff));
        }
    }
}
