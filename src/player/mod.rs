use bevy::prelude::{EventReader, Query, Res};
use bevy_kira_audio::{Audio, AudioControl};
use crate::clock::{Beat, Conductor};
use crate::Sample;

pub fn play_sound_on_the_beat(
    mut beat_reader: EventReader<Beat>,
    audio: Res<Audio>,
    conductor: Res<Conductor>,
) {
    for beat in beat_reader.read() {
        println!("Bar: {}, Beat: {}", beat.bar, beat.beat);

        conductor.musicians.iter().for_each(|musician| {
           musician.signal(&audio, *beat, 0.5);
        });
    }
}
