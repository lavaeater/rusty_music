pub mod drummer;
pub mod conductor;
pub mod bassist;
pub mod soloist;
pub mod arpeggiator;

use std::cmp::Ordering;
use bevy::prelude::{Component, Res};
use bevy_kira_audio::{Audio, AudioSource};
use bevy::asset::Handle;
use rand::seq::IteratorRandom;
use crate::clock::Beat;

/// 16th-note steps per bar in 4/4.
pub const STEPS_PER_BAR: u32 = 16;

pub fn midi_diff_to_pitch_what(midi_diff: i32) -> f64 {
    2.0f64.powf(midi_diff as f64 / 12.0)
}

pub fn midi_diff_to_pitch(midi_diff: i32) -> f64 {
    let min_pitch = -12;
    let max_pitch = 12;
    match midi_diff.cmp(&0) {
        Ordering::Less => {
            if midi_diff < min_pitch { 0.5 } else { midi_diff_to_pitch_what(midi_diff) }
        }
        Ordering::Equal => 1.0,
        Ordering::Greater => {
            if midi_diff > max_pitch { 2.0 } else { midi_diff_to_pitch_what(midi_diff) }
        }
    }
}

pub trait MusicPlayer: Send + Sync {
    fn play(&mut self, beat: Beat, audio: &Res<Audio>, base_intensity: f32, chord: &Chord);
}

pub struct Sampler {
    pub handle: Handle<AudioSource>,
    pub volume: f64,
}

/// Strength semantics (used by both tonal musicians and drummers):
/// - `1.0` = essential, plays at any intensity
/// - `0.0` = maximum embellishment, plays only at full intensity
///
/// Play condition: `note.strength >= (1.0 - intensity)`
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Note {
    pub midi_note_diff: i32,
    pub strength: f32,
}

impl Note {
    pub fn new(midi_note_diff: i32, strength: f32) -> Self {
        Self { midi_note_diff, strength }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chord {
    /// Bar position at which this chord begins (fractional).
    pub pos_bars: f32,
    pub chord_notes: Vec<Note>,
    pub scale_notes: Vec<Note>,
}

impl Chord {
    pub fn new(pos_bars: f32, chord_notes: Vec<Note>, scale_notes: Vec<Note>) -> Self {
        Self { pos_bars, chord_notes, scale_notes }
    }
}

/// Shared helpers for tonal musicians (bassist, soloist, arpeggiator).
pub struct TonalPlayer;

impl TonalPlayer {
    /// Return a random note from `notes` whose strength is at least `min_strength`.
    pub fn get_note<'a>(notes: &'a [Note], min_strength: f32) -> Option<&'a Note> {
        notes
            .iter()
            .filter(|n| n.strength >= min_strength)
            .choose(&mut rand::thread_rng())
    }

    pub fn get_chord_note(chord: &Chord, min_strength: f32) -> Option<Note> {
        Self::get_note(&chord.chord_notes, min_strength).copied()
    }

    pub fn get_scale_note(chord: &Chord, min_strength: f32) -> Option<Note> {
        Self::get_note(&chord.scale_notes, min_strength).copied()
    }

    /// Flat 16th-note step within the current bar (0–15 in 4/4).
    pub fn flat_step(beat: &Beat) -> u32 {
        beat.beat * 4 + beat.sixteenth
    }
}

#[derive(Component)]
pub struct Musician {
    pub name: String,
    pub player: Box<dyn MusicPlayer>,
}

impl Musician {
    pub fn new(name: String, player: impl MusicPlayer + 'static) -> Self {
        Self {
            name,
            player: Box::new(player),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_beat(beat: u32, sixteenth: u32) -> Beat {
        Beat {
            elapsed_time: 0.0,
            beat,
            sixteenth,
            bar_count: 0,
            beat_count: beat,
            sixteenth_count: beat * 4 + sixteenth,
            time_bars: beat as f32 / 4.0 + sixteenth as f32 / 16.0,
        }
    }

    fn scale_notes() -> Vec<Note> {
        vec![
            Note::new(0, 1.0),   // essential
            Note::new(2, 0.75),
            Note::new(4, 0.5),
            Note::new(7, 0.25),  // embellishment
        ]
    }

    #[test]
    fn get_note_returns_none_when_all_filtered() {
        assert!(TonalPlayer::get_note(&scale_notes(), 1.1).is_none());
    }

    #[test]
    fn get_note_includes_at_threshold() {
        let notes = vec![Note::new(0, 0.5)];
        assert!(TonalPlayer::get_note(&notes, 0.5).is_some());
    }

    #[test]
    fn get_note_excludes_below_threshold() {
        let notes = vec![Note::new(0, 0.4), Note::new(2, 0.6)];
        for _ in 0..20 {
            let n = TonalPlayer::get_note(&notes, 0.5).unwrap();
            assert_eq!(n.midi_note_diff, 2);
        }
    }

    #[test]
    fn get_note_only_essential_at_zero_intensity() {
        // min_strength = 1.0 - 0.0 = 1.0
        for _ in 0..20 {
            let n = TonalPlayer::get_note(&scale_notes(), 1.0).unwrap();
            assert_eq!(n.strength, 1.0);
        }
    }

    #[test]
    fn flat_step_downbeat_is_zero() {
        assert_eq!(TonalPlayer::flat_step(&test_beat(0, 0)), 0);
    }

    #[test]
    fn flat_step_last_position_is_fifteen() {
        assert_eq!(TonalPlayer::flat_step(&test_beat(3, 3)), 15);
    }

    #[test]
    fn flat_step_quarter_positions() {
        assert_eq!(TonalPlayer::flat_step(&test_beat(1, 0)), 4);
        assert_eq!(TonalPlayer::flat_step(&test_beat(2, 0)), 8);
        assert_eq!(TonalPlayer::flat_step(&test_beat(3, 0)), 12);
    }

    #[test]
    fn flat_step_eighth_positions() {
        assert_eq!(TonalPlayer::flat_step(&test_beat(0, 2)), 2);
        assert_eq!(TonalPlayer::flat_step(&test_beat(1, 2)), 6);
    }
}
