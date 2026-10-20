use bevy::prelude::{Commands, MessageReader, Query, Res, ResMut, Resource, Time, Without};
use bevy_seedling::prelude::{Audio, AudioContext, FirewheelNode};
use crate::clock::Beat;
use crate::instrument::{NoteEvent, PlayContext};
use crate::musicians::{Musician, Muted};
use crate::musicians::conductor::Conductor;

#[derive(Debug, Resource)]
pub struct Intensity(pub f32);

/// Ask every musician for its notes on each beat and hand them to its
/// instrument, timed from the beat's audio-clock time.
#[allow(clippy::too_many_arguments)]
pub fn play_sound_on_the_beat(
    mut beat_reader: MessageReader<Beat>,
    mut commands: Commands,
    mut audio: ResMut<AudioContext>,
    time: Res<Time<Audio>>,
    conductor: Res<Conductor>,
    intensity: Res<Intensity>,
    mut musicians: Query<(&mut Musician, Option<&FirewheelNode>), Without<Muted>>,
    nodes: Query<&FirewheelNode>,
) {
    let find_node = |entity| nodes.get(entity).ok().map(|n| FirewheelNode(n.0));
    let mut notes: Vec<NoteEvent> = Vec::new();
    for beat in beat_reader.read() {
        let chord = conductor.current_chord(beat.time_bars);

        for (mut musician, node) in musicians.iter_mut() {
            let Musician { player, instrument, .. } = &mut *musician;
            notes.clear();
            player.play(*beat, &mut notes, intensity.0, chord);
            if notes.is_empty() {
                continue;
            }
            let mut cx = PlayContext { commands: &mut commands, audio: &mut audio, time: &time, node, find_node: &find_node };
            for note in &notes {
                let at = beat.time + note.offset as f64 * beat.step_secs;
                let length = note.length.max(0.0) as f64 * beat.step_secs;
                instrument.play(note, at, length, &mut cx);
            }
        }
    }
}
