use bevy::prelude::{EventReader, Query, Res};
use bevy_kira_audio::{Audio, AudioControl};
use crate::clock::Beat;
use crate::Sample;

pub fn play_sound_on_the_beat(
    mut beat_reader: EventReader<Beat>,
    audio: Res<Audio>,
    samples_query: Query<&Sample>,
) {
    for beat in beat_reader.read() {
        // println!("Quarter: {}, Eight: {}, Sixteenth: {},", beat.quarter, beat.eigth, beat.sixteenth);
        for sample in samples_query.iter() {
            println!("Sixteenth: {}, Sample: {}, Modulo: {}",
                   beat.sixteenth,
                   sample.play_every_sixteenth,
                   beat.sixteenth % sample.play_every_sixteenth);
            if (beat.sixteenth + sample.play_at_offset) % sample.play_every_sixteenth == 0 {
                audio.play(sample.handle.clone_weak());
            }
        }

        //
        //
        // if beat.quarter % 4 == 0 {
        //         println!("Bass Drum");
        // }
        // if beat.quarter % 3 == 0 {
        //         println!("Snare Drum");
        // }
    }
}
