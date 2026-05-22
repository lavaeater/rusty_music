use bevy::prelude::Commands;
use bevy_seedling::prelude::{PlaybackSettings, SamplePlayer, Volume};
use crate::clock::Beat;
use crate::musicians::{Chord, midi_diff_to_pitch, MusicPlayer, Note, Sampler, TonalPlayer, STEPS_PER_BAR};

/// AABA phrase form: record A, play A, play A, record B, play A, then start over.
#[derive(Clone, Copy, PartialEq, Debug)]
enum AabaPhase {
    RecordA,
    PlayA1,
    PlayA2,
    RecordB,
    PlayA3,
}

impl AabaPhase {
    fn next(self) -> Self {
        match self {
            Self::RecordA => Self::PlayA1,
            Self::PlayA1 => Self::PlayA2,
            Self::PlayA2 => Self::RecordB,
            Self::RecordB => Self::PlayA3,
            Self::PlayA3 => Self::RecordA,
        }
    }

    fn is_recording(self) -> bool {
        matches!(self, Self::RecordA | Self::RecordB)
    }
}

pub struct Soloist {
    /// Number of bars in each AABA section.
    pub record_bars: u32,
    melody_a: Vec<Option<Note>>,
    melody_b: Vec<Option<Note>>,
    phase: AabaPhase,
    /// `time_bars` at which the current section ends and the next begins.
    /// 0.0 means "start recording immediately".
    section_end_bar: f32,
    sampler: Sampler,
}

impl Soloist {
    pub fn new(sampler: Sampler, record_bars: u32) -> Self {
        let total = (record_bars * STEPS_PER_BAR) as usize;
        Self {
            record_bars,
            melody_a: vec![None; total],
            melody_b: vec![None; total],
            phase: AabaPhase::RecordA,
            section_end_bar: 0.0,
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

    fn recording_index(beat: &Beat, record_bars: u32) -> u32 {
        let within_bar = TonalPlayer::flat_step(beat);
        let bar_in_cycle = beat.bar_count % record_bars;
        bar_in_cycle * STEPS_PER_BAR + within_bar
    }

    fn generate_note(step: u32, chord: &Chord, intensity: f32) -> Option<Note> {
        if step == 0 {
            TonalPlayer::get_scale_note(chord, 1.0)
        } else if step % 4 == 0 {
            if rand::random::<f32>() < intensity {
                TonalPlayer::get_scale_note(chord, 0.5)
            } else {
                None
            }
        } else if step % 2 == 0 {
            if rand::random::<f32>() < intensity - 0.25 {
                TonalPlayer::get_scale_note(chord, 0.25)
            } else {
                None
            }
        } else if rand::random::<f32>() < intensity - 0.5 {
            TonalPlayer::get_scale_note(chord, 0.0)
        } else {
            None
        }
    }
}

impl MusicPlayer for Soloist {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, chord: &Chord) {
        let recording_index = Self::recording_index(&beat, self.record_bars) as usize;
        let last_index = (self.record_bars * STEPS_PER_BAR - 1) as usize;

        // Phase transition: advance when the section has expired and a new cycle starts.
        if self.section_end_bar > 0.0
            && beat.time_bars >= self.section_end_bar
            && recording_index == 0
        {
            self.phase = self.phase.next();
            self.section_end_bar = 0.0;
            match self.phase {
                AabaPhase::RecordA => self.melody_a.fill(None),
                AabaPhase::RecordB => self.melody_b.fill(None),
                _ => {}
            }
        }

        if self.phase.is_recording() {
            let step = TonalPlayer::flat_step(&beat);
            let note = Self::generate_note(step, chord, base_intensity);

            match self.phase {
                AabaPhase::RecordA => self.melody_a[recording_index] = note,
                AabaPhase::RecordB => self.melody_b[recording_index] = note,
                _ => unreachable!(),
            }

            if let Some(n) = note {
                self.play_note(n, commands);
            }

            if recording_index >= last_index && self.section_end_bar <= 0.0 {
                self.section_end_bar = beat.time_bars.ceil();
            }
        } else {
            // Playback: A replays the A melody; B section is always recorded live.
            if let Some(note) = self.melody_a[recording_index] {
                self.play_note(note, commands);
            }

            if recording_index >= last_index && self.section_end_bar <= 0.0 {
                self.section_end_bar = beat.time_bars.ceil();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::Handle;
    use crate::musicians::{AudioSample, Sampler};

    fn make_beat(beat: u32, sixteenth: u32, bar: u32) -> Beat {
        Beat {
            elapsed_time: 0.0,
            beat,
            sixteenth,
            bar_count: bar,
            beat_count: bar * 4 + beat,
            sixteenth_count: bar * 16 + beat * 4 + sixteenth,
            time_bars: bar as f32 + beat as f32 / 4.0 + sixteenth as f32 / 16.0,
            overshoot: 0.0,
        }
    }

    fn make_soloist(record_bars: u32) -> Soloist {
        Soloist::new(
            Sampler { handle: Handle::<AudioSample>::default(), volume: 1.0 },
            record_bars,
        )
    }

    #[test]
    fn recording_index_downbeat_bar0() {
        assert_eq!(Soloist::recording_index(&make_beat(0, 0, 0), 2), 0);
    }

    #[test]
    fn recording_index_last_step_bar0() {
        assert_eq!(Soloist::recording_index(&make_beat(3, 3, 0), 2), 15);
    }

    #[test]
    fn recording_index_bar1() {
        assert_eq!(Soloist::recording_index(&make_beat(0, 0, 1), 2), 16);
    }

    #[test]
    fn recording_index_last_step_bar1() {
        assert_eq!(Soloist::recording_index(&make_beat(3, 3, 1), 2), 31);
    }

    #[test]
    fn recording_index_wraps_at_record_bars() {
        let beat_bar2 = make_beat(0, 0, 2);
        let beat_bar0 = make_beat(0, 0, 0);
        assert_eq!(
            Soloist::recording_index(&beat_bar2, 2),
            Soloist::recording_index(&beat_bar0, 2)
        );
    }

    #[test]
    fn soloist_starts_in_record_a_phase() {
        let s = make_soloist(2);
        assert_eq!(s.phase, AabaPhase::RecordA);
        assert_eq!(s.section_end_bar, 0.0);
    }

    #[test]
    fn both_melodies_pre_allocated_as_none() {
        let s = make_soloist(2);
        assert_eq!(s.melody_a.len(), 32);
        assert_eq!(s.melody_b.len(), 32);
        assert!(s.melody_a.iter().all(|n| n.is_none()));
        assert!(s.melody_b.iter().all(|n| n.is_none()));
    }

    #[test]
    fn total_recording_steps_equals_record_bars_times_steps_per_bar() {
        let s = make_soloist(4);
        assert_eq!(s.melody_a.len(), 4 * STEPS_PER_BAR as usize);
        assert_eq!(s.melody_b.len(), 4 * STEPS_PER_BAR as usize);
    }

    #[test]
    fn aaba_phase_sequence() {
        let mut phase = AabaPhase::RecordA;
        let expected = [
            AabaPhase::PlayA1,
            AabaPhase::PlayA2,
            AabaPhase::RecordB,
            AabaPhase::PlayA3,
            AabaPhase::RecordA,
        ];
        for &next in &expected {
            phase = phase.next();
            assert_eq!(phase, next);
        }
    }

    #[test]
    fn recording_phases_are_record_a_and_record_b() {
        assert!(AabaPhase::RecordA.is_recording());
        assert!(AabaPhase::RecordB.is_recording());
        assert!(!AabaPhase::PlayA1.is_recording());
        assert!(!AabaPhase::PlayA2.is_recording());
        assert!(!AabaPhase::PlayA3.is_recording());
    }

    #[test]
    fn generate_note_downbeat_always_returns_scale_note_when_available() {
        let chord = Chord::new(
            0.0,
            vec![],
            vec![Note::new(0, 1.0), Note::new(2, 0.5)],
        );
        for _ in 0..10 {
            assert!(Soloist::generate_note(0, &chord, 0.0).is_some());
        }
    }

    #[test]
    fn generate_note_16th_never_plays_at_half_intensity() {
        let chord = Chord::new(0.0, vec![], vec![Note::new(0, 0.0)]);
        let fires: Vec<bool> = (0..50)
            .map(|_| Soloist::generate_note(1, &chord, 0.5).is_some())
            .collect();
        assert!(fires.iter().all(|&f| !f));
    }

    #[test]
    fn generate_note_quarter_never_plays_at_zero_intensity() {
        let chord = Chord::new(0.0, vec![], vec![Note::new(0, 0.5)]);
        let fires: Vec<bool> = (0..50)
            .map(|_| Soloist::generate_note(4, &chord, 0.0).is_some())
            .collect();
        assert!(fires.iter().all(|&f| !f));
    }
}
