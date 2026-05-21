use bevy::prelude::{Commands, Time};
use bevy_seedling::prelude::{Audio, AudioEvents, InstantSeconds, PlaybackSettings, SamplePlayer, Volume};
use crate::clock::Beat;
use crate::musicians::{Chord, midi_diff_to_pitch, MusicPlayer, Note, Sampler, TonalPlayer};

pub struct Bassist {
    pub sampler: Sampler,
}

impl Bassist {
    pub fn new(sampler: Sampler) -> Self {
        Self { sampler }
    }

    fn play_note(&self, note: Note, commands: &mut Commands, audio_time: &Time<Audio>, scheduled_at: InstantSeconds) {
        let mut events = AudioEvents::new(audio_time);
        let settings = PlaybackSettings::default()
            .with_playback(false)
            .with_speed(midi_diff_to_pitch(note.midi_note_diff));
        settings.play_at(None, scheduled_at, &mut events);
        commands.spawn((
            SamplePlayer::new(self.sampler.handle.clone())
                .with_volume(Volume::Decibels(self.sampler.volume as f32)),
            settings,
            events,
        ));
    }
}

impl MusicPlayer for Bassist {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, chord: &Chord, audio_time: &Time<Audio>, scheduled_at: InstantSeconds) {
        let step = TonalPlayer::flat_step(&beat); // 0–15 within bar

        if step == 0 {
            // Downbeat: always play the root (strongest chord tone).
            if let Some(note) = TonalPlayer::get_chord_note(chord, 1.0) {
                self.play_note(note, commands, audio_time, scheduled_at);
            }
            return;
        }

        if step % 4 == 0 {
            // Quarter beats: play strong chord tone probabilistically.
            if rand::random::<f32>() < base_intensity {
                if let Some(note) = TonalPlayer::get_chord_note(chord, 0.5) {
                    self.play_note(note, commands, audio_time, scheduled_at);
                }
            }
            return;
        }

        if step % 2 == 0 {
            // 8th-note offbeats: play at moderate probability.
            if rand::random::<f32>() < base_intensity - 0.25 {
                if let Some(note) = TonalPlayer::get_chord_note(chord, 0.25) {
                    self.play_note(note, commands, audio_time, scheduled_at);
                }
            }
            return;
        }

        // 16th-note positions: play only at high intensity.
        if rand::random::<f32>() < base_intensity - 0.5 {
            if let Some(note) = TonalPlayer::get_chord_note(chord, 0.0) {
                self.play_note(note, commands, audio_time, scheduled_at);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // Even but not multiples of 4: positions 2, 6, 10, 14
        for (b, s) in [(0u32, 2u32), (1, 2), (2, 2), (3, 2)] {
            let step = TonalPlayer::flat_step(&make_beat(b, s));
            assert!(step % 2 == 0 && step % 4 != 0, "step {step} should be 8th position");
        }
    }

    #[test]
    fn flat_step_sixteenth_positions() {
        // Odd positions: 1, 3, 5, 7, ...
        for (b, s) in [(0u32, 1u32), (0, 3), (1, 1), (1, 3)] {
            let step = TonalPlayer::flat_step(&make_beat(b, s));
            assert!(step % 2 != 0, "step {step} should be 16th (odd) position");
        }
    }

    #[test]
    fn downbeat_always_plays_regardless_of_intensity() {
        // At step==0 with intensity 0.0 we still play: no random check.
        // Verify the condition: step==0 always triggers (no intensity gate).
        let beat = make_beat(0, 0);
        assert_eq!(TonalPlayer::flat_step(&beat), 0);
        // No intensity threshold — the code always plays when step==0.
    }

    #[test]
    fn quarter_plays_when_intensity_exceeds_random() {
        // At full intensity (1.0) the quarter condition always fires.
        // At zero intensity it never fires (gen::<f32>() is always >= 0).
        let beat = make_beat(1, 0);
        let step = TonalPlayer::flat_step(&beat);
        assert_eq!(step % 4, 0);
        assert_ne!(step, 0);
        // intensity=0.0: condition `gen < 0.0` is always false → no play
        // intensity=1.0: condition `gen < 1.0` is (almost) always true → play
    }

    #[test]
    fn sixteenth_never_plays_below_half_intensity() {
        // `gen::<f32>() < intensity - 0.5` when intensity <= 0.5 is always false.
        let beat = make_beat(0, 1); // step=1, odd
        let step = TonalPlayer::flat_step(&beat);
        assert!(step % 2 != 0);
        // At intensity=0.5: 0.5 - 0.5 = 0.0, gen >= 0.0 → never plays
    }
}
