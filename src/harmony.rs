//! Harmony as *scale degrees*, so the music can change mood without
//! rewriting the progression: the same i–VI–III–VII becomes brighter or
//! darker by switching mode, and tension adds sevenths and ninths.
//!
//! [`Harmony`] builds the [`Conductor`](crate::musicians::conductor::Conductor)'s
//! chords. With [`SituationPlugin`](crate::situation::SituationPlugin) it is
//! rebuilt at every phrase start from the current
//! [`MusicState`](crate::situation::MusicState).

use bevy::prelude::Resource;

use crate::musicians::{Chord, Note};

/// The seven diatonic modes used, ordered from darkest to brightest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    Phrygian,
    #[default]
    Aeolian,
    Dorian,
    Mixolydian,
    Ionian,
    Lydian,
}

impl Mode {
    pub const DARK_TO_BRIGHT: [Mode; 6] = [Mode::Phrygian, Mode::Aeolian, Mode::Dorian, Mode::Mixolydian, Mode::Ionian, Mode::Lydian];

    /// Semitones of each scale degree above the tonic.
    pub fn intervals(self) -> [i32; 7] {
        match self {
            Mode::Phrygian => [0, 1, 3, 5, 7, 8, 10],
            Mode::Aeolian => [0, 2, 3, 5, 7, 8, 10],
            Mode::Dorian => [0, 2, 3, 5, 7, 9, 10],
            Mode::Mixolydian => [0, 2, 4, 5, 7, 9, 10],
            Mode::Ionian => [0, 2, 4, 5, 7, 9, 11],
            Mode::Lydian => [0, 2, 4, 6, 7, 9, 11],
        }
    }

    /// Valence 0 (grim) … 1 (radiant) → a mode.
    pub fn from_valence(valence: f32) -> Mode {
        let i = (valence.clamp(0.0, 1.0) * (Self::DARK_TO_BRIGHT.len() - 1) as f32).round() as usize;
        Self::DARK_TO_BRIGHT[i]
    }

    /// Semitones above the tonic of scale degree `degree` (may exceed an octave).
    pub fn degree(self, degree: i32) -> i32 {
        12 * degree.div_euclid(7) + self.intervals()[degree.rem_euclid(7) as usize]
    }
}

/// A chord progression in scale degrees (0 = tonic, 5 = the sixth…).
#[derive(Debug, Clone, Resource)]
pub struct Harmony {
    /// Tonic, in semitones from C4 (-3 = A). Changes take effect at the next phrase.
    pub key: i32,
    pub mode: Mode,
    /// Scale degree of each chord's root, one chord per `bars_per_chord`.
    pub progression: Vec<i32>,
    pub bars_per_chord: f32,
    /// When true, the mode follows `MusicState` valence at phrase starts.
    pub mode_follows_valence: bool,
}

impl Default for Harmony {
    fn default() -> Self {
        // A minor: i – VI – III – VII.
        Self { key: -3, mode: Mode::Aeolian, progression: vec![0, 5, 2, 6], bars_per_chord: 1.0, mode_follows_valence: true }
    }
}

impl Harmony {
    /// Bars before the progression repeats: one phrase.
    pub fn phrase_bars(&self) -> f32 {
        self.progression.len().max(1) as f32 * self.bars_per_chord
    }

    /// Build the conductor's chords. `tension` (0–1) makes sevenths and ninths
    /// stronger, so musicians reach for them more as things get tense.
    pub fn chords(&self, tension: f32) -> Vec<Chord> {
        let tension = tension.clamp(0.0, 1.0);
        let mode = self.mode;
        let scale: Vec<i32> = (0..7).map(|d| self.key + mode.degree(d)).collect();

        self.progression
            .iter()
            .enumerate()
            .map(|(i, &degree)| {
                // Keep roots near the tonic so musicians' registers stay put.
                let mut root = self.key + mode.degree(degree.rem_euclid(7));
                if root > self.key + 7 {
                    root -= 12;
                }
                let above = |steps: i32| root + mode.degree(degree + steps) - mode.degree(degree);
                let chord_notes = vec![
                    Note::new(root, 1.0),
                    Note::new(above(2), 0.9),
                    Note::new(above(4), 0.8),
                    Note::new(root + 12, 0.6),
                    Note::new(above(6), 0.2 + 0.6 * tension),
                    Note::new(above(8), 0.1 + 0.5 * tension),
                ];
                let is_chord_tone = |pitch: i32| [root, above(2), above(4)].iter().any(|t| (t - pitch).rem_euclid(12) == 0);
                let scale_notes = (-2..10)
                    .map(|d| {
                        let pitch = self.key + mode.degree(d);
                        let strength = if pitch.rem_euclid(12) == scale[0].rem_euclid(12) {
                            1.0
                        } else if is_chord_tone(pitch) {
                            0.9
                        } else {
                            0.5
                        };
                        Note::new(pitch, strength)
                    })
                    .collect();
                Chord::new(i as f32 * self.bars_per_chord, chord_notes, scale_notes)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tones(chord: &Chord) -> Vec<i32> {
        chord.chord_notes.iter().map(|n| n.midi_note_diff).collect()
    }

    #[test]
    fn valence_orders_modes_dark_to_bright() {
        assert_eq!(Mode::from_valence(0.0), Mode::Phrygian);
        assert_eq!(Mode::from_valence(0.2), Mode::Aeolian);
        assert_eq!(Mode::from_valence(0.8), Mode::Ionian);
        assert_eq!(Mode::from_valence(1.0), Mode::Lydian);
        // Brighter modes never have lower degrees than darker ones.
        for pair in Mode::DARK_TO_BRIGHT.windows(2) {
            let (dark, bright) = (pair[0].intervals(), pair[1].intervals());
            assert!(dark.iter().zip(bright).all(|(d, b)| *d <= b));
        }
    }

    #[test]
    fn same_progression_changes_colour_with_mode() {
        let mut h = Harmony { mode: Mode::Aeolian, ..Harmony::default() };
        let minor = h.chords(0.0);
        // A minor: A C E.
        assert_eq!(&tones(&minor[0])[..3], &[-3, 0, 4]);
        // VI in A minor is F major: F A C.
        assert_eq!(&tones(&minor[1])[..3], &[-7, -3, 0]);

        h.mode = Mode::Ionian;
        let major = h.chords(0.0);
        // A major: A C# E.
        assert_eq!(&tones(&major[0])[..3], &[-3, 1, 4]);
    }

    #[test]
    fn tension_strengthens_extensions() {
        let h = Harmony::default();
        let calm = h.chords(0.0);
        let tense = h.chords(1.0);
        assert!(tense[0].chord_notes[4].strength > calm[0].chord_notes[4].strength);
        // The triad itself doesn't change.
        assert_eq!(&tones(&calm[0])[..4], &tones(&tense[0])[..4]);
    }

    #[test]
    fn chords_are_placed_one_per_bar() {
        let h = Harmony { bars_per_chord: 2.0, ..Harmony::default() };
        let pos: Vec<f32> = h.chords(0.5).iter().map(|c| c.pos_bars).collect();
        assert_eq!(pos, vec![0.0, 2.0, 4.0, 6.0]);
        assert_eq!(h.phrase_bars(), 8.0);
    }
}
