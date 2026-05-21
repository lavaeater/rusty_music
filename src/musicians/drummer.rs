use bevy::prelude::Commands;
use std::collections::HashMap;
use bevy_seedling::prelude::{PlaybackSettings, SamplePlayer, Volume};
use rand::seq::IteratorRandom;
use crate::clock::Beat;
use crate::musicians::{Chord, midi_diff_to_pitch, MusicPlayer, Note, Sampler};

pub struct SuperDrummer {
    pub drums: Vec<Drummer>,
}

impl SuperDrummer {
    pub fn new(drums: Vec<Drummer>) -> Self {
        Self { drums }
    }
}

impl MusicPlayer for SuperDrummer {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, chord: &Chord) {
        for drummer in self.drums.iter_mut() {
            drummer.play(beat, commands, base_intensity, chord);
        }
    }
}

pub struct Drummer {
    pub notes: HashMap<(u32, u32), Note>,
    pub sampler: Sampler,
}

impl Drummer {
    pub fn new(sampler: Sampler, notes: HashMap<(u32, u32), Note>) -> Self {
        Self { notes, sampler }
    }
}

impl MusicPlayer for Drummer {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, _chord: &Chord) {
        // A note plays when its strength >= (1.0 - intensity):
        // strength=1.0 → always plays; strength=0.0 → only at full intensity.
        let min_strength = 1.0 - base_intensity;
        if let Some(note_to_play) = self
            .notes
            .iter()
            .filter(|(k, v)| {
                k.0 == beat.beat && k.1 == beat.sixteenth && v.strength >= min_strength
            })
            .choose(&mut rand::rng())
        {
            commands.spawn((
                SamplePlayer::new(self.sampler.handle.clone())
                    .with_volume(Volume::Decibels(self.sampler.volume as f32)),
                PlaybackSettings::default()
                    .with_speed(midi_diff_to_pitch(note_to_play.1.midi_note_diff)),
            ));
        }
    }
}

/// Standard 4/4 kick pattern.
/// Strength 1.0 = always plays; lower values = embellishments at higher intensity.
pub fn generate_kick_beat() -> HashMap<(u32, u32), Note> {
    HashMap::from([
        ((0, 0), Note::new(0, 1.0)),  // beat 1 downbeat — always
        ((2, 0), Note::new(0, 1.0)),  // beat 3 — always
        ((3, 0), Note::new(0, 0.5)),  // beat 4 — at mid intensity
        ((1, 2), Note::new(0, 0.25)), // 16th before beat 2 — fill
        ((3, 2), Note::new(0, 0.25)), // 16th before beat 4 — fill
    ])
}

/// Standard 4/4 snare on beats 2 and 4 with ghost-note embellishments.
pub fn generate_snare_beat() -> HashMap<(u32, u32), Note> {
    HashMap::from([
        ((1, 0), Note::new(0, 1.0)),  // beat 2 backbeat — always
        ((3, 0), Note::new(0, 1.0)),  // beat 4 backbeat — always
        ((0, 2), Note::new(0, 0.1)),  // ghost note — only at near-max intensity
        ((2, 2), Note::new(0, 0.1)),  // ghost note
    ])
}

/// Hi-hat: quarter notes always, 8th offbeats at mid intensity.
pub fn generate_hihat_beat() -> HashMap<(u32, u32), Note> {
    HashMap::from([
        ((0, 0), Note::new(0, 0.75)),
        ((1, 0), Note::new(0, 0.75)),
        ((2, 0), Note::new(0, 0.75)),
        ((3, 0), Note::new(0, 0.75)),
        ((0, 2), Note::new(0, 0.5)),
        ((1, 2), Note::new(0, 0.5)),
        ((2, 2), Note::new(0, 0.5)),
        ((3, 2), Note::new(0, 0.5)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notes_at(map: &HashMap<(u32, u32), Note>, beat: u32, six: u32) -> Vec<&Note> {
        map.iter()
            .filter(|(k, _)| k.0 == beat && k.1 == six)
            .map(|(_, v)| v)
            .collect()
    }

    fn passes(note: &Note, intensity: f32) -> bool {
        note.strength >= (1.0 - intensity)
    }

    #[test]
    fn kick_downbeat_always_plays() {
        let kick = generate_kick_beat();
        let note = &notes_at(&kick, 0, 0)[0];
        assert!(passes(note, 0.0), "kick downbeat should play at zero intensity");
        assert!(passes(note, 1.0));
    }

    #[test]
    fn kick_beat3_always_plays() {
        let kick = generate_kick_beat();
        let note = &notes_at(&kick, 2, 0)[0];
        assert!(passes(note, 0.0));
    }

    #[test]
    fn kick_fill_only_at_high_intensity() {
        let kick = generate_kick_beat();
        let note = &notes_at(&kick, 3, 2)[0];
        assert!(!passes(note, 0.5), "fill should not play at 0.5 intensity");
        assert!(passes(note, 0.8));
    }

    #[test]
    fn snare_backbeats_always_play() {
        let snare = generate_snare_beat();
        for (beat, six) in [(1u32, 0u32), (3, 0)] {
            let note = &notes_at(&snare, beat, six)[0];
            assert!(passes(note, 0.0), "snare on ({beat},{six}) should play at zero intensity");
        }
    }

    #[test]
    fn snare_ghost_only_near_full_intensity() {
        let snare = generate_snare_beat();
        let note = &notes_at(&snare, 0, 2)[0];
        assert!(!passes(note, 0.8), "ghost should not play at 0.8");
        assert!(passes(note, 0.95));
    }

    #[test]
    fn hihat_quarter_plays_at_low_intensity() {
        let hihat = generate_hihat_beat();
        let note = &notes_at(&hihat, 0, 0)[0];
        assert!(passes(note, 0.3));
    }

    #[test]
    fn hihat_eighth_only_at_mid_intensity() {
        let hihat = generate_hihat_beat();
        let note = &notes_at(&hihat, 0, 2)[0];
        assert!(!passes(note, 0.4), "8th hat should not play below 0.5");
        assert!(passes(note, 0.5));
    }
}
