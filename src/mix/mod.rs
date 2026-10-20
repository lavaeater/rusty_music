//! Mixing: a bus per [`Section`], shared reverb and echo sends, and ducking.
//!
//! ```text
//!  samples (section pool) ─┐
//!  synths (Section comp.) ─┴► section bus ─► ducker ─┬──────────────────────────► music bus ─► MainBus
//!                            (volume, pan)           ├► reverb send ─► Freeverb ─► return ─┘
//!                                                    └► echo send ──► Echo ─────► return ─┘
//! ```
//!
//! Add [`MixPlugin`] after [`MusicPlugin`](crate::MusicPlugin), put a
//! [`Section`] on each musician, and shape the sound through the [`Mix`]
//! resource. A musician without a `Section` bypasses the mixer, as before.
//!
//! Ducking comes in two kinds:
//! - [`Mix::duck_db`]: pull the whole music bus down (dialogue, cutscenes); it glides.
//! - [`Sidechain`]: sample-accurate "pumping" when an instrument plays, e.g.
//!   pads dipping on every kick.

pub mod nodes;

use bevy::prelude::*;
use bevy_seedling::pool::SamplerPool;
use bevy_seedling::prelude::*;

use crate::clock::Clock;
use crate::instrument::{Instrument, NoteEvent, PlayContext};
pub use nodes::{DuckNode, DuckTrigger, EchoNode};

/// Where a musician sits in the mix.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Drums,
    Bass,
    Harmony,
    Lead,
    /// Stingers and one-off effects.
    Fx,
}

impl Section {
    pub const ALL: [Section; 5] = [Section::Drums, Section::Bass, Section::Harmony, Section::Lead, Section::Fx];

    fn index(self) -> usize {
        self as usize
    }
}

/// Sample-player pool feeding a section's bus.
#[derive(PoolLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SectionPool(pub Section);

/// One section's channel strip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionMix {
    pub volume_db: f32,
    /// -1 (left) … 1 (right).
    pub pan: f32,
    /// Send level into the shared reverb; -60 dB or lower is off.
    pub reverb_db: f32,
    /// Send level into the shared echo; -60 dB or lower is off.
    pub echo_db: f32,
}

impl SectionMix {
    pub const fn new(volume_db: f32, reverb_db: f32, echo_db: f32) -> Self {
        Self { volume_db, pan: 0.0, reverb_db, echo_db }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReverbMix {
    /// 0 (small room) … 1 (near-endless hall).
    pub room_size: f32,
    /// 0 (dark) … 1 (bright).
    pub damping: f32,
    pub width: f32,
    pub return_db: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EchoMix {
    /// Delay in clock steps (16ths in 4/4): 3 = a dotted eighth. Follows tempo changes.
    pub steps: f32,
    pub feedback: f32,
    /// Low-pass on the repeats, in Hz.
    pub damping: f32,
    pub ping_pong: bool,
    pub return_db: f32,
}

/// The mix, as data. Change any field from a system; the audio graph follows.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct Mix {
    pub music_db: f32,
    /// Extra attenuation of the whole music bus (e.g. 12 under dialogue), gliding over `duck_time` seconds.
    pub duck_db: f32,
    pub duck_time: f32,
    pub sections: [SectionMix; 5],
    pub reverb: ReverbMix,
    pub echo: EchoMix,
}

impl Default for Mix {
    fn default() -> Self {
        Self {
            music_db: 0.0,
            duck_db: 0.0,
            duck_time: 0.4,
            sections: [
                SectionMix::new(0.0, -26.0, -80.0), // drums: a touch of room
                SectionMix::new(0.0, -80.0, -80.0), // bass: dry, keeps the low end tight
                SectionMix::new(0.0, -12.0, -80.0), // harmony: washed
                SectionMix::new(0.0, -15.0, -18.0), // lead: room plus echoes
                SectionMix::new(0.0, -10.0, -14.0), // fx: big and splashy
            ],
            reverb: ReverbMix { room_size: 0.75, damping: 0.5, width: 1.0, return_db: 0.0 },
            echo: EchoMix { steps: 3.0, feedback: 0.35, damping: 3_500.0, ping_pong: true, return_db: 0.0 },
        }
    }
}

impl Mix {
    pub fn section(&mut self, section: Section) -> &mut SectionMix {
        &mut self.sections[section.index()]
    }
}

impl Mix {
    /// The mix `t` (0–1) of the way from `self` to `other`. The dialogue duck
    /// is left alone: it is runtime state, not part of a scene.
    pub fn lerp(&self, other: &Mix, t: f32) -> Mix {
        let l = |a: f32, b: f32| a + (b - a) * t;
        let mut out = other.clone();
        out.music_db = l(self.music_db, other.music_db);
        out.duck_db = self.duck_db;
        out.duck_time = self.duck_time;
        for (o, (a, b)) in out.sections.iter_mut().zip(self.sections.iter().zip(&other.sections)) {
            *o = SectionMix { volume_db: l(a.volume_db, b.volume_db), pan: l(a.pan, b.pan), reverb_db: l(a.reverb_db, b.reverb_db), echo_db: l(a.echo_db, b.echo_db) };
        }
        let (ra, rb) = (self.reverb, other.reverb);
        out.reverb = ReverbMix { room_size: l(ra.room_size, rb.room_size), damping: l(ra.damping, rb.damping), width: l(ra.width, rb.width), return_db: l(ra.return_db, rb.return_db) };
        let (ea, eb) = (self.echo, other.echo);
        out.echo = EchoMix {
            steps: if t < 0.5 { ea.steps } else { eb.steps },
            feedback: l(ea.feedback, eb.feedback),
            damping: l(ea.damping, eb.damping),
            ping_pong: if t < 0.5 { ea.ping_pong } else { eb.ping_pong },
            return_db: l(ea.return_db, eb.return_db),
        };
        out
    }

    fn distance(&self, other: &Mix) -> f32 {
        let mut d = (self.music_db - other.music_db).abs() + (self.reverb.room_size - other.reverb.room_size).abs() * 40.0;
        for (a, b) in self.sections.iter().zip(&other.sections) {
            d += (a.volume_db - b.volume_db).abs() + (a.reverb_db - b.reverb_db).abs() + (a.echo_db - b.echo_db).abs() + (a.pan - b.pan).abs() * 10.0;
        }
        d + (self.reverb.return_db - other.reverb.return_db).abs() + (self.echo.return_db - other.echo.return_db).abs() + (self.echo.feedback - other.echo.feedback).abs() * 20.0
    }
}

/// Named mixes for places or moments ("cave", "garden", "boss"). Set
/// `active` and the [`Mix`] glides to that scene over `glide` seconds.
#[derive(Resource, Debug, Clone, Default)]
pub struct MixScenes {
    pub scenes: std::collections::HashMap<String, Mix>,
    pub active: Option<String>,
    pub glide: f32,
}

impl MixScenes {
    pub fn with(mut self, name: impl Into<String>, mix: Mix) -> Self {
        self.scenes.insert(name.into(), mix);
        self
    }

    pub fn activate(&mut self, name: &str) {
        if self.active.as_deref() != Some(name) {
            self.active = Some(name.to_string());
        }
    }
}

fn glide_to_scene(scenes: Option<Res<MixScenes>>, time: Res<Time>, mut mix: ResMut<Mix>) {
    let Some(scenes) = scenes else { return };
    let Some(target) = scenes.active.as_ref().and_then(|name| scenes.scenes.get(name)) else { return };
    if mix.distance(target) < 0.05 {
        return;
    }
    let k = if scenes.glide <= 0.0 { 1.0 } else { 1.0 - (-3.0 * time.delta_secs() / scenes.glide).exp() };
    let next = mix.lerp(target, k);
    // Land exactly once close, so change detection goes quiet again.
    *mix = if next.distance(target) < 0.05 { Mix { duck_db: mix.duck_db, duck_time: mix.duck_time, ..target.clone() } } else { next };
}

/// The mixer's audio-graph entities, for routing extra nodes or reading meters.
#[derive(Resource, Debug, Clone)]
pub struct Mixer {
    pub music: Entity,
    pub reverb: Entity,
    pub echo: Entity,
    reverb_return: Entity,
    echo_return: Entity,
    buses: [Entity; 5],
    duckers: [Entity; 5],
    reverb_sends: [Entity; 5],
    echo_sends: [Entity; 5],
}

impl Mixer {
    /// The section's input: connect sources here.
    pub fn bus(&self, section: Section) -> Entity {
        self.buses[section.index()]
    }

    /// The section's [`DuckNode`], for [`Sidechain`] targets.
    pub fn ducker(&self, section: Section) -> Entity {
        self.duckers[section.index()]
    }
}

pub struct MixPlugin;

impl Plugin for MixPlugin {
    fn build(&self, app: &mut App) {
        app.register_node::<DuckNode>().register_node::<EchoNode>().init_resource::<Mix>();

        let world = app.world_mut();
        let mixer = {
            let mut commands = world.commands();
            let music = commands.spawn((Name::new("music bus"), VolumeNode::default())).connect(MainBus).head();
            let reverb_return = commands.spawn((Name::new("reverb return"), VolumeNode::default())).connect(music).head();
            let reverb = commands.spawn((Name::new("reverb"), FreeverbNode::default())).connect(reverb_return).head();
            let echo_return = commands.spawn((Name::new("echo return"), VolumeNode::default())).connect(music).head();
            let echo = commands.spawn((Name::new("echo"), EchoNode::default())).connect(echo_return).head();

            let mut strip = |section: Section| {
                let name = |what: &str| Name::new(format!("{section:?} {what}"));
                let ducker = commands.spawn((name("ducker"), DuckNode::default())).connect(music).head();
                let bus = commands.spawn((name("bus"), VolumePanNode::default())).connect(ducker).head();
                let reverb_send = commands.spawn((name("reverb send"), VolumeNode::default())).connect(reverb).head();
                let echo_send = commands.spawn((name("echo send"), VolumeNode::default())).connect(echo).head();
                commands.entity(ducker).connect(reverb_send);
                commands.entity(ducker).connect(echo_send);
                commands.spawn((name("samples"), SamplerPool(SectionPool(section)))).connect(bus);
                (bus, ducker, reverb_send, echo_send)
            };
            let strips = Section::ALL.map(&mut strip);
            Mixer {
                music,
                reverb,
                echo,
                reverb_return,
                echo_return,
                buses: strips.map(|s| s.0),
                duckers: strips.map(|s| s.1),
                reverb_sends: strips.map(|s| s.2),
                echo_sends: strips.map(|s| s.3),
            }
        };
        world.flush();
        app.insert_resource(mixer).add_systems(Update, (glide_to_scene, apply_mix).chain());
    }
}

fn db(db: f32) -> Volume {
    if db <= -60.0 { Volume::SILENT } else { Volume::Decibels(db) }
}

/// Push [`Mix`] into the audio graph; glide the music-bus duck; keep echo in tempo.
#[allow(clippy::too_many_arguments)]
fn apply_mix(
    mix: Res<Mix>,
    mixer: Res<Mixer>,
    clock: Res<Clock>,
    time: Res<Time>,
    mut ducked: Local<f32>,
    mut last_step: Local<f32>,
    mut volumes: Query<&mut VolumeNode>,
    mut pans: Query<&mut VolumePanNode>,
    mut reverb: Query<&mut FreeverbNode>,
    mut echo: Query<&mut EchoNode>,
) {
    let duck_moving = (*ducked - mix.duck_db).abs() > 0.01;
    let tempo_changed = *last_step != clock.beat_length;
    if !mix.is_changed() && !duck_moving && !tempo_changed {
        return;
    }
    // Glide the duck at a constant rate in dB: `duck_time` to fully apply it.
    let rate = mix.duck_db.abs().max(6.0) / mix.duck_time.max(0.01) * time.delta_secs();
    *ducked += (mix.duck_db - *ducked).clamp(-rate, rate);
    *last_step = clock.beat_length;

    let mut set = |entity: Entity, volume: Volume| {
        if let Ok(mut node) = volumes.get_mut(entity)
            && node.volume != volume
        {
            node.volume = volume;
        }
    };
    set(mixer.music, db(mix.music_db - *ducked));
    set(mixer.reverb_return, db(mix.reverb.return_db));
    set(mixer.echo_return, db(mix.echo.return_db));
    for section in Section::ALL {
        let strip = mix.sections[section.index()];
        set(mixer.reverb_sends[section.index()], db(strip.reverb_db));
        set(mixer.echo_sends[section.index()], db(strip.echo_db));
        if let Ok(mut bus) = pans.get_mut(mixer.bus(section))
            && (bus.volume != db(strip.volume_db) || bus.pan != strip.pan)
        {
            bus.volume = db(strip.volume_db);
            bus.pan = strip.pan.clamp(-1.0, 1.0);
        }
    }
    if let Ok(mut r) = reverb.get_mut(mixer.reverb) {
        let ReverbMix { room_size, damping, width, .. } = mix.reverb;
        if r.room_size != room_size || r.damping != damping || r.width != width {
            r.room_size = room_size;
            r.damping = damping;
            r.width = width;
        }
    }
    if let Ok(mut e) = echo.get_mut(mixer.echo) {
        let target = EchoNode {
            time: (mix.echo.steps * clock.beat_length).min(nodes::MAX_ECHO),
            feedback: mix.echo.feedback,
            damping: mix.echo.damping,
            ping_pong: mix.echo.ping_pong,
        };
        if (e.time - target.time).abs() > 1e-4 || e.feedback != target.feedback || e.damping != target.damping || e.ping_pong != target.ping_pong {
            *e = target;
        }
    }
}

// ── Sidechain ────────────────────────────────────────────────────────────────

/// Wraps an instrument so that its notes also duck other sections, exactly
/// on time: the classic pad-pumping-on-the-kick.
///
/// ```ignore
/// let kit = Sidechain::new(kit).channel(0).ducks(&mixer, Section::Harmony, 8.0, 0.25);
/// ```
pub struct Sidechain {
    inner: Box<dyn Instrument>,
    channel: Option<usize>,
    targets: Vec<(Entity, DuckTrigger)>,
}

impl Sidechain {
    pub fn new(inner: impl Instrument + 'static) -> Self {
        Self { inner: Box::new(inner), channel: None, targets: Vec::new() }
    }

    /// Only notes on this [`NoteEvent::channel`] trigger ducking (e.g. the kick in a kit).
    pub fn channel(self, channel: usize) -> Self {
        Self { channel: Some(channel), ..self }
    }

    /// Duck `section` by `depth_db`, recovering over `release` seconds.
    pub fn ducks(mut self, mixer: &Mixer, section: Section, depth_db: f32, release: f32) -> Self {
        self.targets.push((mixer.ducker(section), DuckTrigger { depth_db, release }));
        self
    }
}

impl Instrument for Sidechain {
    fn play(&mut self, note: &NoteEvent, at: f64, length: f64, cx: &mut PlayContext) {
        self.inner.play(note, at, length, cx);
        if self.channel.is_some_and(|c| c != note.channel) {
            return;
        }
        for (ducker, trigger) in &self.targets {
            if let Some(node) = (cx.find_node)(*ducker) {
                // Louder notes pump harder.
                let t = DuckTrigger { depth_db: trigger.depth_db * note.velocity.clamp(0.0, 1.0), ..*trigger };
                t.send(cx.audio, &node, at);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenes_lerp_and_keep_the_duck() {
        let open = Mix::default();
        let mut cave = Mix::default();
        cave.reverb.room_size = 0.95;
        cave.section(Section::Drums).reverb_db = -6.0;
        let mut now = open.clone();
        now.duck_db = 12.0;
        let half = now.lerp(&cave, 0.5);
        assert!((half.reverb.room_size - 0.85).abs() < 1e-6);
        assert!((half.sections[0].reverb_db - -16.0).abs() < 1e-6);
        assert_eq!(half.duck_db, 12.0, "dialogue duck is not part of a scene");
        assert!(now.lerp(&cave, 1.0).distance(&cave) < 1e-6);
    }
}
