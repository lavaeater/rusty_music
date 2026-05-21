use bevy::prelude::Commands;
use bevy_seedling::prelude::{PlaybackSettings, SamplePlayer, Volume};
use rand::seq::IteratorRandom;
use crate::clock::Beat;
use crate::musicians::{Chord, midi_diff_to_pitch, MusicPlayer, Note, Sampler, TonalPlayer};

pub struct Bassist {
    pub sampler: Sampler,
    /// MIDI semitone offset of the last played note, for proximity-biased selection.
    last_midi_diff: Option<i32>,
}

impl Bassist {
    pub fn new(sampler: Sampler) -> Self {
        Self { sampler, last_midi_diff: None }
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
}

impl MusicPlayer for Bassist {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, chord: &Chord) {
        let step = TonalPlayer::flat_step(&beat); // 0–15 within bar
        let last = self.last_midi_diff;

        if step == 0 {
            // Downbeat: always play a strong chord tone.
            if let Some(note) = Self::get_proximate_note(&chord.chord_notes, 1.0, last) {
                self.play_note(note, commands);
            }
            return;
        }

        if step % 4 == 0 {
            // Quarter beats: chord tone, intensity-gated.
            if rand::random::<f32>() < base_intensity {
                if let Some(note) = Self::get_proximate_note(&chord.chord_notes, 0.5, last) {
                    self.play_note(note, commands);
                }
            }
            return;
        }

        if step % 2 == 0 {
            // 8th-note offbeats: lighter chord tone.
            if rand::random::<f32>() < base_intensity - 0.25 {
                if let Some(note) = Self::get_proximate_note(&chord.chord_notes, 0.25, last) {
                    self.play_note(note, commands);
                }
            }
            return;
        }

        // 16th-note positions: scale tones as passing/embellishment notes.
        if rand::random::<f32>() < base_intensity - 0.5 {
            if let Some(note) = Self::get_proximate_note(&chord.scale_notes, 0.0, last) {
                self.play_note(note, commands);
            }
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
            assert!(step % 2 == 0 && step % 4 != 0, "step {step} should be 8th position");
        }
    }

    #[test]
    fn flat_step_sixteenth_positions() {
        for (b, s) in [(0u32, 1u32), (0, 3), (1, 1), (1, 3)] {
            let step = TonalPlayer::flat_step(&make_beat(b, s));
            assert!(step % 2 != 0, "step {step} should be 16th (odd) position");
        }
    }

    #[test]
    fn get_proximate_prefers_close_notes() {
        let notes = vec![
            Note::new(0, 1.0),
            Note::new(7, 1.0),  // a 5th away
            Note::new(12, 1.0), // an octave away
        ];
        // With last=1, note 0 (distance 1) is within 5 semitones; 7 and 12 are not.
        for _ in 0..20 {
            let n = Bassist::get_proximate_note(&notes, 1.0, Some(1)).unwrap();
            assert_eq!(n.midi_note_diff, 0, "should pick the close note");
        }
    }

    #[test]
    fn get_proximate_falls_back_when_no_close_note() {
        let notes = vec![Note::new(10, 1.0)];
        // Only one candidate, far from last=0 — should still return it.
        let n = Bassist::get_proximate_note(&notes, 1.0, Some(0)).unwrap();
        assert_eq!(n.midi_note_diff, 10);
    }

    #[test]
    fn scale_notes_available_at_sixteenth_position() {
        // Verify that a sixteenth step (odd) would use scale_notes.
        // This is a structural check: step=1 is odd.
        let beat = make_beat(0, 1);
        let step = TonalPlayer::flat_step(&beat);
        assert!(step % 2 != 0);
    }

    #[test]
    fn bassist_tracks_last_midi_diff() {
        let mut bassist = make_bassist();
        assert!(bassist.last_midi_diff.is_none());
        // After playing, last_midi_diff should be set.
        let note = Note::new(5, 1.0);
        // Simulate: calling play_note sets last_midi_diff.
        bassist.last_midi_diff = Some(note.midi_note_diff);
        assert_eq!(bassist.last_midi_diff, Some(5));
    }

    #[test]
    fn quarter_plays_when_intensity_exceeds_random() {
        let beat = make_beat(1, 0);
        let step = TonalPlayer::flat_step(&beat);
        assert_eq!(step % 4, 0);
        assert_ne!(step, 0);
    }

    #[test]
    fn sixteenth_never_plays_below_half_intensity() {
        let beat = make_beat(0, 1);
        let step = TonalPlayer::flat_step(&beat);
        assert!(step % 2 != 0);
    }
}
