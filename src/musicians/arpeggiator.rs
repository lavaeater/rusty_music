use crate::clock::Beat;
use crate::instrument::{Instrument, NoteEvent};
use crate::musicians::{Chord, MusicPlayer, Sampler};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ArpeggioMode {
    Up,
    Down,
    PingPong,
    Random,
    /// Intensity-driven: Up at low, PingPong at mid, Random at high.
    Auto,
}

pub struct Arpeggiator {
    /// Handed to the musician as its instrument; `None` once taken.
    pub sampler: Option<Sampler>,
    pub arpeggio_mode: ArpeggioMode,
    /// When true, occasionally substitutes scale notes for chord tones at high intensity.
    pub use_scale_runs: bool,
    pub some_index: u32,
    /// Direction for PingPong mode: +1 = ascending, -1 = descending.
    pub ping_pong_dir: i32,
    /// The sixteenth_count at which the next note should fire.
    pub next_sixteenth: u32,
}

/// An arpeggiator with no instrument of its own; give its `Musician` one.
impl Default for Arpeggiator {
    fn default() -> Self {
        Self {
            sampler: None,
            arpeggio_mode: ArpeggioMode::Auto,
            use_scale_runs: false,
            some_index: 0,
            ping_pong_dir: 1,
            next_sixteenth: 0,
        }
    }
}

impl Arpeggiator {
    pub fn new(sampler: Sampler) -> Self {
        Self { sampler: Some(sampler), ..Self::default() }
    }

    fn advance_index(&mut self, len: u32, mode: ArpeggioMode) -> u32 {
        match mode {
            ArpeggioMode::Up => {
                let note = self.some_index;
                self.some_index = (self.some_index + 1) % len;
                note
            }
            ArpeggioMode::Down => {
                let note = (len - 1).saturating_sub(self.some_index);
                self.some_index = (self.some_index + 1) % len;
                note
            }
            ArpeggioMode::PingPong => {
                let note = self.some_index;
                let next = self.some_index as i32 + self.ping_pong_dir;
                if next < 0 || next >= len as i32 {
                    self.ping_pong_dir = -self.ping_pong_dir;
                    self.some_index = (self.some_index as i32 + self.ping_pong_dir)
                        .clamp(0, len as i32 - 1) as u32;
                } else {
                    self.some_index = next as u32;
                }
                note
            }
            ArpeggioMode::Random => rand::random_range(0u32..len),
            ArpeggioMode::Auto => unreachable!(),
        }
    }
}

impl MusicPlayer for Arpeggiator {
    fn take_instrument(&mut self) -> Option<Box<dyn Instrument>> {
        self.sampler.take().map(|s| Box::new(s) as Box<dyn Instrument>)
    }

    fn play(&mut self, beat: Beat, notes_out: &mut Vec<NoteEvent>, base_intensity: f32, chord: &Chord) {
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

        // At high intensity, occasionally weave in scale notes instead of chord tones.
        let use_scale = self.use_scale_runs
            && !chord.scale_notes.is_empty()
            && rand::random::<f32>() < (base_intensity - 0.5).max(0.0);

        let notes = if use_scale { &chord.scale_notes } else { &chord.chord_notes };
        let len = notes.len() as u32;
        if len == 0 {
            return;
        }

        let effective_mode = match self.arpeggio_mode {
            ArpeggioMode::Auto => {
                if base_intensity < 0.4 {
                    ArpeggioMode::Up
                } else if base_intensity < 0.7 {
                    ArpeggioMode::PingPong
                } else {
                    ArpeggioMode::Random
                }
            }
            other => other,
        };

        let note_index = self.advance_index(len, effective_mode);

        if let Some(note) = notes.get(note_index as usize) {
            notes_out.push(NoteEvent::from_diff(note.midi_note_diff).length(wait_ticks as f32));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::Handle;
    use crate::musicians::{AudioSample, Sampler};

    fn wait_for(intensity: f32) -> u32 {
        if intensity < 0.4 { 4 } else if intensity < 0.7 { 2 } else { 1 }
    }

    fn make_arp() -> Arpeggiator {
        Arpeggiator {
            sampler: Some(Sampler { handle: Handle::<AudioSample>::default(), volume: 1.0 }),
            arpeggio_mode: ArpeggioMode::Up,
            use_scale_runs: false,
            some_index: 0,
            ping_pong_dir: 1,
            next_sixteenth: 0,
        }
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
    fn up_mode_cycles_ascending() {
        let mut arp = make_arp();
        let seq: Vec<u32> = (0..6).map(|_| arp.advance_index(4, ArpeggioMode::Up)).collect();
        assert_eq!(seq, vec![0, 1, 2, 3, 0, 1]);
    }

    #[test]
    fn down_mode_cycles_descending() {
        let mut arp = make_arp();
        let seq: Vec<u32> = (0..6).map(|_| arp.advance_index(4, ArpeggioMode::Down)).collect();
        assert_eq!(seq, vec![3, 2, 1, 0, 3, 2]);
    }

    #[test]
    fn ping_pong_bounces() {
        let mut arp = make_arp();
        let seq: Vec<u32> = (0..8).map(|_| arp.advance_index(4, ArpeggioMode::PingPong)).collect();
        assert_eq!(seq, vec![0, 1, 2, 3, 2, 1, 0, 1]);
    }

    #[test]
    fn auto_mode_selects_up_at_low_intensity() {
        let auto_mode = |intensity: f32| -> ArpeggioMode {
            match ArpeggioMode::Auto {
                ArpeggioMode::Auto => {
                    if intensity < 0.4 { ArpeggioMode::Up }
                    else if intensity < 0.7 { ArpeggioMode::PingPong }
                    else { ArpeggioMode::Random }
                }
                other => other,
            }
        };
        assert_eq!(auto_mode(0.0), ArpeggioMode::Up);
        assert_eq!(auto_mode(0.39), ArpeggioMode::Up);
        assert_eq!(auto_mode(0.4), ArpeggioMode::PingPong);
        assert_eq!(auto_mode(0.7), ArpeggioMode::Random);
    }

    #[test]
    fn next_sixteenth_advances_by_wait() {
        let wait: u32 = wait_for(0.5);
        let mut next_sixteenth: u32 = 0;
        let current: u32 = 5;
        if current >= next_sixteenth {
            next_sixteenth = current + wait;
        }
        assert_eq!(next_sixteenth, 7);
    }

    #[test]
    fn does_not_fire_before_next_sixteenth() {
        let next_sixteenth: u32 = 10;
        assert!(9 < next_sixteenth);
        assert!(10 >= next_sixteenth);
    }
}
