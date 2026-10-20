use bevy::prelude::{Commands, Has, Local, MessageReader, Query, Res, ResMut, Resource, Time};
use bevy_seedling::prelude::{Audio, AudioContext, FirewheelNode};
use crate::clock::Beat;
use crate::instrument::{NoteEvent, PlayContext};
use crate::musicians::{Musician, Muted};
use crate::musicians::conductor::Conductor;
use crate::mix::Section;
use crate::situation::Stinger;

#[derive(Debug, Resource)]
pub struct Intensity(pub f32);

/// A musician, its synth node (if any), whether it's muted, and its mix section.
type MusicianParts = (&'static mut Musician, Option<&'static FirewheelNode>, Has<Muted>, Option<&'static Section>);

/// Ask every musician for its notes on each beat and hand them to its
/// instrument, timed from the beat's audio-clock time. Queued [`Stinger`]s
/// play on their musician at the first beat matching their quantisation.
#[allow(clippy::too_many_arguments)]
pub fn play_sound_on_the_beat(
    mut beat_reader: MessageReader<Beat>,
    mut stinger_reader: MessageReader<Stinger>,
    mut stingers: Local<Vec<Stinger>>,
    mut commands: Commands,
    mut audio: ResMut<AudioContext>,
    time: Res<Time<Audio>>,
    conductor: Res<Conductor>,
    intensity: Res<Intensity>,
    mut musicians: Query<MusicianParts>,
    nodes: Query<&FirewheelNode>,
) {
    let find_node = |entity| nodes.get(entity).ok().map(|n| FirewheelNode(n.0));
    stingers.extend(stinger_reader.read().cloned());
    let mut notes: Vec<NoteEvent> = Vec::new();
    for beat in beat_reader.read() {
        let chord = conductor.current_chord(beat.time_bars);

        for (mut musician, node, muted, section) in musicians.iter_mut() {
            if muted {
                continue;
            }
            let Musician { player, instrument, .. } = &mut *musician;
            notes.clear();
            player.play(*beat, &mut notes, intensity.0, chord);
            if notes.is_empty() {
                continue;
            }
            let section = section.copied();
            let mut cx = PlayContext { commands: &mut commands, audio: &mut audio, time: &time, node, find_node: &find_node, section };
            for note in &notes {
                let at = beat.time + note.offset as f64 * beat.step_secs;
                let length = note.length.max(0.0) as f64 * beat.step_secs;
                instrument.play(note, at, length, &mut cx);
            }
        }

        let mut k = 0;
        while k < stingers.len() {
            if !stingers[k].quantize.matches(beat) {
                k += 1;
                continue;
            }
            let stinger = stingers.swap_remove(k);
            let Ok((mut musician, node, _, section)) = musicians.get_mut(stinger.musician) else { continue };
            let section = section.copied();
            let mut cx = PlayContext { commands: &mut commands, audio: &mut audio, time: &time, node, find_node: &find_node, section };
            for note in stinger.gesture.notes_for(chord) {
                let at = beat.time + note.offset as f64 * beat.step_secs;
                let length = note.length.max(0.0) as f64 * beat.step_secs;
                musician.instrument.play(&note, at, length, &mut cx);
            }
        }
    }
}
