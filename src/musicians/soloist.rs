use rand::Rng;
use crate::clock::Beat;
use crate::musicians::{Chord, MusicPlayer, Note};
use rand::seq::IteratorRandom;

pub struct Soloist {
    pub name: String,
    pub record_bars: u32,
    pub repeats: u32,
    pub beats_per_bar: u32,
    pub recorded_melody: Vec<Option<Note>>,
    pub repeat_bar: i32,
}

impl Soloist {
    pub fn new(name: String, record_bars: u32, beats_per_bar: u32, repeats: u32) -> Self {
        Self {
            name,
            record_bars,
            repeats,
            beats_per_bar,
            recorded_melody: Vec::with_capacity((record_bars * beats_per_bar) as usize),
            repeat_bar: -999,
        }
    }
}

impl MusicPlayer for Soloist {
    fn get_note(&mut self, beat: Beat, base_intensity: f32, chord: &Chord) -> Option<Note> {
        let recording_index = beat.sixteenth + beat.bar % self.record_bars * self.beats_per_bar;
        let repeat_end_bars = self.repeat_bar + (self.repeats * self.record_bars) as i32;

        return if (beat.bar as i32) < repeat_end_bars {
            self.recorded_melody[recording_index as usize]
        } else {
            // if self.recorded_melody.len() > (self.record_bars * self.beats_per_bar) as usize {
            //     self.recorded_melody.clear();
            // }
            let note = if beat.beat == 0 {
                chord.scale_notes.iter().filter(|n| n.strength >= 1.0).choose(&mut rand::thread_rng())
            } else if beat.sixteenth == 3 && rand::thread_rng().gen_bool(0.5) {
                chord.scale_notes.iter().filter(|n| n.strength >= 0.5).choose(&mut rand::thread_rng())
            } else if (beat.sixteenth == 2 || beat.sixteenth == 0) && rand::thread_rng().gen_range(0.0..=1.0) < (base_intensity - 0.25) {
                chord.scale_notes.iter().filter(|n| n.strength >= 0.25).choose(&mut rand::thread_rng())
            } else if rand::thread_rng().gen_range(0.0..=1.0) < (base_intensity - 0.5) {
                chord.scale_notes.iter().filter(|n| n.strength >= 0.0).choose(&mut rand::thread_rng())
            } else {
                None
            };
            self.recorded_melody.push(note.copied());
            let last_recording_index = self.record_bars * self.beats_per_bar - 1;
            if recording_index > last_recording_index {
                self.repeat_bar = beat.bar as i32;
            }
            note.copied()
        };
    }
}
