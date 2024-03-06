use crate::clock::Beat;
use crate::musicians::{Chord, MusicPlayer, Note};
use rand::seq::IteratorRandom;

pub struct Soloist {
    pub name: String,
    pub record_bars: u32,
    pub repeats: u32,
    pub beats_per_bar: u32,
    pub recorded_melody: Vec<Note>,
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
    fn get_note(&mut self, beat: Beat, _base_intensity: f32, chord: &Chord) -> Option<Note> {
        let recording_index = beat.beat + beat.bar % self.record_bars * self.beats_per_bar;
        let repeat_end_bars = self.repeat_bar + (self.repeats * self.record_bars) as i32;

        return if (beat.bar as i32 ) < repeat_end_bars  {
            let note = self.recorded_melody[recording_index as usize];
            Some(note)
        } else {
            // if self.recorded_melody.len() > (self.record_bars * self.beats_per_bar) as usize {
            //     self.recorded_melody.clear();
            // }
            let note = if beat.beat == 0 {
                chord.scale_notes.iter().filter(|n| n.strength >= 1.0).choose(&mut rand::thread_rng()).unwrap()
            } else if beat.beat % self.beats_per_bar == 0 {
                chord.scale_notes.iter().filter(|n| n.strength >= 0.5).choose(&mut rand::thread_rng()).unwrap()
            } else if beat.beat & self.beats_per_bar / 2 == 0 {
                chord.scale_notes.iter().filter(|n| n.strength >= 0.25).choose(&mut rand::thread_rng()).unwrap()
            } else {
                chord.scale_notes.iter().filter(|n| n.strength >= 0.0).choose(&mut rand::thread_rng()).unwrap()
            };
            self.recorded_melody.push(note.clone());
            let last_recording_index = self.record_bars * self.beats_per_bar - 1;
            if recording_index > last_recording_index {
                self.repeat_bar = beat.bar as i32;
            }
            Some(*note)
        }
    }
}
