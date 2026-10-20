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

use glicol_synth::operator::Mul;
use glicol_synth::{AudioContext, AudioContextBuilder, Buffer, Input, Message, Node};
use hashbrown::HashMap;

use super::dsp::{one_pole, Env, Osc, PitchSweep, Svf, Sum};
use super::patch::{SynthPatch, VoiceMode};

/// Glicol block size in frames.
pub const BLOCK: usize = 128;
/// Constant output latency in frames, so on-time events are never already rendered.
pub const LATENCY: i64 = BLOCK as i64;
/// Upper bound on queued note events; more are dropped instead of allocating.
const MAX_PENDING: usize = 256;
/// Notes a mono/legato synth remembers holding, to fall back to on release.
const MAX_HELD: usize = 16;

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

// ── Graph ────────────────────────────────────────────────────────────────────

/// Tag under which a voice graph registers its filter node.
const FILTER: &str = "filter";

/// Wire `patch` into a fresh glicol context driven by the given lanes.
///
/// ```text
/// freq ─► [pitch sweep] ─┬─► osc ×n (◄─ FM: sine × env) ─► sum ─► × amp env ─► svf ─► × vel ─► × gain
///                        └──────────────────────────────────────────────────────┘ (key tracking)
/// ```
fn build_graph(patch: &SynthPatch, sr: usize, freq: &Arc<Lane>, gate: &Arc<Lane>, vel: &Arc<Lane>, cutoff: f32, q: f32) -> AudioContext<BLOCK> {
    let mut ctx = AudioContextBuilder::<BLOCK>::new().sr(sr).channels(1).max_nodes(48).max_edges(80).build();
    let freq_in = ctx.add_mono_node(LaneNode(freq.clone()));
    let gate_in = ctx.add_mono_node(LaneNode(gate.clone()));
    let vel_in = ctx.add_mono_node(LaneNode(vel.clone()));

    let pitch = match patch.pitch_sweep {
        Some((semitones, seconds)) => {
            let sweep = ctx.add_mono_node(PitchSweep::new(semitones, seconds, sr));
            ctx.connect(freq_in, sweep);
            ctx.connect(gate_in, sweep);
            sweep
        }
        None => freq_in,
    };

    let sum = ctx.add_mono_node(Sum::default());
    for (k, spec) in patch.oscs.iter().take(4).enumerate() {
        // Spread start phases so detuned copies don't begin in lockstep.
        let osc = Osc::new(spec.wave, sr).ratio(spec.ratio).level(spec.level).pulse_width(spec.pulse_width).phase(k as f32 * 0.37);
        let osc = ctx.add_mono_node(osc);
        ctx.connect(pitch, osc);
        if let Some(fm) = spec.fm {
            let modulator = ctx.add_mono_node(Osc::new(super::dsp::Wave::Sine, sr).ratio(fm.ratio).level(fm.index));
            ctx.connect(pitch, modulator);
            let depth = ctx.add_mono_node(Env::new(fm.env, sr));
            ctx.connect(gate_in, depth);
            let fm_out = ctx.add_mono_node(Mul::new(0.0));
            ctx.connect(modulator, fm_out);
            ctx.connect(depth, fm_out);
            ctx.connect(fm_out, osc);
        }
        ctx.connect(osc, sum);
    }

    let amp_env = ctx.add_mono_node(Env::new(patch.amp, sr));
    ctx.connect(gate_in, amp_env);
    let vca = ctx.add_mono_node(Mul::new(0.0));
    ctx.connect(sum, vca);
    ctx.connect(amp_env, vca);

    let filter_env = ctx.add_mono_node(Env::new(patch.filter_env, sr));
    ctx.connect(gate_in, filter_env);
    let filter = ctx.add_mono_node(Svf::new(patch.filter, cutoff, q, sr));
    for input in [vca, filter_env, pitch, vel_in] {
        ctx.connect(input, filter);
    }
    ctx.tags.insert(FILTER, filter);

    let velocity = ctx.add_mono_node(Mul::new(0.0));
    ctx.connect(filter, velocity);
    ctx.connect(vel_in, velocity);
    let gain = ctx.add_mono_node(Mul::new(patch.gain));
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
    /// The frequency actually sounding, gliding towards `hz`.
    hz_now: f32,
    /// Lane offset where the next note starts without gliding.
    snap_at: Option<usize>,
}

impl Voice {
    fn is_idle(&self, frame: i64) -> bool {
        !self.gate_on && frame > self.silent_after
    }

    fn release(&mut self, offset: usize, silent_after: i64) {
        self.gate_on = false;
        self.gate.fill(offset, 0.0);
        self.silent_after = silent_after;
    }
}

#[derive(Clone, Copy)]
struct Pending {
    frame: i64,
    msg: NoteMsg,
}

impl Pending {
    /// Sort key: releases before new notes on the same frame, so a note
    /// that ends exactly where the next begins never counts as legato.
    fn key(&self) -> (i64, u8) {
        (self.frame, matches!(self.msg, NoteMsg::On { .. }) as u8)
    }
}

/// The audio-thread synth: a fixed voice pool plus the block adapter.
pub struct SynthEngine {
    mode: VoiceMode,
    /// Frames a released voice keeps sounding.
    release: i64,
    /// Per-sample glide coefficient (1.0 = no glide).
    glide: f32,
    /// Mono / legato note stack: (id, midi, velocity), most recent last.
    held: Vec<(u32, f32, f32)>,
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
    /// Mono and legato patches always get exactly one voice.
    pub fn new(patch: &SynthPatch, voices: usize, sr: usize, cutoff: f32, q: f32) -> Self {
        let voices = if patch.mode == VoiceMode::Poly { voices.max(1) } else { 1 };
        let voices = (0..voices)
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
                    hz_now: 0.0, snap_at: None,
                }
            })
            .collect();
        Self {
            mode: patch.mode,
            release: (patch.release_secs() * sr as f32) as i64 + BLOCK as i64,
            glide: one_pole(patch.glide, sr as f32),
            held: Vec::with_capacity(MAX_HELD),
            sr, voices,
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
        self.pending.sort_unstable_by_key(Pending::key);
        let due = self.pending.iter().take_while(|p| p.frame < end).count();
        for i in 0..due {
            let Pending { frame, msg } = self.pending[i];
            let offset = (frame - start).clamp(0, BLOCK as i64 - 1) as usize;
            self.apply(msg, offset, start + offset as i64);
        }
        self.pending.drain(..due);
        self.glide_lanes();

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
        match (msg, self.mode) {
            (NoteMsg::On { id, midi, velocity }, VoiceMode::Poly) => {
                let idx = self.pick_voice(frame);
                self.start(idx, id, midi, velocity, offset, true);
            }
            (NoteMsg::On { id, midi, velocity }, mode) => {
                let gliding = !self.held.is_empty();
                if self.held.len() == MAX_HELD {
                    self.held.remove(0);
                }
                self.held.push((id, midi, velocity));
                // Legato keeps the envelopes running across overlapping notes.
                let retrigger = !gliding || mode == VoiceMode::Mono;
                if retrigger {
                    self.start(0, id, midi, velocity, offset, !gliding);
                } else {
                    let v = &mut self.voices[0];
                    v.note = Some(id);
                    v.hz = midi_to_hz(midi);
                    v.freq.fill(offset, v.hz);
                }
            }
            (NoteMsg::Off { id }, VoiceMode::Poly) => {
                let release = self.release;
                for v in self.voices.iter_mut().filter(|v| v.gate_on && v.note == Some(id)) {
                    v.release(offset, frame + release);
                }
            }
            (NoteMsg::Off { id }, _) => {
                let Some(pos) = self.held.iter().position(|h| h.0 == id) else { return };
                self.held.remove(pos);
                if pos < self.held.len() {
                    return; // An older note, not the one sounding.
                }
                let v = &mut self.voices[0];
                match self.held.last() {
                    // Fall back to the note still held, sliding to it.
                    Some(&(prev, midi, _)) => {
                        v.note = Some(prev);
                        v.hz = midi_to_hz(midi);
                        v.freq.fill(offset, v.hz);
                    }
                    None => v.release(offset, frame + self.release),
                }
            }
            (NoteMsg::AllOff, _) => {
                self.held.clear();
                let release = self.release;
                for v in self.voices.iter_mut().filter(|v| v.gate_on) {
                    v.release(offset, frame + release);
                }
            }
        }
    }

    /// (Re)start voice `idx` on a note; `snap` jumps straight to its pitch.
    fn start(&mut self, idx: usize, id: u32, midi: f32, velocity: f32, offset: usize, snap: bool) {
        self.starts += 1;
        let v = &mut self.voices[idx];
        let retrigger = v.gate_on;
        v.note = Some(id);
        v.gate_on = true;
        v.hz = midi_to_hz(midi);
        v.velocity = velocity.clamp(0.0, 1.0);
        v.started = self.starts;
        if snap {
            v.snap_at = Some(offset);
        }
        v.freq.fill(offset, v.hz);
        v.vel.fill(offset, v.velocity);
        v.gate.fill(offset, 1.0);
        if retrigger {
            // Envelopes restart on a 0 → 1 gate edge.
            v.gate.set(offset, 0.0);
        }
    }

    /// Turn each voice's stepped frequency lane into a glide, where gliding applies.
    fn glide_lanes(&mut self) {
        let k = self.glide;
        for v in &mut self.voices {
            let snap_at = v.snap_at.take();
            for i in 0..BLOCK {
                let target = v.freq.get(i);
                if k >= 1.0 || snap_at == Some(i) || v.hz_now <= 0.0 {
                    v.hz_now = target;
                } else if v.hz_now != target {
                    // Glide in log-frequency, so every interval takes the same time.
                    v.hz_now *= (target / v.hz_now).powf(k);
                    if (v.hz_now / target - 1.0).abs() < 1e-5 {
                        v.hz_now = target;
                    }
                }
                v.freq.set(i, v.hz_now);
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
        SynthEngine::new(&patch, voices, SR, 2_000.0, 1.0)
    }

    /// Frequency lane of voice 0 at the last rendered block's final sample.
    fn sounding_hz(e: &SynthEngine) -> f32 {
        e.voices[0].freq.get(BLOCK - 1)
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
            let mut e = engine(SynthPatch::saw_lead(), 4);
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
        let mut e = engine(SynthPatch::pluck(), 2);
        e.schedule(NoteMsg::On { id: 1, midi: 64.0, velocity: 1.0 }, 0);
        e.schedule(NoteMsg::Off { id: 1 }, 2_000);
        let out = render(&mut e, 0, SR);
        assert!(out[..2_000].iter().any(|s| s.abs() > 0.01));
        assert!(out[SR * 3 / 4..].iter().all(|s| *s == 0.0), "tail should be silent");
        assert_eq!(e.active_voices(), 0);
    }

    #[test]
    fn voice_stealing_keeps_polyphony_bounded() {
        let mut e = engine(SynthPatch::pad(), 3);
        for id in 0..10 {
            e.schedule(NoteMsg::On { id, midi: 50.0 + id as f32, velocity: 0.5 }, id as i64 * 100);
        }
        let out = render(&mut e, 0, 8_000);
        assert_eq!(e.active_voices(), 3);
        assert!(out.iter().all(|s| s.is_finite() && s.abs() < 2.0));
    }

    #[test]
    fn rendering_does_not_allocate() {
        let mut e = engine(SynthPatch::saw_lead(), 8);
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

    #[test]
    fn legato_glides_without_retriggering() {
        let mut e = engine(SynthPatch::acid_bass(), 8);
        assert_eq!(e.voices.len(), 1, "legato patches are monophonic");
        e.schedule(NoteMsg::On { id: 1, midi: 45.0, velocity: 1.0 }, 0);
        e.schedule(NoteMsg::On { id: 2, midi: 57.0, velocity: 1.0 }, 4_800); // overlaps note 1
        e.schedule(NoteMsg::Off { id: 1 }, 5_000);
        render(&mut e, 0, 4_800 + LATENCY as usize + BLOCK);
        let a2 = midi_to_hz(45.0);
        let a3 = midi_to_hz(57.0);
        let mid = sounding_hz(&e);
        assert!(mid > a2 * 1.05 && mid < a3 * 0.95, "should be mid-glide, at {mid} Hz");
        render(&mut e, 6_000, 12_000);
        assert!((sounding_hz(&e) - a3).abs() < 0.01, "should arrive at A3");
        // The gate never dropped: the envelope was not re-triggered.
        assert!(e.voices[0].gate_on);
        assert!((0..BLOCK).all(|i| e.voices[0].gate.get(i) == 1.0));
    }

    #[test]
    fn releasing_the_newest_note_falls_back_to_the_held_one() {
        let mut e = engine(SynthPatch::glide_lead(), 1);
        e.schedule(NoteMsg::On { id: 1, midi: 60.0, velocity: 1.0 }, 0);
        e.schedule(NoteMsg::On { id: 2, midi: 67.0, velocity: 1.0 }, 1_000);
        e.schedule(NoteMsg::Off { id: 2 }, 5_000);
        render(&mut e, 0, 30_000);
        assert!((sounding_hz(&e) - midi_to_hz(60.0)).abs() < 0.01);
        assert!(e.voices[0].gate_on, "note 1 is still held");
        e.schedule(NoteMsg::Off { id: 1 }, 30_000);
        render(&mut e, 30_000, 1_000);
        assert!(!e.voices[0].gate_on);
    }

    #[test]
    fn back_to_back_notes_are_not_legato() {
        // Off and On on the same frame: the release comes first, so the new
        // note starts fresh (snaps to pitch, re-triggers) instead of sliding.
        let mut e = engine(SynthPatch::acid_bass(), 1);
        e.schedule(NoteMsg::On { id: 1, midi: 40.0, velocity: 1.0 }, 0);
        e.schedule(NoteMsg::On { id: 2, midi: 52.0, velocity: 1.0 }, 1_000);
        e.schedule(NoteMsg::Off { id: 1 }, 1_000);
        render(&mut e, 0, 1_000 + LATENCY as usize + BLOCK);
        assert!((sounding_hz(&e) - midi_to_hz(52.0)).abs() < 0.01, "snapped, not gliding");
    }

    #[test]
    fn every_preset_sounds_and_stays_in_range() {
        for patch in SynthPatch::presets() {
            let name = patch.name.clone();
            // Last note-off at 1.125 s, then the release tail, then a quiet margin.
            let seconds = 1.125 + patch.release_secs() + 0.5;
            let frames = (seconds * SR as f32) as usize;
            let mut e = engine(patch, 4);
            for (k, midi) in [36.0, 48.0, 60.0, 72.0, 84.0].into_iter().enumerate() {
                let at = k as i64 * 12_000;
                e.schedule(NoteMsg::On { id: k as u32, midi, velocity: 1.0 }, at);
                e.schedule(NoteMsg::Off { id: k as u32 }, at + 6_000);
            }
            let out = render(&mut e, 0, frames);
            let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            let rms = (out.iter().map(|s| s * s).sum::<f32>() / out.len() as f32).sqrt();
            println!("{name:>10}: peak {peak:.3}  rms {rms:.4}");
            assert!(out.iter().all(|s| s.is_finite()), "{name}: non-finite output");
            assert!(peak > 0.02, "{name}: too quiet ({peak})");
            assert!(peak < 1.0, "{name}: clips ({peak})");
            assert!(out[frames - SR / 4..].iter().all(|s| s.abs() < 1e-3), "{name}: never fades out");
            assert_eq!(e.active_voices(), 0, "{name}: voices stuck on");
        }
    }

    /// `cargo test --release --features synth --lib voice_cost -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn voice_cost() {
        for patch in SynthPatch::presets() {
            for voices in [8usize, 32, 64] {
                let mut e = engine(patch.clone(), voices);
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
                println!("{:>9} × {voices:>2} voices: {:5.2}% of one core ({:.3}% per voice)", patch.name, load * 100.0, load * 100.0 / voices as f64);
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
