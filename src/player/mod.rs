use bevy::prelude::{Commands, MessageReader, Query, Res, Resource, Without};
use crate::clock::Beat;
use crate::musicians::{Musician, Muted};
use crate::musicians::conductor::Conductor;

#[derive(Debug, Resource)]
pub struct Intensity(pub f32);

pub fn play_sound_on_the_beat(
    mut beat_reader: MessageReader<Beat>,
    mut commands: Commands,
    conductor: Res<Conductor>,
    intensity: Res<Intensity>,
    mut instruments: Query<&mut Musician, Without<Muted>>,
) {
    for beat in beat_reader.read() {
        let chord = conductor.current_chord(beat.time_bars);

        for mut musician in instruments.iter_mut() {
            musician.player.play(*beat, &mut commands, intensity.0, chord);
        }
    }
}
