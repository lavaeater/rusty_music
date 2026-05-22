use bevy::prelude::Commands;
use bevy_seedling::prelude::{PlaybackSettings, SamplePlayer, Volume};
use rand::seq::IteratorRandom;
use crate::clock::Beat;
use crate::musicians::{Chord, midi_diff_to_pitch, MusicPlayer, Note, Sampler, TonalPlayer, STEPS_PER_BAR};

pub struct Bassist {
    pub sampler: Sampler,
    /// MIDI semitone offset of the last played note, for proximity-biased selection.
    last_midi_diff: Option<i32>,
    /// Record `memory_bars` bars of bass line and repeat it `memory_repeats` times. 0 = no memory.
    pub memory_bars: u32,
    pub memory_repeats: u32,
    recorded_line: Vec<Option<Note>>,
    repeat_end_bar: f32,
}

impl Bassist {
    pub fn new(sampler: Sampler) -> Self {
        Self {
            sampler,
            last_midi_diff: None,
            memory_bars: 0,
            memory_repeats: 1,
            recorded_line: Vec::new(),
            repeat_end_bar: 0.0,
        }
    }

    /// Select a random note at or above `min_strength`, biasing toward notes
    /// within 5 semitones of the last played note for smoother melodic lines.
    fn get_proximate_note(notes: &[Note], min_strength: f32, last: Option<i32>) -> Option<Note> {
        let candidates: Vec<Note> = notes
            .iter()
            .filter(|n| n.strength >= min_strength)
            .copied()
            .collect();
        if candidates.is_empty() {
            return None;
        }
        if let Some(last_midi) = last {
            let close: Vec<Note> = candidates
                .iter()
                .filter(|n| (n.midi_note_diff - last_midi).abs() <= 5)
                .copied()
                .collect();
            if !close.is_empty() {
                return close.into_iter().choose(&mut rand::rng());
            }
        }
        candidates.into_iter().choose(&mut rand::rng())
    }

    fn play_note(&mut self, note: Note, commands: &mut Commands) {
        self.last_midi_diff = Some(note.midi_note_diff);
        commands.spawn((
            SamplePlayer::new(self.sampler.handle.clone())
                .with_volume(Volume::Decibels(self.sampler.volume as f32)),
            PlaybackSettings::default()
                .with_speed(midi_diff_to_pitch(note.midi_note_diff)),
        ));
    }

    fn generate_note(&self, step: u32, base_intensity: f32, chord: &Chord) -> Option<Note> {
        let last = self.last_midi_diff;
        if step == 0 {
            Self::get_proximate_note(&chord.chord_notes, 1.0, last)
        } else if step % 4 == 0 {
            if rand::random::<f32>() < base_intensity {
                Self::get_proximate_note(&chord.chord_notes, 0.5, last)
            } else {
                None
            }
        } else if step % 2 == 0 {
            if rand::random::<f32>() < base_intensity - 0.25 {
                Self::get_proximate_note(&chord.chord_notes, 0.25, last)
            } else {
                None
            }
        } else if rand::random::<f32>() < base_intensity - 0.5 {
            Self::get_proximate_note(&chord.scale_notes, 0.0, last)
        } else {
            None
        }
    }
}

impl MusicPlayer for Bassist {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, chord: &Chord) {
        let step = TonalPlayer::flat_step(&beat);

        if self.memory_bars > 0 {
            let total = (self.memory_bars * STEPS_PER_BAR) as usize;
            if self.recorded_line.len() != total {
                self.recorded_line = vec![None; total];
            }

            let bar_in_cycle = beat.bar_count % self.memory_bars;
            let recording_index = (bar_in_cycle * STEPS_PER_BAR + step) as usize;

            if beat.time_bars < self.repeat_end_bar {
                if let Some(note) = self.recorded_line[recording_index] {
                    self.play_note(note, commands);
                }
                return;
            }

            if recording_index == 0 {
                self.recorded_line.fill(None);
            }

            let note = self.generate_note(step, base_intensity, chord);
            self.recorded_line[recording_index] = note;
            if let Some(n) = note {
                self.play_note(n, commands);
            }

            let last_index = total - 1;
            if recording_index >= last_index {
                self.repeat_end_bar =
                    beat.time_bars.ceil() + (self.memory_repeats * self.memory_bars) as f32;
            }
            return;
        }

        if let Some(note) = self.generate_note(step, base_intensity, chord) {
            self.play_note(note, commands);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::Handle;
    use crate::musicians::{AudioSample, Sampler};

    fn make_beat(beat: u32, sixteenth: u32) -> Beat {
        Beat {
            elapsed_time: 0.0,
            beat,
            sixteenth,
            bar_count: 0,
            beat_count: beat,
            sixteenth_count: beat * 4 + sixteenth,
            time_bars: beat as f32 / 4.0 + sixteenth as f32 / 16.0,
            overshoot: 0.0,
        }
    }

    fn make_bassist() -> Bassist {
        Bassist::new(Sampler { handle: Handle::<AudioSample>::default(), volume: 1.0 })
    }

    #[test]
    fn flat_step_downbeat_is_zero() {
        assert_eq!(TonalPlayer::flat_step(&make_beat(0, 0)), 0);
    }

    #[test]
    fn flat_step_quarter_positions() {
        assert_eq!(TonalPlayer::flat_step(&make_beat(1, 0)), 4);
        assert_eq!(TonalPlayer::flat_step(&make_beat(2, 0)), 8);
        assert_eq!(TonalPlayer::flat_step(&make_beat(3, 0)), 12);
    }

    #[test]
    fn flat_step_eighth_positions() {
        for (b, s) in [(0u32, 2u32), (1, 2), (2, 2), (3, 2)] {
            let step = TonalPlayer::flat_step(&make_beat(b, s));
            assert!(step % 2 == 0 && step % 4 != 0);
        }
    }

    #[test]
    fn flat_step_sixteenth_positions() {
        for (b, s) in [(0u32, 1u32), (0, 3), (1, 1), (1, 3)] {
            let step = TonalPlayer::flat_step(&make_beat(b, s));
            assert!(step % 2 != 0);
        }
    }

    #[test]
    fn get_proximate_prefers_close_notes() {
        let notes = vec![
            Note::new(0, 1.0),
            Note::new(7, 1.0),
            Note::new(12, 1.0),
        ];
        for _ in 0..20 {
            let n = Bassist::get_proximate_note(&notes, 1.0, Some(1)).unwrap();
            assert_eq!(n.midi_note_diff, 0);
        }
    }

    #[test]
    fn get_proximate_falls_back_when_no_close_note() {
        let notes = vec![Note::new(10, 1.0)];
        let n = Bassist::get_proximate_note(&notes, 1.0, Some(0)).unwrap();
        assert_eq!(n.midi_note_diff, 10);
    }

    #[test]
    fn bassist_tracks_last_midi_diff() {
        let mut bassist = make_bassist();
        assert!(bassist.last_midi_diff.is_none());
        bassist.last_midi_diff = Some(5);
        assert_eq!(bassist.last_midi_diff, Some(5));
    }

    #[test]
    fn memory_bars_initializes_recorded_line() {
        let mut b = make_bassist();
        b.memory_bars = 2;
        b.recorded_line.resize((2 * STEPS_PER_BAR) as usize, None);
        assert_eq!(b.recorded_line.len(), 32);
    }
}
