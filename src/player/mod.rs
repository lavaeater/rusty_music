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
        println!("beat: {:?}", beat);
        if beat.bar % 4 == 0 {
            *intensity = 1.0;
        } else {
            *intensity = 0.5;
        }
        conductor.musicians.iter().for_each(|musician| {
           musician.signal(&audio, *beat, intensity.abs());
        });
    }
}
