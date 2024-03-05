use bevy::prelude::{EventReader, Local, Query, Res};
use bevy_kira_audio::{Audio, AudioControl};
use crate::clock::{Beat, Conductor};
use crate::Sample;

pub fn play_sound_on_the_beat(
    mut beat_reader: EventReader<Beat>,
    audio: Res<Audio>,
    conductor: Res<Conductor>,
    mut intensity: Local<f32>
) {
    // *intensity += 0.01;
    // if *intensity >= 1.0 {
    //     *intensity = 0.0;
    // }
    for beat in beat_reader.read() {
        if beat.bar % 2 == 0 {
            *intensity = 0.3;
        } else {
            *intensity = 1.0;
        }
        conductor.musicians.iter().for_each(|musician| {
           musician.signal(&audio, *beat, intensity.abs());
        });
    }
}
