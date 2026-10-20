//! A polyphonic synth engine built from `glicol_synth` graphs.
//!
//! This half knows nothing about Firewheel or Bevy, so it can be rendered
//! offline in tests. Each [`Voice`] is a small glicol `AudioContext` whose
//! control inputs (frequency, gate, velocity) are fed from [`Lane`]s that the
//! engine writes sample-accurately before every glicol block.
//!
//! Glicol renders fixed blocks of [`BLOCK`] frames while Firewheel hands us
//! arbitrary sub-chunks, so the engine keeps one rendered block and runs
//! [`LATENCY`] frames behind the clock. Any event that arrives on time can
//! therefore still be applied on its exact frame.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use glicol_synth::envelope::Adsr;
use glicol_synth::filter::ResonantLowPassFilter;
use glicol_synth::operator::{Add, Mul};
use glicol_synth::oscillator::{SawOsc, SinOsc, SquOsc, TriOsc};
use glicol_synth::{AudioContext, AudioContextBuilder, Buffer, Input, Message, Node};
use hashbrown::HashMap;

/// Glicol block size in frames.
pub const BLOCK: usize = 128;
/// Constant output latency in frames, so on-time events are never already rendered.
pub const LATENCY: i64 = BLOCK as i64;
/// Upper bound on queued note events; more are dropped instead of allocating.
const MAX_PENDING: usize = 256;

// ── Control lanes ──────────────────────────────────────────────────────────────

/// One block of per-sample control values, shared between the engine (writer)
/// and a [`LaneNode`] inside the glicol graph (reader). Both live on the audio
/// thread; atomics just make the sharing `Send + Sync` without locks.
pub struct Lane([AtomicU32; BLOCK]);

impl Lane {
    fn new() -> Arc<Self> {
        Arc::new(Self(std::array::from_fn(|_| AtomicU32::new(0))))
    }

    fn fill(&self, from: usize, value: f32) {
        for cell in &self.0[from..] {
            cell.store(value.to_bits(), Ordering::Relaxed);
        }
    }

    fn set(&self, at: usize, value: f32) {
        self.0[at].store(value.to_bits(), Ordering::Relaxed);
    }

    fn get(&self, at: usize) -> f32 {
        f32::from_bits(self.0[at].load(Ordering::Relaxed))
    }
}

/// Glicol node that outputs a [`Lane`].
struct LaneNode(Arc<Lane>);

impl Node<BLOCK> for LaneNode {
    fn process(&mut self, _inputs: &mut HashMap<usize, Input<BLOCK>>, output: &mut [Buffer<BLOCK>]) {
        for out in output.iter_mut() {
            for (i, sample) in out.iter_mut().enumerate() {
                *sample = self.0.get(i);
            }
        }
    }

    fn send_msg(&mut self, _info: Message) {}
}

// ── Patches ──────────────────────────────────────────────────────────────────

/// The built-in voice patches. Each one is a glicol graph wired to the lanes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SynthPatch {
    /// Two detuned saws through a resonant low-pass.
    #[default]
    SawLead,
    /// Square pluck with a fast decay.
    Pluck,
    /// Sine + triangle sub bass.
    SubBass,
    /// Three detuned saws, slow attack and release.
    Pad,
}

struct PatchSpec {
    /// (attack, decay, sustain, release) in seconds / level.
    adsr: (f32, f32, f32, f32),
    gain: f32,
}

impl SynthPatch {
    fn spec(self) -> PatchSpec {
        match self {
            SynthPatch::SawLead => PatchSpec { adsr: (0.005, 0.2, 0.6, 0.15), gain: 0.25 },
            SynthPatch::Pluck => PatchSpec { adsr: (0.002, 0.25, 0.0, 0.1), gain: 0.3 },
            SynthPatch::SubBass => PatchSpec { adsr: (0.004, 0.1, 0.8, 0.08), gain: 0.5 },
            SynthPatch::Pad => PatchSpec { adsr: (0.4, 0.5, 0.7, 1.2), gain: 0.15 },
        }
    }

    /// Frames a released voice keeps sounding before it can be skipped.
    fn release_frames(self, sr: usize) -> i64 {
        (self.spec().adsr.3 * sr as f32) as i64 + BLOCK as i64
    }
}

/// Tag under which a voice graph registers its filter node.
const FILTER: &str = "filter";

/// Wire `patch` into a fresh glicol context driven by the given lanes.
fn build_graph(patch: SynthPatch, sr: usize, freq: &Arc<Lane>, gate: &Arc<Lane>, vel: &Arc<Lane>, cutoff: f32, q: f32) -> AudioContext<BLOCK> {
    let mut ctx = AudioContextBuilder::<BLOCK>::new().sr(sr).channels(1).max_nodes(32).max_edges(48).build();
    let freq_in = ctx.add_mono_node(LaneNode(freq.clone()));
    let gate_in = ctx.add_mono_node(LaneNode(gate.clone()));
    let vel_in = ctx.add_mono_node(LaneNode(vel.clone()));

    // An oscillator following the frequency lane, optionally detuned by `ratio`.
    let osc = |ctx: &mut AudioContext<BLOCK>, kind: u8, ratio: f32| {
        let pitch = ctx.add_mono_node(Mul::new(ratio));
        let node = match kind {
            0 => ctx.add_mono_node(SawOsc::new().sr(sr)),
            1 => ctx.add_mono_node(SquOsc::new().sr(sr)),
            2 => ctx.add_mono_node(SinOsc::new().sr(sr)),
            _ => ctx.add_mono_node(TriOsc::new().sr(sr)),
        };
        ctx.connect(freq_in, pitch);
        ctx.connect(pitch, node);
        node
    };
    let sources = match patch {
        SynthPatch::SawLead => vec![osc(&mut ctx, 0, 1.0), osc(&mut ctx, 0, 1.006)],
        SynthPatch::Pluck => vec![osc(&mut ctx, 1, 1.0)],
        SynthPatch::SubBass => vec![osc(&mut ctx, 2, 1.0), osc(&mut ctx, 3, 0.5)],
        SynthPatch::Pad => vec![osc(&mut ctx, 0, 0.993), osc(&mut ctx, 0, 1.0), osc(&mut ctx, 0, 1.007)],
    };
    // Sum the oscillators pairwise with glicol's two-input `add`.
    let mut mix = sources[0];
    for &src in &sources[1..] {
        let add = ctx.add_mono_node(Add::new(0.0));
        ctx.connect(mix, add);
        ctx.connect(src, add);
        mix = add;
    }

    let spec = patch.spec();
    let (a, d, s, r) = spec.adsr;
    let env = ctx.add_mono_node(Adsr::new().attack(a).decay(d).sustain(s).release(r).sr(sr));
    ctx.connect(gate_in, env);
    let amp = ctx.add_mono_node(Mul::new(0.0));
    ctx.connect(mix, amp);
    ctx.connect(env, amp);

    let filter = ctx.add_mono_node(ResonantLowPassFilter::new().cutoff(cutoff).q(q).sr(sr));
    ctx.connect(amp, filter);
    ctx.tags.insert(FILTER, filter);

    let velocity = ctx.add_mono_node(Mul::new(0.0));
    ctx.connect(filter, velocity);
    ctx.connect(vel_in, velocity);
    let gain = ctx.add_mono_node(Mul::new(spec.gain / sources.len() as f32));
    ctx.connect(velocity, gain);
    ctx.connect(gain, ctx.destination);

    ctx
}

// ── Voices ───────────────────────────────────────────────────────────────────

/// A note command, as sent from the game to the synth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NoteMsg {
    On { id: u32, midi: f32, velocity: f32 },
    Off { id: u32 },
    AllOff,
}

fn midi_to_hz(midi: f32) -> f32 {
    440.0 * 2f32.powf((midi - 69.0) / 12.0)
}

struct Voice {
    graph: AudioContext<BLOCK>,
    freq: Arc<Lane>,
    gate: Arc<Lane>,
    vel: Arc<Lane>,
    note: Option<u32>,
    gate_on: bool,
    hz: f32,
    velocity: f32,
    /// Increasing counter of when the voice was (re)started, for stealing.
    started: u64,
    /// Absolute frame after which a released voice is silent.
    silent_after: i64,
}

impl Voice {
    fn is_idle(&self, frame: i64) -> bool {
        !self.gate_on && frame > self.silent_after
    }
}

#[derive(Clone, Copy)]
struct Pending {
    frame: i64,
    msg: NoteMsg,
}

/// The audio-thread synth: a fixed voice pool plus the block adapter.
pub struct SynthEngine {
    patch: SynthPatch,
    sr: usize,
    voices: Vec<Voice>,
    pending: Vec<Pending>,
    block: [f32; BLOCK],
    /// Absolute frame of `block[0]`.
    block_start: i64,
    /// Frame we expect the next `render` call to start at.
    next_frame: Option<i64>,
    starts: u64,
}

impl SynthEngine {
    /// Build all voice graphs up front; nothing is allocated while rendering.
    pub fn new(patch: SynthPatch, voices: usize, sr: usize, cutoff: f32, q: f32) -> Self {
        let voices = (0..voices.max(1))
            .map(|_| {
                let (freq, gate, vel) = (Lane::new(), Lane::new(), Lane::new());
                let mut graph = build_graph(patch, sr, &freq, &gate, &vel, cutoff, q);
                // Glicol sizes its processor's maps on the first block; do that now,
                // not on the audio thread when the voice is first used.
                graph.next_block();
                Voice {
                    graph,
                    freq, gate, vel,
                    note: None, gate_on: false, hz: 0.0, velocity: 0.0, started: 0,
                    silent_after: i64::MIN,
                }
            })
            .collect();
        Self {
            patch, sr, voices,
            pending: Vec::with_capacity(MAX_PENDING),
            block: [0.0; BLOCK],
            block_start: 0,
            next_frame: None,
            starts: 0,
        }
    }

    /// Queue a note command for absolute frame `frame` (it sounds at `frame + LATENCY`).
    pub fn schedule(&mut self, msg: NoteMsg, frame: i64) {
        if self.pending.len() < MAX_PENDING {
            self.pending.push(Pending { frame: frame + LATENCY, msg });
        }
    }

    pub fn set_filter(&mut self, cutoff: f32, q: f32) {
        let cutoff = cutoff.clamp(20.0, self.sr as f32 * 0.45);
        let q = q.max(0.1);
        for v in &mut self.voices {
            let filter = v.graph.tags[FILTER];
            v.graph.send_msg(filter, Message::SetToNumber(0, cutoff));
            v.graph.send_msg(filter, Message::SetToNumber(1, q));
        }
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| !v.is_idle(self.block_start)).count()
    }

    /// Render `out.len()` frames starting at absolute frame `clock`.
    /// Returns `false` when everything rendered was silence.
    pub fn render(&mut self, clock: i64, out: &mut [f32]) -> bool {
        if self.next_frame != Some(clock) {
            // First call or a clock jump: restart the block grid here.
            self.block_start = clock - BLOCK as i64;
        }
        self.next_frame = Some(clock + out.len() as i64);

        let mut audible = false;
        for (k, sample) in out.iter_mut().enumerate() {
            let frame = clock + k as i64;
            if frame >= self.block_start + BLOCK as i64 {
                self.render_block(frame);
            }
            *sample = self.block[(frame - self.block_start) as usize];
            audible |= *sample != 0.0;
        }
        audible
    }

    fn render_block(&mut self, start: i64) {
        self.block_start = start;
        let end = start + BLOCK as i64;

        for v in &mut self.voices {
            v.freq.fill(0, v.hz);
            v.gate.fill(0, if v.gate_on { 1.0 } else { 0.0 });
            v.vel.fill(0, v.velocity);
        }

        // Apply this block's events in time order. Late events land on frame 0.
        self.pending.sort_unstable_by_key(|p| p.frame);
        let due = self.pending.iter().take_while(|p| p.frame < end).count();
        for i in 0..due {
            let Pending { frame, msg } = self.pending[i];
            let offset = (frame - start).clamp(0, BLOCK as i64 - 1) as usize;
            self.apply(msg, offset, start + offset as i64);
        }
        self.pending.drain(..due);

        self.block.fill(0.0);
        for v in &mut self.voices {
            if v.is_idle(start) {
                continue;
            }
            let out = v.graph.next_block();
            for (acc, s) in self.block.iter_mut().zip(out[0].iter()) {
                *acc += *s;
            }
        }
    }

    fn apply(&mut self, msg: NoteMsg, offset: usize, frame: i64) {
        let release = self.patch.release_frames(self.sr);
        match msg {
            NoteMsg::On { id, midi, velocity } => {
                let idx = self.pick_voice(frame);
                self.starts += 1;
                let v = &mut self.voices[idx];
                let retrigger = v.gate_on;
                v.note = Some(id);
                v.gate_on = true;
                v.hz = midi_to_hz(midi);
                v.velocity = velocity.clamp(0.0, 1.0);
                v.started = self.starts;
                v.freq.fill(offset, v.hz);
                v.vel.fill(offset, v.velocity);
                v.gate.fill(offset, 1.0);
                if retrigger {
                    // Glicol's ADSR needs a 0 → 1 edge to restart.
                    v.gate.set(offset, 0.0);
                }
            }
            NoteMsg::Off { id } => {
                for v in self.voices.iter_mut().filter(|v| v.gate_on && v.note == Some(id)) {
                    v.gate_on = false;
                    v.gate.fill(offset, 0.0);
                    v.silent_after = frame + release;
                }
            }
            NoteMsg::AllOff => {
                for v in self.voices.iter_mut().filter(|v| v.gate_on) {
                    v.gate_on = false;
                    v.gate.fill(offset, 0.0);
                    v.silent_after = frame + release;
                }
            }
        }
    }

    /// An idle voice if there is one, else the released voice that started
    /// longest ago, else the oldest voice outright.
    fn pick_voice(&self, frame: i64) -> usize {
        let oldest = |held: bool| {
            self.voices
                .iter()
                .enumerate()
                .filter(|(_, v)| v.gate_on == held)
                .min_by_key(|(_, v)| v.started)
                .map(|(i, _)| i)
        };
        self.voices
            .iter()
            .position(|v| v.is_idle(frame))
            .or_else(|| oldest(false))
            .or_else(|| oldest(true))
            .unwrap_or(0)
    }
}

// ── Note message encoding ────────────────────────────────────────────────────

impl NoteMsg {
    /// Pack into Firewheel's stack-only custom event payload (no allocation).
    pub fn encode(self) -> [u8; 36] {
        let mut b = [0u8; 36];
        let (kind, id, midi, vel) = match self {
            NoteMsg::On { id, midi, velocity } => (1u8, id, midi, velocity),
            NoteMsg::Off { id } => (2, id, 0.0, 0.0),
            NoteMsg::AllOff => (3, 0, 0.0, 0.0),
        };
        b[0] = kind;
        b[1..5].copy_from_slice(&id.to_le_bytes());
        b[5..9].copy_from_slice(&midi.to_le_bytes());
        b[9..13].copy_from_slice(&vel.to_le_bytes());
        b
    }

    pub fn decode(b: &[u8; 36]) -> Option<Self> {
        let id = u32::from_le_bytes(b[1..5].try_into().ok()?);
        let f = |r: std::ops::Range<usize>| f32::from_le_bytes(b[r].try_into().unwrap());
        match b[0] {
            1 => Some(NoteMsg::On { id, midi: f(5..9), velocity: f(9..13) }),
            2 => Some(NoteMsg::Off { id }),
            3 => Some(NoteMsg::AllOff),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: usize = 48_000;

    fn engine(patch: SynthPatch, voices: usize) -> SynthEngine {
        SynthEngine::new(patch, voices, SR, 2_000.0, 1.0)
    }

    /// Render `frames` frames in uneven chunks, like Firewheel sub-chunks do.
    fn render(e: &mut SynthEngine, from: i64, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames];
        let mut done = 0;
        let chunks = [1usize, 37, 64, 200, 13, 512];
        let mut k = 0;
        while done < frames {
            let n = chunks[k % chunks.len()].min(frames - done);
            e.render(from + done as i64, &mut out[done..done + n]);
            done += n;
            k += 1;
        }
        out
    }

    #[test]
    fn note_msg_roundtrip() {
        for msg in [NoteMsg::On { id: 7, midi: 61.5, velocity: 0.8 }, NoteMsg::Off { id: 9 }, NoteMsg::AllOff] {
            assert_eq!(NoteMsg::decode(&msg.encode()), Some(msg));
        }
    }

    #[test]
    fn silent_until_the_note_then_sounds_on_the_exact_frame() {
        for start in [0i64, 1000, 4321] {
            let mut e = engine(SynthPatch::SawLead, 4);
            let on_frame = start + 777;
            e.schedule(NoteMsg::On { id: 1, midi: 60.0, velocity: 1.0 }, on_frame);
            let out = render(&mut e, start, 4000);
            let first = out.iter().position(|s| s.abs() > 1e-6).expect("note never sounded");
            let expected = (on_frame + LATENCY - start) as usize;
            // The saw starts at -1 * env(0) = 0, so allow the first couple of frames to be ramping.
            assert!((expected..expected + 3).contains(&first), "start {start}: first sound at {first}, expected {expected}");
        }
    }

    #[test]
    fn released_voice_goes_idle_and_silent() {
        let mut e = engine(SynthPatch::Pluck, 2);
        e.schedule(NoteMsg::On { id: 1, midi: 64.0, velocity: 1.0 }, 0);
        e.schedule(NoteMsg::Off { id: 1 }, 2_000);
        let out = render(&mut e, 0, SR);
        assert!(out[..2_000].iter().any(|s| s.abs() > 0.01));
        assert!(out[SR / 2..].iter().all(|s| *s == 0.0), "tail should be silent");
        assert_eq!(e.active_voices(), 0);
    }

    #[test]
    fn voice_stealing_keeps_polyphony_bounded() {
        let mut e = engine(SynthPatch::Pad, 3);
        for id in 0..10 {
            e.schedule(NoteMsg::On { id, midi: 50.0 + id as f32, velocity: 0.5 }, id as i64 * 100);
        }
        let out = render(&mut e, 0, 8_000);
        assert_eq!(e.active_voices(), 3);
        assert!(out.iter().all(|s| s.is_finite() && s.abs() < 2.0));
    }

    #[test]
    fn rendering_does_not_allocate() {
        let mut e = engine(SynthPatch::SawLead, 8);
        // Warm up: let glicol's processor size its internal maps.
        e.schedule(NoteMsg::On { id: 0, midi: 60.0, velocity: 1.0 }, 0);
        let mut buf = vec![0.0; 512];
        for k in 0..20 {
            e.render(k * 512, &mut buf);
        }
        let before = alloc_count::get();
        let mut frame = 20 * 512;
        for n in 0..200u32 {
            e.schedule(NoteMsg::On { id: n, midi: 48.0 + (n % 24) as f32, velocity: 0.7 }, frame);
            e.schedule(NoteMsg::Off { id: n.wrapping_sub(3) }, frame + 100);
            e.set_filter(500.0 + n as f32 * 10.0, 2.0);
            e.render(frame, &mut buf);
            frame += 512;
        }
        assert_eq!(alloc_count::get() - before, 0, "allocations on the render path");
    }

    /// `cargo test --release --features synth --lib voice_cost -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn voice_cost() {
        for patch in [SynthPatch::Pluck, SynthPatch::SawLead, SynthPatch::Pad] {
            for voices in [8usize, 32, 64] {
                let mut e = engine(patch, voices);
                for id in 0..voices as u32 {
                    e.schedule(NoteMsg::On { id, midi: 40.0 + id as f32, velocity: 0.5 }, 0);
                }
                let seconds = 10;
                let mut buf = vec![0.0; 256];
                let t = std::time::Instant::now();
                for k in 0..(seconds * SR / 256) as i64 {
                    e.render(k * 256, &mut buf);
                }
                let load = t.elapsed().as_secs_f64() / seconds as f64;
                println!("{patch:?} × {voices:>2} voices: {:5.2}% of one core ({:.3}% per voice)", load * 100.0, load * 100.0 / voices as f64);
            }
        }
    }

    /// Thread-local allocation counter, so parallel tests don't interfere.
    mod alloc_count {
        use std::alloc::{GlobalAlloc, Layout, System};
        use std::cell::Cell;

        thread_local!(static COUNT: Cell<usize> = const { Cell::new(0) });

        struct Counting;
        unsafe impl GlobalAlloc for Counting {
            unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
                let _ = COUNT.try_with(|c| c.set(c.get() + 1));
                unsafe { System.alloc(layout) }
            }
            unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
                unsafe { System.dealloc(ptr, layout) }
            }
            unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
                let _ = COUNT.try_with(|c| c.set(c.get() + 1));
                unsafe { System.realloc(ptr, layout, new_size) }
            }
        }

        #[global_allocator]
        static GLOBAL: Counting = Counting;

        pub fn get() -> usize {
            COUNT.with(|c| c.get())
        }
    }
}
