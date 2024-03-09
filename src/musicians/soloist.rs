use bevy::prelude::Res;
use bevy_kira_audio::{Audio, AudioControl};
use bevy_kira_audio::prelude::Volume;
use rand::Rng;
use crate::clock::Beat;
use crate::musicians::{Chord, midi_diff_to_pitch, MusicPlayer, Note, Sampler};
use rand::seq::IteratorRandom;

pub struct Soloist {
    pub record_bars: u32,
    pub repeats: u32,
    pub beats_per_bar: u32,
    pub recorded_melody: Vec<Option<Note>>,
    pub repeat_bar: i32,
    sampler: Sampler
}

impl Soloist {
    pub fn new(sampler: Sampler, record_bars: u32, beats_per_bar: u32, repeats: u32) -> Self {
        Self {
            record_bars,
            repeats,
            beats_per_bar,
            recorded_melody: Vec::with_capacity((record_bars * beats_per_bar) as usize),
            repeat_bar: -999,
            sampler
        }
    }
}

impl MusicPlayer for Soloist {
    fn play(&mut self, beat: Beat, audio: &Res<Audio>, base_intensity: f32, chord: &Chord) {
        let recording_index = beat.sixteenth_count % (self.record_bars * self.beats_per_bar);
        let repeat_end_bars = self.repeat_bar + (self.repeats * self.record_bars) as i32;

        println!("recording_index: {}, repeat_end_bars: {}, recording_lenght: {}", recording_index, repeat_end_bars, self.recorded_melody.len());

        if let Some(note) = if (beat.bar_count as i32) < repeat_end_bars {
            self.recorded_melody[recording_index as usize]
        } else {
            let max_recording_size = self.record_bars * self.beats_per_bar;
            if self.recorded_melody.len() > max_recording_size as usize {
                println!("clearing recorded melody");
                self.recorded_melody.clear();
            }
            let note = if beat.beat == 0 &&  beat.sixteenth == 0 {
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
            println!("last_recording_index: {}, length: {}", max_recording_size, self.recorded_melody.len());
            if self.recorded_melody.len() > max_recording_size as usize {
                self.repeat_bar = beat.bar_count as i32;
            }
            note.copied()
        } {
            audio.play(self.sampler.handle.clone_weak())
                .with_volume(Volume::from(self.sampler.volume))
                .with_playback_rate(midi_diff_to_pitch(note.midi_note_diff));
        }
    }
}
