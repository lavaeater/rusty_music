use bevy::prelude::Commands;
use bevy_seedling::prelude::{PlaybackSettings, SamplePlayer, Volume};
use crate::clock::Beat;
use crate::musicians::{Chord, midi_diff_to_pitch, MusicPlayer, Note, Sampler, TonalPlayer, STEPS_PER_BAR};

pub struct Soloist {
    /// Number of bars to record before repeating.
    pub record_bars: u32,
    /// Number of times to repeat the recording before re-generating.
    pub repeats: u32,
    /// Recorded notes: `None` = rest. Size = `record_bars * STEPS_PER_BAR`.
    recorded_melody: Vec<Option<Note>>,
    /// `time_bars` at which the current repeat window ends.
    /// Starts at 0.0 so the first update immediately enters recording mode.
    repeat_end_bar: f32,
    sampler: Sampler,
}

impl Soloist {
    pub fn new(sampler: Sampler, record_bars: u32, repeats: u32) -> Self {
        let total = (record_bars * STEPS_PER_BAR) as usize;
        Self {
            record_bars,
            repeats,
            recorded_melody: vec![None; total],
            repeat_end_bar: 0.0,
            sampler,
        }
    }

    fn play_note(&self, note: Note, commands: &mut Commands) {
        commands.spawn((
            SamplePlayer::new(self.sampler.handle.clone())
                .with_volume(Volume::Decibels(self.sampler.volume as f32)),
            PlaybackSettings::default()
                .with_speed(midi_diff_to_pitch(note.midi_note_diff)),
        ));
    }

    /// Recording index: 0 .. record_bars * STEPS_PER_BAR.
    fn recording_index(beat: &Beat, record_bars: u32) -> u32 {
        let within_bar = TonalPlayer::flat_step(beat); // 0–15
        let bar_in_cycle = beat.bar_count % record_bars;
        bar_in_cycle * STEPS_PER_BAR + within_bar
    }

    fn generate_note(step: u32, chord: &Chord, intensity: f32) -> Option<Note> {
        if step == 0 {
            // Downbeat of every bar: always play a strong scale tone.
            TonalPlayer::get_scale_note(chord, 1.0)
        } else if step % 4 == 0 {
            // Quarter beats: play with probability ~ intensity.
            if rand::random::<f32>() < intensity {
                TonalPlayer::get_scale_note(chord, 0.5)
            } else {
                None
            }
        } else if step % 2 == 0 {
            // 8th-note positions.
            if rand::random::<f32>() < intensity - 0.25 {
                TonalPlayer::get_scale_note(chord, 0.25)
            } else {
                None
            }
        } else {
            // 16th-note positions: only at high intensity.
            if rand::random::<f32>() < intensity - 0.5 {
                TonalPlayer::get_scale_note(chord, 0.0)
            } else {
                None
            }
        }
    }
}

impl MusicPlayer for Soloist {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, chord: &Chord) {
        let recording_index = Self::recording_index(&beat, self.record_bars) as usize;

        if beat.time_bars < self.repeat_end_bar {
            // Playback mode: replay the recorded melody.
            if let Some(note) = self.recorded_melody[recording_index] {
                self.play_note(note, commands);
            }
        } else {
            // Recording mode: generate a new note and record it.
            // Reset the buffer at the start of a new recording cycle.
            if recording_index == 0 {
                self.recorded_melody.fill(None);
            }

            let step = TonalPlayer::flat_step(&beat); // 0–15 within bar
            let note = Self::generate_note(step, chord, base_intensity);
            self.recorded_melody[recording_index] = note;

            if let Some(n) = note {
                self.play_note(n, commands);
            }

            // Once the last step of the recording is filled, schedule the repeat.
            let last_index = (self.record_bars * STEPS_PER_BAR - 1) as usize;
            if recording_index >= last_index {
                self.repeat_end_bar =
                    beat.time_bars.ceil() + (self.repeats * self.record_bars) as f32;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_beat(beat: u32, sixteenth: u32, bar: u32) -> Beat {
        Beat {
            elapsed_time: 0.0,
            beat,
            sixteenth,
            bar_count: bar,
            beat_count: bar * 4 + beat,
            sixteenth_count: bar * 16 + beat * 4 + sixteenth,
            time_bars: bar as f32 + beat as f32 / 4.0 + sixteenth as f32 / 16.0,
        }
    }

    #[test]
    fn recording_index_downbeat_bar0() {
        let beat = make_beat(0, 0, 0);
        assert_eq!(Soloist::recording_index(&beat, 2), 0);
    }

    #[test]
    fn recording_index_last_step_bar0() {
        let beat = make_beat(3, 3, 0);
        // within_bar=15, bar_in_cycle=0 → index=15
        assert_eq!(Soloist::recording_index(&beat, 2), 15);
    }

    #[test]
    fn recording_index_bar1() {
        let beat = make_beat(0, 0, 1);
        // within_bar=0, bar_in_cycle=1 → 1*16+0=16
        assert_eq!(Soloist::recording_index(&beat, 2), 16);
    }

    #[test]
    fn recording_index_last_step_bar1() {
        let beat = make_beat(3, 3, 1);
        // within_bar=15, bar_in_cycle=1 → 1*16+15=31
        assert_eq!(Soloist::recording_index(&beat, 2), 31);
    }

    #[test]
    fn recording_index_wraps_at_record_bars() {
        // bar 2 wraps to bar_in_cycle=0 for record_bars=2
        let beat_bar2 = make_beat(0, 0, 2);
        let beat_bar0 = make_beat(0, 0, 0);
        assert_eq!(
            Soloist::recording_index(&beat_bar2, 2),
            Soloist::recording_index(&beat_bar0, 2)
        );
    }

    #[test]
    fn soloist_starts_in_recording_mode() {
        // repeat_end_bar starts at 0.0, and time_bars starts at 0.0.
        // 0.0 < 0.0 is false → recording mode active from the start.
        let s = Soloist::new(
            Sampler { handle: bevy::asset::Handle::default(), volume: 1.0 },
            2,
            2,
        );
        assert_eq!(s.repeat_end_bar, 0.0);
        assert!(!(0.0f32 < s.repeat_end_bar)); // recording mode
    }

    #[test]
    fn recorded_melody_pre_allocated_as_none() {
        let s = Soloist::new(
            Sampler { handle: bevy::asset::Handle::default(), volume: 1.0 },
            2,
            1,
        );
        assert_eq!(s.recorded_melody.len(), 32);
        assert!(s.recorded_melody.iter().all(|n| n.is_none()));
    }

    #[test]
    fn generate_note_downbeat_always_returns_scale_note_when_available() {
        let chord = crate::musicians::Chord::new(
            0.0,
            vec![],
            vec![Note::new(0, 1.0), Note::new(2, 0.5)],
        );
        // step=0 should always return something when a strength-1.0 note exists
        for _ in 0..10 {
            assert!(Soloist::generate_note(0, &chord, 0.0).is_some());
        }
    }

    #[test]
    fn generate_note_16th_never_plays_at_half_intensity() {
        let chord = crate::musicians::Chord::new(
            0.0,
            vec![],
            vec![Note::new(0, 0.0)],
        );
        // step=1 (odd), intensity=0.5: 0.5 - 0.5 = 0.0, gen >= 0 → never fires
        let fires: Vec<bool> = (0..50)
            .map(|_| Soloist::generate_note(1, &chord, 0.5).is_some())
            .collect();
        assert!(fires.iter().all(|&f| !f), "16th should not play at intensity=0.5");
    }

    #[test]
    fn generate_note_quarter_never_plays_at_zero_intensity() {
        let chord = crate::musicians::Chord::new(
            0.0,
            vec![],
            vec![Note::new(0, 0.5)],
        );
        // step=4 (quarter), intensity=0.0: gen < 0.0 → never
        let fires: Vec<bool> = (0..50)
            .map(|_| Soloist::generate_note(4, &chord, 0.0).is_some())
            .collect();
        assert!(fires.iter().all(|&f| !f));
    }

    #[test]
    fn total_recording_steps_equals_record_bars_times_steps_per_bar() {
        let s = Soloist::new(
            Sampler { handle: bevy::asset::Handle::default(), volume: 1.0 },
            4,
            1,
        );
        assert_eq!(s.recorded_melody.len(), 4 * STEPS_PER_BAR as usize);
    }
}
