use bevy::prelude::Resource;
use crate::musicians::Chord;

#[derive(Resource)]
pub struct Conductor {
    /// Sorted list of chords by `pos_bars`. Must have at least one entry.
    pub chords: Vec<Chord>,
    /// Total length of the chord progression in bars before it loops.
    pub chord_length_bars: f32,
}

impl Conductor {
    /// Return the chord active at `time_bars`, wrapping at `chord_length_bars`.
    pub fn current_chord(&self, time_bars: f32) -> &Chord {
        let pos = time_bars % self.chord_length_bars;
        // Walk backwards: the last chord whose pos_bars <= pos wins.
        self.chords
            .iter()
            .rev()
            .find(|c| c.pos_bars <= pos)
            .unwrap_or(&self.chords[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::musicians::{Chord, Note};

    fn make_conductor() -> Conductor {
        Conductor {
            chords: vec![
                Chord::new(0.0, vec![Note::new(0, 1.0)], vec![]),
                Chord::new(1.0, vec![Note::new(2, 1.0)], vec![]),
                Chord::new(2.0, vec![Note::new(4, 1.0)], vec![]),
                Chord::new(3.0, vec![Note::new(7, 1.0)], vec![]),
            ],
            chord_length_bars: 4.0,
        }
    }

    #[test]
    fn current_chord_at_bar_zero() {
        let c = make_conductor();
        assert_eq!(c.current_chord(0.0).chord_notes[0].midi_note_diff, 0);
    }

    #[test]
    fn current_chord_mid_bar() {
        let c = make_conductor();
        assert_eq!(c.current_chord(0.5).chord_notes[0].midi_note_diff, 0);
    }

    #[test]
    fn current_chord_at_bar_boundary() {
        let c = make_conductor();
        assert_eq!(c.current_chord(1.0).chord_notes[0].midi_note_diff, 2);
        assert_eq!(c.current_chord(2.0).chord_notes[0].midi_note_diff, 4);
        assert_eq!(c.current_chord(3.0).chord_notes[0].midi_note_diff, 7);
    }

    #[test]
    fn current_chord_wraps_at_chord_length() {
        let c = make_conductor();
        assert_eq!(
            c.current_chord(4.0).chord_notes[0].midi_note_diff,
            c.current_chord(0.0).chord_notes[0].midi_note_diff
        );
        assert_eq!(
            c.current_chord(5.5).chord_notes[0].midi_note_diff,
            c.current_chord(1.5).chord_notes[0].midi_note_diff
        );
    }

    #[test]
    fn current_chord_handles_fractional_pos_bars() {
        let c = Conductor {
            chords: vec![
                Chord::new(0.0, vec![Note::new(0, 1.0)], vec![]),
                Chord::new(0.5, vec![Note::new(5, 1.0)], vec![]),
            ],
            chord_length_bars: 1.0,
        };
        assert_eq!(c.current_chord(0.25).chord_notes[0].midi_note_diff, 0);
        assert_eq!(c.current_chord(0.5).chord_notes[0].midi_note_diff, 5);
        assert_eq!(c.current_chord(0.75).chord_notes[0].midi_note_diff, 5);
    }
}
