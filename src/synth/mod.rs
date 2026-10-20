//! Synthesised instruments: glicol voice graphs running inside Firewheel.
//!
//! Spawn a [`SynthNode`] (plus an optional [`SynthConfig`]) like any other
//! bevy_seedling node; it connects to the main bus. Play it by queueing
//! timestamped [`NoteMsg`]s with [`send_note`]. Mutating the `SynthNode`
//! component (cutoff, resonance, volume) is diffed to the audio thread.

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
