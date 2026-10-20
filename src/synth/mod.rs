//! Synthesised instruments: glicol voice graphs running inside Firewheel.
//!
//! Spawn a [`SynthNode`] (plus an optional [`SynthConfig`]) like any other
//! bevy_seedling node; it connects to the main bus. Mutating the `SynthNode`
//! component (cutoff, resonance, volume) is diffed to the audio thread.
//!
//! To let a [`Musician`](crate::musicians::Musician) play it, put both on the
//! same entity with a [`SynthInstrument`] — see [`synth_musician`]. Raw
//! timestamped notes can also be sent with [`send_note`].

pub mod dsp;
pub mod engine;
pub mod patch;

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
use bevy_seedling::prelude::{AudioContext, Connect, FirewheelNode, RegisterNode};

pub use engine::{NoteMsg, LATENCY};
pub use patch::{Adsr, FilterMod, FilterMode, Fm, OscSpec, SynthPatch, VoiceMode, Wave};
use engine::SynthEngine;

use crate::instrument::{Channels, Instrument, NoteEvent, PlayContext};
use crate::musicians::{MusicPlayer, Musician};
use crate::mix::{Mixer, Section};
use crate::situation::{Dim, MusicState};

/// Registers [`SynthNode`] with bevy_seedling.
pub struct SynthPlugin;

impl Plugin for SynthPlugin {
    fn build(&self, app: &mut App) {
        app.register_node::<SynthNode>()
            .add_observer(route_to_section)
            .add_systems(Update, respond_to_mood.run_if(resource_exists::<MusicState>));
    }
}

/// Live parameters of a synth instrument. Change them from any system; the
/// audio thread glides to new values, so per-frame changes never zipper.
#[derive(Diff, Patch, Debug, Clone, Component)]
pub struct SynthNode {
    pub volume: Volume,
    /// Base filter cutoff in Hz (envelope, key tracking and velocity move it per note).
    pub cutoff: f32,
    /// Filter resonance (Q).
    pub resonance: f32,
}

impl Default for SynthNode {
    fn default() -> Self {
        Self::for_patch(&SynthPatch::default())
    }
}

impl SynthNode {
    /// Unity volume, with the patch's own cutoff and resonance.
    pub fn for_patch(patch: &SynthPatch) -> Self {
        Self { volume: Volume::UNITY_GAIN, cutoff: patch.cutoff, resonance: patch.resonance }
    }

    pub fn volume_db(self, db: f32) -> Self {
        Self { volume: Volume::Decibels(db), ..self }
    }

    pub fn cutoff(self, cutoff: f32) -> Self {
        Self { cutoff, ..self }
    }
}

/// Construction-time settings: which patch, how many voices.
#[derive(Debug, Clone, PartialEq, Component)]
pub struct SynthConfig {
    pub patch: SynthPatch,
    /// Polyphony; mono and legato patches always use one voice.
    pub voices: usize,
}

impl Default for SynthConfig {
    fn default() -> Self {
        Self::new(SynthPatch::default())
    }
}

impl SynthConfig {
    pub fn new(patch: SynthPatch) -> Self {
        Self { patch, voices: 8 }
    }

    pub fn voices(self, voices: usize) -> Self {
        Self { voices, ..self }
    }
}

/// A synth node and its config for `patch`, at `volume_db`, ready to spawn.
pub fn synth(patch: SynthPatch, volume_db: f32) -> (SynthNode, SynthConfig) {
    (SynthNode::for_patch(&patch).volume_db(volume_db), SynthConfig::new(patch))
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
            engine: SynthEngine::new(&config.patch, config.voices, sr, self.cutoff, self.resonance),
            amp: self.volume.amp(),
            params: self.clone(),
            sr: sr as f64,
        })
    }
}

struct SynthProcessor {
    engine: SynthEngine,
    params: SynthNode,
    /// Gain applied at the end of the last block, ramped towards `params.volume`.
    amp: f32,
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
        // Ramp volume changes across the block instead of stepping them.
        let target = self.params.volume.amp();
        let step = (target - self.amp) / frames as f32;
        for s in left.iter_mut() {
            self.amp += step;
            *s *= self.amp;
        }
        self.amp = target;
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

/// Plays notes on a [`SynthNode`]: by default the one sharing the musician's
/// entity, or another entity's with [`SynthInstrument::on`].
#[derive(Default)]
pub struct SynthInstrument {
    next_id: u32,
    /// Semitones added to every note, to put a musician in this synth's register.
    pub transpose: f32,
    /// Play this entity's synth instead of the musician's own.
    pub target: Option<Entity>,
}

impl SynthInstrument {
    pub fn transposed(semitones: f32) -> Self {
        Self { transpose: semitones, ..default() }
    }

    /// Play the synth on `entity`.
    pub fn on(entity: Entity) -> Self {
        Self { target: Some(entity), ..default() }
    }

    pub fn transpose(self, semitones: f32) -> Self {
        Self { transpose: semitones, ..self }
    }
}

impl Instrument for SynthInstrument {
    fn play(&mut self, note: &NoteEvent, at: f64, length: f64, cx: &mut PlayContext) {
        // The node gets an ID once the audio graph has picked it up; until then, rest.
        let node = match self.target {
            Some(entity) => (cx.find_node)(entity),
            None => cx.node.copied(),
        };
        let Some(node) = node else { return };
        let node = &node;
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

/// A drum kit of synths: one synth per drum, each on its own entity, played
/// by `player` through [`NoteEvent::channel`] (channel *n* plays `drums[n]`).
/// Each drum is `(patch, midi pitch it should sound at, volume in dB)`.
/// Returns the musician's entity.
///
/// Players that write notes relative to [`crate::instrument::SAMPLE_ROOT`]
/// (like [`Drummer`](crate::musicians::drummer::Drummer)) are transposed so
/// each drum sounds at its pitch.
pub fn spawn_synth_kit(
    commands: &mut Commands,
    name: impl Into<String>,
    player: impl MusicPlayer + 'static,
    drums: Vec<(SynthPatch, f32, f32)>,
) -> Entity {
    spawn_synth_kit_with(commands, name, player, drums, |kit| Box::new(kit))
}

/// [`spawn_synth_kit`], with a chance to wrap the kit's instrument, e.g. in a
/// [`Sidechain`](crate::mix::Sidechain) so the kick pumps the pads.
/// The drum synths are children of the returned musician entity.
pub fn spawn_synth_kit_with(
    commands: &mut Commands,
    name: impl Into<String>,
    player: impl MusicPlayer + 'static,
    drums: Vec<(SynthPatch, f32, f32)>,
    wrap: impl FnOnce(Channels) -> Box<dyn Instrument>,
) -> Entity {
    let mut kit = Channels::default();
    let mut drum_entities = Vec::new();
    for (patch, pitch, volume_db) in drums {
        let name = Name::new(format!("synth drum: {}", patch.name));
        let mut config = SynthConfig::new(patch);
        config.voices = 2;
        let node = SynthNode::for_patch(&config.patch).volume_db(volume_db);
        let drum = commands.spawn((name, node, config)).id();
        drum_entities.push(drum);
        kit = kit.with(SynthInstrument::on(drum).transpose(pitch - crate::instrument::SAMPLE_ROOT));
    }
    let mut musician = Musician::new(name.into(), player);
    musician.instrument = wrap(kit);
    let musician = commands.spawn(musician).id();
    for drum in drum_entities {
        commands.entity(drum).insert(ChildOf(musician));
    }
    musician
}

/// When a [`Section`] is put on a synth musician (or a synth kit, whose drums
/// are its children), route its synth(s) into that section's mixer bus
/// instead of straight to the main bus.
fn route_to_section(
    insert: On<Insert, Section>,
    mixer: Option<Res<Mixer>>,
    sections: Query<&Section>,
    synths: Query<(), With<SynthNode>>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    let (Some(mixer), Ok(section)) = (mixer, sections.get(insert.entity)) else { return };
    let bus = mixer.bus(*section);
    let kids = children.get(insert.entity).map(|c| c.to_vec()).unwrap_or_default();
    for entity in std::iter::once(insert.entity).chain(kids) {
        if synths.contains(entity) {
            commands.entity(entity).connect(bus);
        }
    }
}

// ── Responding to the situation ───────────────────────────────────────────────

/// A [`SynthNode`] parameter a mood dimension can drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SynthParam {
    /// Hz; mapped exponentially, so equal steps sound equally brighter.
    Cutoff,
    Resonance,
    /// Decibels.
    VolumeDb,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Response {
    pub dim: Dim,
    pub param: SynthParam,
    /// Value at dimension 0.
    pub from: f32,
    /// Value at dimension 1.
    pub to: f32,
}

impl Response {
    pub fn value(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match self.param {
            SynthParam::Cutoff if self.from > 0.0 && self.to > 0.0 => self.from * (self.to / self.from).powf(x),
            _ => self.from + (self.to - self.from) * x,
        }
    }
}

/// Map mood dimensions (from [`MusicState`]) onto this entity's [`SynthNode`]:
/// `Responds::default().cutoff(Dim::Tension, 300.0, 4_000.0).volume_db(Dim::Intensity, -30.0, -8.0)`.
#[derive(Debug, Clone, Default, Component)]
pub struct Responds(pub Vec<Response>);

impl Responds {
    pub fn to(mut self, dim: Dim, param: SynthParam, from: f32, to: f32) -> Self {
        self.0.push(Response { dim, param, from, to });
        self
    }

    pub fn cutoff(self, dim: Dim, from: f32, to: f32) -> Self {
        self.to(dim, SynthParam::Cutoff, from, to)
    }

    pub fn resonance(self, dim: Dim, from: f32, to: f32) -> Self {
        self.to(dim, SynthParam::Resonance, from, to)
    }

    pub fn volume_db(self, dim: Dim, from: f32, to: f32) -> Self {
        self.to(dim, SynthParam::VolumeDb, from, to)
    }
}

fn respond_to_mood(state: Res<MusicState>, mut synths: Query<(&Responds, &mut SynthNode)>) {
    let mood = state.current();
    for (responds, mut synth) in &mut synths {
        for r in &responds.0 {
            let value = r.value(mood.get(r.dim));
            // Only touch the component on real changes: every write is diffed to the audio thread.
            match r.param {
                SynthParam::Cutoff if (synth.cutoff / value - 1.0).abs() > 0.005 => synth.cutoff = value,
                SynthParam::Resonance if (synth.resonance - value).abs() > 0.01 => synth.resonance = value,
                SynthParam::VolumeDb => {
                    let current = match synth.volume {
                        Volume::Decibels(db) => db,
                        other => 20.0 * other.amp().max(1e-6).log10(),
                    };
                    if (current - value).abs() > 0.1 {
                        synth.volume = Volume::Decibels(value);
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cutoff_responds_exponentially_others_linearly() {
        let cutoff = Response { dim: Dim::Tension, param: SynthParam::Cutoff, from: 250.0, to: 4_000.0 };
        assert!((cutoff.value(0.5) - 1_000.0).abs() < 1.0, "geometric midpoint");
        let vol = Response { dim: Dim::Intensity, param: SynthParam::VolumeDb, from: -30.0, to: -10.0 };
        assert_eq!(vol.value(0.5), -20.0);
        assert_eq!(vol.value(2.0), -10.0, "clamped");
    }
}
