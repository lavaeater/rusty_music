use bevy::prelude::{EventReader, Local, Query, Res};
use bevy::render::render_resource::binding_types::sampler;
use bevy_kira_audio::{AudioChannel, AudioControl};
use bevy_kira_audio::prelude::Volume;
use crate::clock::Beat;
use crate::{Bass, Drums, Soloists};
use crate::musicians::{Musician, MusicianType};
use crate::musicians::conductor::Conductor;


fn midi_diff_to_pitch_what(midi_diff: i32) -> f64 {
    let f = 2.0f64.powf(midi_diff as f64 / 12.0);
    f
}
fn midi_diff_to_pitch(midi_diff: i32) -> f64 {
    let min_pitch = -12;
    let max_pitch = 12;
    if midi_diff < 0 {
        if midi_diff < min_pitch {
            0.5
        } else {
            midi_diff_to_pitch_what(midi_diff)
        }
    } else if midi_diff > 0 {
        if midi_diff > max_pitch {
            2.0
        } else {
            midi_diff_to_pitch_what(midi_diff)
        }
    } else {
        1.0
    }
}


pub fn play_sound_on_the_beat(
    mut beat_reader: EventReader<Beat>,
    drums: Res<AudioChannel<Drums>>,
    bass: Res<AudioChannel<Bass>>,
    solos: Res<AudioChannel<Soloists>>,
    conductor: Res<Conductor>,
    mut intensity: Local<f32>,
    mut instruments: Query<&mut Musician>
) {
    for beat in beat_reader.read() {
        println!("beat: {:?}", beat);
        if beat.bar % 3 == 0 {
            *intensity += 0.1;
        }
        if beat.bar % 8 == 0 {
            *intensity = 0.5;
        }
        if *intensity > 1.0 {
            *intensity = 0.0;
        }

        let chord_bar = beat.bar % conductor.chords.len() as u32;
        let chord = &conductor.chords[chord_bar as usize];

        for mut musician in instruments.iter_mut() {
            if let Some(note) = musician.player.get_note(*beat, *intensity, chord) {
                match musician.musician_type {
                    MusicianType::Drums => {
                        drums.play(musician.sampler.handle.clone_weak())
                            .with_volume(Volume::from(musician.sampler.volume))
                            .with_playback_rate(midi_diff_to_pitch(note.midi_note_diff));
                    }
                    MusicianType::Bass => {
                        bass.play(musician.sampler.handle.clone_weak())
                            .with_volume(Volume::from(musician.sampler.volume))
                            .with_playback_rate(midi_diff_to_pitch(note.midi_note_diff));
                    }
                    MusicianType::Solo => {
                        solos.play(musician.sampler.handle.clone_weak())
                            .with_volume(Volume::from(musician.sampler.volume))
                            .with_playback_rate(midi_diff_to_pitch(note.midi_note_diff));
                    }
                }
            }
        }
    }
}
