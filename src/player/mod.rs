use bevy::prelude::{EventReader, Local, Query, Res};
use bevy_kira_audio::Audio;
use crate::clock::Beat;
use crate::musicians::Musician;
use crate::musicians::conductor::Conductor;

pub fn play_sound_on_the_beat(
    mut beat_reader: EventReader<Beat>,
    audio: Res<Audio>,
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
            musician.player.signal(&audio, *beat, intensity.abs(), chord);
        }
    }
}
