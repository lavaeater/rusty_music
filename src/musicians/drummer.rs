use bevy::prelude::Res;
use bevy::utils::HashMap;
use bevy_kira_audio::{Audio, AudioControl};
use bevy_kira_audio::prelude::Volume;
use rand::prelude::IteratorRandom;
use crate::clock::Beat;
use crate::musicians::{Chord, midi_diff_to_pitch, MusicPlayer, Note, Sampler};

pub struct SuperDrummer {
    pub drums: Vec<Drummer>
}

impl SuperDrummer {
    pub fn new(drums: Vec<Drummer>) -> Self {
        Self {
            drums
        }
    }
}

impl MusicPlayer for SuperDrummer {
    fn play(&mut self, beat: Beat, audio: &Res<Audio>, base_intensity: f32, _chord: &Chord) {
        for drummer in self.drums.iter_mut() {
            drummer.play(beat, audio, base_intensity, _chord);
        }
    }
}

pub struct Drummer {
    pub notes: HashMap<(u32, u32), Note>,
    pub sampler: Sampler,
}

impl Drummer {
    pub fn new(sampler: Sampler, notes: HashMap<(u32, u32), Note>) -> Self {
        Self {
            notes,
            sampler,
        }
    }
}

impl MusicPlayer for Drummer {
    fn play(&mut self, beat: Beat, audio: &Res<Audio>, base_intensity: f32, _chord: &Chord) {
        if let Some(note_to_play) = self.notes.iter().filter(|(k, v) | {
            k.0 == beat.beat && k.1 == beat.sixteenth && v.strength <= base_intensity
        }).choose(&mut rand::thread_rng()) {
            audio.play(self.sampler.handle.clone_weak())
                .with_volume(Volume::from(self.sampler.volume))
                .with_playback_rate(midi_diff_to_pitch(note_to_play.1.midi_note_diff));
        }
    }
}

pub fn generate_kick_beat() -> HashMap<(u32, u32), Note> {
    // 0 1 2 3 0 1 2 3 0 1 2 3 0 1 2 3
    // 0       1       2       3
    HashMap::from([
        ((1, 0), Note {
            midi_note_diff: 0,
            strength: 0.25,
        }),
        ((3, 0), Note {
            midi_note_diff: 0,
            strength: 0.25,
        }),
        ((3, 2), Note {
            midi_note_diff: 0,
            strength: 0.5,
        }),
    ])
}

pub fn generate_snare_beat() -> HashMap<(u32, u32), Note> {
    HashMap::from([
        ((0, 0), Note {
            midi_note_diff: 0,
            strength: 0.25,
        }),
        ((2, 0), Note {
            midi_note_diff: 0,
            strength: 0.25,
        })
    ])
}

pub fn generate_hihat_beat() -> HashMap<(u32, u32), Note> {
    HashMap::from([
        ((0, 0), Note {
            midi_note_diff: 0,
            strength: 0.25,
        }),
        ((1, 0), Note {
            midi_note_diff: 0,
            strength: 0.25,
        }),
        ((2, 0), Note {
            midi_note_diff: 0,
            strength: 0.25,
        }),
        ((3, 0), Note {
            midi_note_diff: 0,
            strength: 0.25,
        }),
    ])
}
