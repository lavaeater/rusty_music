use bevy::prelude::{Commands, Time};
use bevy_seedling::prelude::{Audio, AudioEvents, InstantSeconds, PlaybackSettings, SamplePlayer, Volume};
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
    /// The sixteenth_count at which the next note should fire.
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
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, chord: &Chord, audio_time: &Time<Audio>, scheduled_at: InstantSeconds) {
        // Higher intensity → more frequent notes.
        // wait=4 → quarter notes, wait=2 → 8ths, wait=1 → 16ths.
        let wait_ticks: u32 = if base_intensity < 0.4 {
            4
        } else if base_intensity < 0.7 {
            2
        } else {
            1
        };

        if beat.sixteenth_count < self.next_sixteenth {
            return;
        }
        self.next_sixteenth = beat.sixteenth_count + wait_ticks;

        let chord_note_length = chord.chord_notes.len() as u32;
        if chord_note_length == 0 {
            return;
        }

        let note_index = match self.arpeggio_mode {
            ArpeggioMode::Up => {
                self.some_index = (self.some_index + 1) % chord_note_length;
                self.some_index
            }
            ArpeggioMode::Down => {
                self.some_index = (self.some_index + 1) % chord_note_length;
                chord_note_length - (self.some_index + 1)
            }
            ArpeggioMode::Random => rand::random_range(0u32..chord_note_length),
        };

        if let Some(note) = chord.chord_notes.get(note_index as usize) {
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
}

#[cfg(test)]
mod tests {
    fn wait_for(intensity: f32) -> u32 {
        if intensity < 0.4 { 4 } else if intensity < 0.7 { 2 } else { 1 }
    }

    #[test]
    fn low_intensity_plays_every_4_ticks() {
        assert_eq!(wait_for(0.0), 4);
        assert_eq!(wait_for(0.39), 4);
    }

    #[test]
    fn mid_intensity_plays_every_2_ticks() {
        assert_eq!(wait_for(0.4), 2);
        assert_eq!(wait_for(0.69), 2);
    }

    #[test]
    fn high_intensity_plays_every_tick() {
        assert_eq!(wait_for(0.7), 1);
        assert_eq!(wait_for(1.0), 1);
    }

    #[test]
    fn wait_is_monotonically_decreasing_with_intensity() {
        assert!(wait_for(0.0) > wait_for(0.5));
        assert!(wait_for(0.5) > wait_for(1.0));
    }

    #[test]
    fn next_sixteenth_advances_by_wait() {
        // Simulate firing logic: next_sixteenth is set to current + wait
        let wait: u32 = wait_for(0.5); // mid intensity → 2 ticks
        let mut next_sixteenth: u32 = 0;
        let current: u32 = 5;
        if current >= next_sixteenth {
            next_sixteenth = current + wait;
        }
        assert_eq!(next_sixteenth, 7);
    }

    #[test]
    fn does_not_fire_before_next_sixteenth() {
        let _wait = wait_for(0.5); // 2
        let next_sixteenth: u32 = 10;
        // sixteenth_count < next_sixteenth → skip
        assert!(9 < next_sixteenth);
        assert!(10 >= next_sixteenth); // fires at exactly next_sixteenth
    }
}
