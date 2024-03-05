use bevy::prelude::{EventReader, Local, Query, Res};
use bevy_kira_audio::Audio;
use crate::clock::Beat;
use crate::musicians::bassist::Musician;
use crate::musicians::conductor::Conductor;

pub fn play_sound_on_the_beat(
    mut beat_reader: EventReader<Beat>,
    audio: Res<Audio>,
    conductor: Res<Conductor>,
    mut intensity: Local<f32>,
    instruments: Query<&Musician>
) {
    for beat in beat_reader.read() {
        println!("beat: {:?}", beat);
        if beat.bar % 4 == 0 {
            *intensity = 1.0;
        } else {
            *intensity = 0.5;
        }

        let chord_bar = beat.bar % conductor.chords.len() as u32;
        let chord = &conductor.chords[chord_bar as usize];

        for (musician) in instruments.iter() {
            musician.signal(&audio, *beat, intensity.abs(), chord);
        }
    }
}
