//! Two small Firewheel nodes the mixer needs that Firewheel doesn't have:
//! a sample-accurate sidechain [`DuckNode`] and a tempo-syncable [`EchoNode`].
//! Each wraps a plain-Rust DSP core ([`Ducker`], [`Echo`]) that is unit-tested
//! without an audio device.

use bevy::prelude::*;
use firewheel::{
    channel_config::ChannelConfig,
    clock::{EventInstant, InstantSeconds},
    diff::{Diff, Patch},
    event::{NodeEvent, NodeEventType, ProcEvents},
    node::{
        AudioNode, AudioNodeInfo, AudioNodeProcessor, ConstructProcessorContext, NodeError,
        ProcBuffers, ProcExtra, ProcInfo, ProcessStatus,
    },
};
use bevy_seedling::prelude::{AudioContext, FirewheelNode};

/// These nodes have nothing to configure at construction time.
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct NodeConfig;

// ── Ducker ───────────────────────────────────────────────────────────────────

/// Gain envelope that dips on each trigger and recovers: sidechain "pumping".
///
/// The envelope runs in decibels, so the recovery sounds even: a 12 dB dip
/// spends as long climbing from -12 to -6 dB as from -6 to 0.
#[derive(Debug, Clone)]
pub struct Ducker {
    sr: f32,
    /// Depth of the current dip as a natural-log gain (dB · ln10 / 20).
    depth: f32,
    /// Envelope 0–1: 1 = fully ducked.
    env: f32,
    attack_step: f32,
    release_k: f32,
    attacking: bool,
}

impl Ducker {
    /// Attack is fixed and fast (4 ms): long enough not to click.
    const ATTACK: f32 = 0.004;

    pub fn new(sr: f32) -> Self {
        Self { sr, depth: 0.0, env: 0.0, attack_step: 1.0 / (Self::ATTACK * sr), release_k: 0.0, attacking: false }
    }

    /// Dip by `depth_db` (positive dB), recovering over `release` seconds.
    pub fn trigger(&mut self, depth_db: f32, release: f32) {
        self.depth = depth_db.abs() * std::f32::consts::LN_10 / 20.0;
        self.release_k = 1.0 - (-4.0 / (release.max(0.01) * self.sr)).exp();
        self.attacking = true;
    }

    pub fn is_idle(&self) -> bool {
        !self.attacking && self.env < 1e-4
    }

    /// The gain for the next sample.
    pub fn next_gain(&mut self) -> f32 {
        if self.attacking {
            self.env += self.attack_step;
            if self.env >= 1.0 {
                self.env = 1.0;
                self.attacking = false;
            }
        } else {
            self.env -= self.env * self.release_k;
        }
        (-self.depth * self.env).exp()
    }
}

/// A trigger for [`DuckNode`], packed into Firewheel's custom event bytes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DuckTrigger {
    pub depth_db: f32,
    pub release: f32,
}

impl DuckTrigger {
    fn encode(self) -> [u8; 36] {
        let mut b = [0u8; 36];
        b[0] = 0xD7;
        b[1..5].copy_from_slice(&self.depth_db.to_le_bytes());
        b[5..9].copy_from_slice(&self.release.to_le_bytes());
        b
    }

    fn decode(b: &[u8; 36]) -> Option<Self> {
        (b[0] == 0xD7).then(|| Self {
            depth_db: f32::from_le_bytes(b[1..5].try_into().unwrap()),
            release: f32::from_le_bytes(b[5..9].try_into().unwrap()),
        })
    }

    /// Duck the [`DuckNode`] on `node` exactly at audio time `at`.
    pub fn send(self, context: &mut AudioContext, node: &FirewheelNode, at: f64) {
        let node_id = node.0;
        context.with(move |ctx| {
            ctx.queue_event(NodeEvent {
                node_id,
                time: Some(EventInstant::AtClockSeconds(InstantSeconds(at))),
                event: NodeEventType::CustomBytes(self.encode()),
            });
        });
    }
}

/// Stereo gain that dips when triggered by timestamped [`DuckTrigger`]s.
/// Passes audio through untouched while idle.
#[derive(Diff, Patch, Debug, Clone, Default, Component)]
pub struct DuckNode {
    /// Unused; Firewheel nodes need at least one parameter to diff.
    pub enabled: bool,
}

impl AudioNode for DuckNode {
    type Configuration = NodeConfig;

    fn info(&self, _: &NodeConfig) -> Result<AudioNodeInfo, NodeError> {
        Ok(AudioNodeInfo::new()
            .debug_name("rusty_music ducker")
            .channel_config(ChannelConfig { num_inputs: 2.into(), num_outputs: 2.into() }))
    }

    fn construct_processor(&self, _: &NodeConfig, cx: ConstructProcessorContext) -> Result<impl AudioNodeProcessor, NodeError> {
        let sr = cx.stream_info.sample_rate.get();
        Ok(DuckProcessor { ducker: Ducker::new(sr as f32), sr: sr as f64, pending: [None; MAX_PENDING_DUCKS] })
    }
}

/// Triggers waiting for their frame; more than this many in flight are applied early.
const MAX_PENDING_DUCKS: usize = 8;

struct DuckProcessor {
    ducker: Ducker,
    sr: f64,
    /// (absolute frame, trigger), unordered.
    pending: [Option<(i64, DuckTrigger)>; MAX_PENDING_DUCKS],
}

impl AudioNodeProcessor for DuckProcessor {
    fn events(&mut self, info: &ProcInfo, events: &mut ProcEvents, _extra: &mut ProcExtra) {
        let now = info.clock_samples.0;
        for (event, time) in events.drain_with_timestamps() {
            let NodeEventType::CustomBytes(bytes) = &event else { continue };
            let Some(trigger) = DuckTrigger::decode(bytes) else { continue };
            let frame = match time {
                Some(EventInstant::AtClockSeconds(s)) => ((s.0 * self.sr).round() as i64).max(now),
                _ => now,
            };
            match self.pending.iter_mut().find(|p| p.is_none()) {
                Some(slot) => *slot = Some((frame, trigger)),
                None => self.ducker.trigger(trigger.depth_db, trigger.release),
            }
        }
    }

    fn process(&mut self, info: &ProcInfo, ProcBuffers { inputs, outputs }: ProcBuffers, _extra: &mut ProcExtra) -> ProcessStatus {
        let start = info.clock_samples.0;
        let end = start + info.frames as i64;
        let due_now = self.pending.iter().flatten().any(|(f, _)| *f < end);
        if self.ducker.is_idle() && !due_now {
            return ProcessStatus::Bypass;
        }
        for i in 0..info.frames {
            if due_now {
                let frame = start + i as i64;
                for slot in &mut self.pending {
                    if let Some((f, t)) = *slot
                        && f <= frame
                    {
                        self.ducker.trigger(t.depth_db, t.release);
                        *slot = None;
                    }
                }
            }
            let gain = self.ducker.next_gain();
            for (out, input) in outputs.iter_mut().zip(inputs.iter()) {
                out[i] = input[i] * gain;
            }
        }
        ProcessStatus::OutputsModified
    }
}

// ── Echo ─────────────────────────────────────────────────────────────────────

/// Longest supported echo, in seconds.
pub const MAX_ECHO: f32 = 2.0;

/// Stereo feedback delay with damping and optional ping-pong. Wet only.
#[derive(Debug, Clone)]
pub struct Echo {
    sr: f32,
    buf: [Vec<f32>; 2],
    write: usize,
    /// Current delay in samples, gliding towards the target.
    delay: f32,
    lp: [f32; 2],
}

impl Echo {
    pub fn new(sr: f32, time: f32) -> Self {
        let len = (MAX_ECHO * sr) as usize + 2;
        Self { sr, buf: [vec![0.0; len], vec![0.0; len]], write: 0, delay: time * sr, lp: [0.0; 2] }
    }

    fn read(&self, ch: usize, delay: f32) -> f32 {
        let len = self.buf[ch].len();
        let pos = self.write as f32 + len as f32 - delay;
        let i = pos.floor() as usize;
        let frac = pos.fract();
        let a = self.buf[ch][i % len];
        let b = self.buf[ch][(i + 1) % len];
        a + (b - a) * frac
    }

    /// Process one stereo frame with the given parameters; returns the wet signal.
    pub fn tick(&mut self, input: [f32; 2], p: &EchoNode) -> [f32; 2] {
        let target = (p.time.clamp(0.001, MAX_ECHO) * self.sr).max(1.0);
        // Glide delay-time changes (tempo changes) over ~50 ms: a short pitch bend, no clicks.
        self.delay += (target - self.delay) * (1.0 - (-1.0 / (0.05 * self.sr)).exp());
        let wet = [self.read(0, self.delay), self.read(1, self.delay)];
        // One-pole low-pass on the feedback path: each repeat darker than the last.
        let k = 1.0 - (-std::f32::consts::TAU * p.damping.clamp(200.0, 20_000.0) / self.sr).exp();
        for (ch, w) in wet.iter().enumerate() {
            self.lp[ch] += (w - self.lp[ch]) * k;
        }
        let fb = p.feedback.clamp(0.0, 0.95);
        let (to_l, to_r) = if p.ping_pong {
            // Mono in on the left, repeats bounce between the sides.
            ((input[0] + input[1]) * 0.5 + self.lp[1] * fb, self.lp[0] * fb)
        } else {
            (input[0] + self.lp[0] * fb, input[1] + self.lp[1] * fb)
        };
        let len = self.buf[0].len();
        self.buf[0][self.write] = to_l;
        self.buf[1][self.write] = to_r;
        self.write = (self.write + 1) % len;
        wet
    }
}

/// A tempo-syncable echo send. Wet only: feed it from a send level and mix it back in.
#[derive(Diff, Patch, Debug, Clone, Component)]
pub struct EchoNode {
    /// Delay time in seconds (at most [`MAX_ECHO`]). The mixer keeps it in time with the clock.
    pub time: f32,
    /// 0–0.95: how much of each repeat feeds the next.
    pub feedback: f32,
    /// Low-pass cutoff (Hz) applied to the repeats.
    pub damping: f32,
    pub ping_pong: bool,
}

impl Default for EchoNode {
    fn default() -> Self {
        Self { time: 0.375, feedback: 0.35, damping: 3_500.0, ping_pong: true }
    }
}

impl AudioNode for EchoNode {
    type Configuration = NodeConfig;

    fn info(&self, _: &NodeConfig) -> Result<AudioNodeInfo, NodeError> {
        Ok(AudioNodeInfo::new()
            .debug_name("rusty_music echo")
            .channel_config(ChannelConfig { num_inputs: 2.into(), num_outputs: 2.into() }))
    }

    fn construct_processor(&self, _: &NodeConfig, cx: ConstructProcessorContext) -> Result<impl AudioNodeProcessor, NodeError> {
        let sr = cx.stream_info.sample_rate.get() as f32;
        Ok(EchoProcessor { echo: Echo::new(sr, self.time), params: self.clone(), quiet_frames: usize::MAX, sr })
    }
}

struct EchoProcessor {
    echo: Echo,
    params: EchoNode,
    /// Frames since the input was last non-silent; the tail rings for a while after.
    quiet_frames: usize,
    sr: f32,
}

impl AudioNodeProcessor for EchoProcessor {
    fn events(&mut self, _info: &ProcInfo, events: &mut ProcEvents, _extra: &mut ProcExtra) {
        for patch in events.drain_patches::<EchoNode>() {
            self.params.apply(patch);
        }
    }

    fn process(&mut self, info: &ProcInfo, ProcBuffers { inputs, outputs }: ProcBuffers, _extra: &mut ProcExtra) -> ProcessStatus {
        if info.in_silence_mask.all_channels_silent(2) {
            self.quiet_frames = self.quiet_frames.saturating_add(info.frames);
        } else {
            self.quiet_frames = 0;
        }
        // Repeats decay by `feedback` per echo; after this long they are inaudible.
        let fb = self.params.feedback.clamp(0.01, 0.95);
        let repeats = (-80.0 / (20.0 * fb.log10())).ceil();
        let tail = (repeats * self.params.time.max(0.01) * self.sr) as usize + info.frames;
        if self.quiet_frames > tail {
            return ProcessStatus::ClearAllOutputs;
        }
        let (left, right) = outputs.split_at_mut(1);
        for i in 0..info.frames {
            let wet = self.echo.tick([inputs[0][i], inputs[1][i]], &self.params);
            left[0][i] = wet[0];
            right[0][i] = wet[1];
        }
        ProcessStatus::OutputsModified
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    #[test]
    fn ducker_dips_and_recovers() {
        let mut d = Ducker::new(SR);
        assert_eq!(d.next_gain(), 1.0);
        d.trigger(12.0, 0.2);
        let gains: Vec<f32> = (0..SR as usize).map(|_| d.next_gain()).collect();
        let floor = gains.iter().cloned().fold(1.0, f32::min);
        assert!((floor - 10f32.powf(-12.0 / 20.0)).abs() < 0.01, "dips 12 dB: {floor}");
        let lowest = gains.iter().position(|g| *g == floor).unwrap();
        assert!((190..=200).contains(&lowest), "4 ms attack, got {lowest} samples");
        assert!(gains[(0.25 * SR) as usize] > 0.95, "recovered after the release");
        // Recovery is even in dB: halfway (-6 dB) takes a real fraction of the release, not a blip.
        let half = gains.iter().skip(lowest).position(|g| *g > 10f32.powf(-6.0 / 20.0)).unwrap();
        assert!(half as f32 > 0.03 * SR, "-12 → -6 dB took only {half} samples");
        assert!(gains.windows(2).all(|w| (w[1] - w[0]).abs() < 0.01), "no clicks");
        assert!(d.is_idle());
    }

    #[test]
    fn duck_trigger_roundtrip() {
        let t = DuckTrigger { depth_db: 9.0, release: 0.15 };
        assert_eq!(DuckTrigger::decode(&t.encode()), Some(t));
        assert_eq!(DuckTrigger::decode(&[0; 36]), None);
    }

    #[test]
    fn echo_repeats_on_time_and_decays() {
        let p = EchoNode { time: 0.1, feedback: 0.5, damping: 20_000.0, ping_pong: false };
        let mut e = Echo::new(SR, p.time);
        let out: Vec<[f32; 2]> = (0..(SR * 0.5) as usize).map(|n| e.tick(if n == 0 { [1.0, 1.0] } else { [0.0; 2] }, &p)).collect();
        let peak_near = |t: f32| {
            let c = (t * SR) as usize;
            out[c - 10..c + 10].iter().map(|s| s[0].abs()).fold(0.0, f32::max)
        };
        assert!(peak_near(0.05) < 1e-6, "nothing before the first repeat");
        let (first, second) = (peak_near(0.1), peak_near(0.2));
        assert!(first > 0.9, "first repeat at 100 ms: {first}");
        assert!((second / first - 0.5).abs() < 0.05, "second repeat at half level: {second}");
    }

    #[test]
    fn ping_pong_alternates_sides() {
        let p = EchoNode { time: 0.1, feedback: 0.5, damping: 20_000.0, ping_pong: true };
        let mut e = Echo::new(SR, p.time);
        let out: Vec<[f32; 2]> = (0..(SR * 0.35) as usize).map(|n| e.tick(if n == 0 { [1.0, 1.0] } else { [0.0; 2] }, &p)).collect();
        let at = |t: f32, ch: usize| out[(t * SR) as usize][ch].abs();
        assert!(at(0.1, 0) > 0.5 && at(0.1, 1) < 1e-3, "first repeat left");
        assert!(at(0.2, 1) > 0.25 && at(0.2, 0) < 1e-3, "second repeat right");
        assert!(at(0.3, 0) > 0.1 && at(0.3, 1) < 1e-3, "third back left");
    }
}
