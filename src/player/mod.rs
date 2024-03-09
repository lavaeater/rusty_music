use bevy::prelude::{EventReader, Query, Res, Resource};
use bevy_kira_audio::Audio;
use crate::clock::Beat;
use crate::musicians::{Musician};
use crate::musicians::conductor::Conductor;

#[derive(Debug, Resource)]
pub struct Intensity(pub f32);

pub fn play_sound_on_the_beat(
    mut beat_reader: EventReader<Beat>,
    audio: Res<Audio>,
    conductor: Res<Conductor>,
    intensity: Res<Intensity>,
    mut instruments: Query<&mut Musician>,
) {
    for beat in beat_reader.read() {
        let chord_bar = beat.bar_count % conductor.chords.len() as u32;
        let chord = &conductor.chords[chord_bar as usize];

        for mut musician in instruments.iter_mut() {
            musician.player.play(*beat, &audio, intensity.0, chord);
        }
    }
}
