use bevy::prelude::{Commands, MessageReader, Query, Res, Resource, Time};
use bevy_seedling::prelude::{Audio, DurationSeconds};
use bevy_seedling::time::AudioTime;
use crate::clock::Beat;
use crate::musicians::Musician;
use crate::musicians::conductor::Conductor;

#[derive(Debug, Resource)]
pub struct Intensity(pub f32);

pub fn play_sound_on_the_beat(
    mut beat_reader: MessageReader<Beat>,
    mut commands: Commands,
    conductor: Res<Conductor>,
    intensity: Res<Intensity>,
    audio_time: Res<Time<Audio>>,
    mut instruments: Query<&mut Musician>,
) {
    for beat in beat_reader.read() {
        let chord = conductor.current_chord(beat.time_bars);
        let scheduled_at = audio_time.now() - DurationSeconds(beat.overshoot as f64);

        for mut musician in instruments.iter_mut() {
            musician.player.play(*beat, &mut commands, intensity.0, chord, &audio_time, scheduled_at);
        }
    }
}
