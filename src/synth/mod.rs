//! Synthesised instruments: glicol voice graphs running inside Firewheel.
//!
//! Spawn a [`SynthNode`] (plus an optional [`SynthConfig`]) like any other
//! bevy_seedling node; it connects to the main bus. Mutating the `SynthNode`
//! component (cutoff, resonance, volume) is diffed to the audio thread.
//!
//! To let a [`Musician`](crate::musicians::Musician) play it, put both on the
//! same entity with a [`SynthInstrument`] — see [`synth_musician`]. Raw
//! timestamped notes can also be sent with [`send_note`].

pub mod engine;

use bevy::prelude::*;
use bevy_seedling::firewheel::{
    channel_config::ChannelConfig,
    clock::{EventInstant, InstantSeconds},
    diff::{Diff, Patch},
    event::{NodeEvent, NodeEventType, ProcEvents},
    node::{
        AudioNode, AudioNodeInfo, AudioNodeProcessor, ConstructProcessorContext, NodeError,
        ProcBuffers, ProcExtra, ProcInfo, ProcessStatus,
    },
    Volume,
};
use bevy_seedling::prelude::{AudioContext, FirewheelNode, RegisterNode};

pub use engine::{NoteMsg, SynthPatch, LATENCY};
use engine::SynthEngine;

use crate::instrument::{Instrument, NoteEvent, PlayContext};
use crate::musicians::{MusicPlayer, Musician};

/// Registers [`SynthNode`] with bevy_seedling.
pub struct SynthPlugin;

impl Plugin for SynthPlugin {
    fn build(&self, app: &mut App) {
        app.register_node::<SynthNode>();
    }
}

/// Live parameters of a synth instrument. Change them from any system.
#[derive(Diff, Patch, Debug, Clone, Component)]
pub struct SynthNode {
    pub volume: Volume,
    /// Low-pass cutoff in Hz.
    pub cutoff: f32,
    /// Filter resonance (Q).
    pub resonance: f32,
}

impl Default for SynthNode {
    fn default() -> Self {
        Self { volume: Volume::UNITY_GAIN, cutoff: 2_000.0, resonance: 1.0 }
    }
}

/// Construction-time settings: which patch, how many voices.
#[derive(Debug, Clone, PartialEq, Component)]
pub struct SynthConfig {
    pub patch: SynthPatch,
    pub voices: usize,
}

impl Default for SynthConfig {
    fn default() -> Self {
        Self { patch: SynthPatch::default(), voices: 8 }
    }
}

impl AudioNode for SynthNode {
    type Configuration = SynthConfig;

    fn info(&self, _config: &Self::Configuration) -> Result<AudioNodeInfo, NodeError> {
        Ok(AudioNodeInfo::new()
            .debug_name("glicol synth")
            .channel_config(ChannelConfig { num_inputs: 0.into(), num_outputs: 2.into() }))
    }

    fn construct_processor(
        &self,
        config: &Self::Configuration,
        cx: ConstructProcessorContext,
    ) -> Result<impl AudioNodeProcessor, NodeError> {
        let sr = cx.stream_info.sample_rate.get() as usize;
        Ok(SynthProcessor {
            engine: SynthEngine::new(config.patch, config.voices, sr, self.cutoff, self.resonance),
            params: self.clone(),
            sr: sr as f64,
        })
    }
}

struct SynthProcessor {
    engine: SynthEngine,
    params: SynthNode,
    sr: f64,
}

impl AudioNodeProcessor for SynthProcessor {
    fn events(&mut self, info: &ProcInfo, events: &mut ProcEvents, _extra: &mut ProcExtra) {
        let now = info.clock_samples.0;
        let mut filter_changed = false;
        for (event, time) in events.drain_with_timestamps() {
            if let NodeEventType::CustomBytes(bytes) = &event {
                let Some(msg) = NoteMsg::decode(bytes) else { continue };
                // Firewheel splits blocks at scheduled events, so `now` is usually
                // already the event's frame; the timestamp is the fallback.
                let frame = match time {
                    Some(EventInstant::AtClockSeconds(s)) => ((s.0 * self.sr).round() as i64).max(now),
                    _ => now,
                };
                self.engine.schedule(msg, frame);
            } else if let Some(patch) = SynthNode::patch_event(&event) {
                filter_changed |= !matches!(patch, SynthNodePatch::Volume(_));
                self.params.apply(patch);
            }
        }
        if filter_changed {
            self.engine.set_filter(self.params.cutoff, self.params.resonance);
        }
    }

    fn process(
        &mut self,
        info: &ProcInfo,
        ProcBuffers { outputs, .. }: ProcBuffers,
        _extra: &mut ProcExtra,
    ) -> ProcessStatus {
        let frames = info.frames;
        let (left, rest) = outputs.split_at_mut(1);
        let left = &mut left[0][..frames];
        if !self.engine.render(info.clock_samples.0, left) {
            return ProcessStatus::ClearAllOutputs;
        }
        let amp = self.params.volume.amp();
        for s in left.iter_mut() {
            *s *= amp;
        }
        if let Some(right) = rest.first_mut() {
            right[..frames].copy_from_slice(left);
        }
        ProcessStatus::OutputsModified
    }
}

/// Queue `msg` for the synth on `node`, to start exactly at audio time `at`.
pub fn send_note(context: &mut AudioContext, node: &FirewheelNode, msg: NoteMsg, at: InstantSeconds) {
    let node_id = node.0;
    context.with(move |ctx| {
        ctx.queue_event(NodeEvent {
            node_id,
            time: Some(EventInstant::AtClockSeconds(at)),
            event: NodeEventType::CustomBytes(msg.encode()),
        });
    });
}

/// Plays notes on the [`SynthNode`] that shares the musician's entity.
#[derive(Default)]
pub struct SynthInstrument {
    next_id: u32,
    /// Semitones added to every note, to put a musician in this synth's register.
    pub transpose: f32,
}

impl SynthInstrument {
    pub fn transposed(semitones: f32) -> Self {
        Self { next_id: 0, transpose: semitones }
    }
}

impl Instrument for SynthInstrument {
    fn play(&mut self, note: &NoteEvent, at: f64, length: f64, cx: &mut PlayContext) {
        // The node gets an ID once the audio graph has picked it up; until then, rest.
        let Some(node) = cx.node else { return };
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let on = NoteMsg::On { id, midi: note.pitch + self.transpose, velocity: note.velocity * Volume::Decibels(note.gain_db).amp() };
        send_note(cx.audio, node, on, InstantSeconds(at));
        send_note(cx.audio, node, NoteMsg::Off { id }, InstantSeconds(at + length.max(0.01)));
    }
}

/// A musician playing a synth patch, `transpose` semitones from where the
/// player writes its notes: everything to spawn on one entity.
pub fn synth_musician(
    name: impl Into<String>,
    player: impl MusicPlayer + 'static,
    transpose: f32,
    synth: SynthNode,
    config: SynthConfig,
) -> impl Bundle {
    (Musician::new(name.into(), player).with_instrument(SynthInstrument::transposed(transpose)), synth, config)
}
