//! Our own glicol nodes, for the parts where glicol's built-ins fall short of
//! instrument quality: band-limited oscillators, an envelope that never
//! clicks, and a smoothed, modulatable state-variable filter.
//!
//! They implement glicol's [`Node`] trait, so they live in ordinary glicol
//! graphs next to glicol's own nodes. Nodes with several inputs read them in
//! the order they were connected ([`Ports`]).

use std::f32::consts::{PI, TAU};

use glicol_synth::{Buffer, Input, Message, Node};
use hashbrown::HashMap;

use super::engine::BLOCK;

/// Up to four inputs, remembered in connection order without allocating.
#[derive(Debug, Default, Clone, Copy)]
struct Ports {
    ids: [usize; 4],
    len: usize,
}

impl Ports {
    fn handle(&mut self, msg: &Message) {
        if let Message::Index(id) = *msg
            && self.len < self.ids.len()
        {
            self.ids[self.len] = id;
            self.len += 1;
        }
    }

    fn get<'a>(&self, inputs: &'a HashMap<usize, Input<BLOCK>>, port: usize) -> Option<&'a [f32; BLOCK]> {
        if port >= self.len {
            return None;
        }
        inputs.get(&self.ids[port]).and_then(|i| i.buffers().first()).and_then(|b| (&**b).try_into().ok())
    }
}

/// Seconds → one-pole coefficient: the fraction of the remaining distance
/// covered per sample, reaching ~98% after `seconds`.
pub(super) fn one_pole(seconds: f32, sr: f32) -> f32 {
    if seconds <= 0.0 { 1.0 } else { 1.0 - (-4.0 / (seconds * sr)).exp() }
}

// ── Oscillator ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Wave {
    Sine,
    #[default]
    Saw,
    /// Pulse wave; the width is [`Osc`]'s `pulse_width` (0.5 = square).
    Square,
    Triangle,
    /// White noise; ignores the frequency input.
    Noise,
}

/// PolyBLEP residual: smooths the step at a phase wrap so saws and squares
/// don't alias into a fizzy mess at high notes.
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

/// Band-limited oscillator. Inputs: 0 = frequency in Hz, 1 = optional phase
/// modulation in cycles (FM). Messages: 0 = frequency ratio, 1 = pulse width,
/// 2 = level.
pub struct Osc {
    wave: Wave,
    /// Frequency used when nothing is connected to input 0.
    fixed_hz: f32,
    ratio: f32,
    pulse_width: f32,
    level: f32,
    phase: f32,
    noise: u32,
    sr: f32,
    ports: Ports,
}

impl Osc {
    pub fn new(wave: Wave, sr: usize) -> Self {
        Self { wave, fixed_hz: 0.0, ratio: 1.0, pulse_width: 0.5, level: 1.0, phase: 0.0, noise: 0x9E37_79B9, sr: sr as f32, ports: Ports::default() }
    }

    pub fn ratio(self, ratio: f32) -> Self {
        Self { ratio, ..self }
    }

    /// A fixed frequency, for oscillators with no frequency input (LFOs, drones).
    pub fn fixed(self, hz: f32) -> Self {
        Self { fixed_hz: hz, ..self }
    }

    pub fn pulse_width(self, pulse_width: f32) -> Self {
        Self { pulse_width: pulse_width.clamp(0.05, 0.95), ..self }
    }

    pub fn level(self, level: f32) -> Self {
        Self { level, ..self }
    }

    /// Start at a different phase, so detuned copies don't begin in lockstep.
    pub fn phase(self, phase: f32) -> Self {
        Self { phase: phase.rem_euclid(1.0), ..self }
    }

    fn sample(&mut self, hz: f32, pm: f32) -> f32 {
        let dt = (hz * self.ratio / self.sr).clamp(0.0, 0.5);
        let t = (self.phase + pm).rem_euclid(1.0);
        let out = match self.wave {
            Wave::Sine => (t * TAU).sin(),
            Wave::Saw => 2.0 * t - 1.0 - poly_blep(t, dt),
            Wave::Square => {
                let pw = self.pulse_width;
                let naive = if t < pw { 1.0 } else { -1.0 };
                naive + poly_blep(t, dt) - poly_blep((t + 1.0 - pw).rem_euclid(1.0), dt)
            }
            // Harmonics fall off as 1/n², so a naive triangle barely aliases.
            Wave::Triangle => 4.0 * (t - 0.5).abs() - 1.0,
            Wave::Noise => {
                // xorshift32: cheap, allocation-free, good enough for audio.
                self.noise ^= self.noise << 13;
                self.noise ^= self.noise >> 17;
                self.noise ^= self.noise << 5;
                self.noise as f32 / u32::MAX as f32 * 2.0 - 1.0
            }
        };
        self.phase = (self.phase + dt).fract();
        out * self.level
    }
}

impl Node<BLOCK> for Osc {
    fn process(&mut self, inputs: &mut HashMap<usize, Input<BLOCK>>, output: &mut [Buffer<BLOCK>]) {
        let freq = self.ports.get(inputs, 0).copied();
        let pm = self.ports.get(inputs, 1).copied();
        for (i, out) in output[0].iter_mut().enumerate() {
            let hz = freq.map_or(self.fixed_hz, |f| f[i]);
            *out = self.sample(hz, pm.map_or(0.0, |p| p[i]));
        }
    }

    fn send_msg(&mut self, msg: Message) {
        self.ports.handle(&msg);
        match msg {
            Message::SetToNumber(0, v) => self.ratio = v,
            Message::SetToNumber(1, v) => self.pulse_width = v.clamp(0.05, 0.95),
            Message::SetToNumber(2, v) => self.level = v,
            _ => {}
        }
    }
}

// ── Envelope ─────────────────────────────────────────────────────────────────

/// Attack / decay / release in seconds, sustain as a level 0–1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Adsr {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

impl Adsr {
    pub const fn new(attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        Self { attack, decay, sustain, release }
    }

    /// Percussive: instant attack, decays to silence whether held or not.
    pub const fn perc(decay: f32) -> Self {
        Self::new(0.001, decay, 0.0, decay)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Release,
}

/// Gate-driven ADSR. Input 0 = gate (> 0.5 is held).
///
/// Unlike glicol's `adsr`, a re-triggered note attacks from wherever the level
/// is instead of jumping to 0, and decay/release are exponential, so stolen
/// and repeated notes never click.
pub struct Env {
    adsr: Adsr,
    /// Percussive: a rising edge plays attack + decay in full, ignoring the gate's fall.
    one_shot: bool,
    stage: Stage,
    level: f32,
    gate: bool,
    attack_step: f32,
    decay_k: f32,
    release_k: f32,
    ports: Ports,
}

impl Env {
    pub fn new(adsr: Adsr, sr: usize) -> Self {
        let sr = sr as f32;
        Self {
            adsr,
            one_shot: false,
            stage: Stage::Idle,
            level: 0.0,
            gate: false,
            attack_step: 1.0 / (adsr.attack.max(0.0005) * sr),
            decay_k: one_pole(adsr.decay, sr),
            release_k: one_pole(adsr.release, sr),
            ports: Ports::default(),
        }
    }

    /// Glicol's `envperc`: fires on a rising edge (a `~trig` impulse works) and decays to silence.
    pub fn perc(attack: f32, decay: f32, sr: usize) -> Self {
        Self { one_shot: true, ..Self::new(Adsr::new(attack, decay, 0.0, decay), sr) }
    }

    fn next(&mut self, gate: bool) -> f32 {
        if gate && !self.gate {
            self.stage = Stage::Attack;
        } else if !gate && self.gate && !self.one_shot {
            self.stage = Stage::Release;
        }
        self.gate = gate;
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.level += self.attack_step;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level += (self.adsr.sustain - self.level) * self.decay_k;
                if self.one_shot && self.level < 1e-5 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
            Stage::Release => {
                self.level -= self.level * self.release_k;
                if self.level < 1e-5 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}

impl Node<BLOCK> for Env {
    fn process(&mut self, inputs: &mut HashMap<usize, Input<BLOCK>>, output: &mut [Buffer<BLOCK>]) {
        let gate = self.ports.get(inputs, 0).copied().unwrap_or([0.0; BLOCK]);
        for (out, g) in output[0].iter_mut().zip(gate) {
            *out = self.next(g > 0.5);
        }
    }

    fn send_msg(&mut self, msg: Message) {
        self.ports.handle(&msg);
    }
}

// ── Pitch sweep ──────────────────────────────────────────────────────────────

/// Bends a frequency down from `semitones` above it on each new gate: the
/// "boom" of a synth kick or tom. Inputs: 0 = frequency, 1 = gate.
pub struct PitchSweep {
    semitones: f32,
    k: f32,
    amount: f32,
    gate: bool,
    ports: Ports,
}

impl PitchSweep {
    pub fn new(semitones: f32, seconds: f32, sr: usize) -> Self {
        Self { semitones, k: one_pole(seconds, sr as f32), amount: 0.0, gate: false, ports: Ports::default() }
    }

    fn next(&mut self, hz: f32, gate: bool) -> f32 {
        if gate && !self.gate {
            self.amount = 1.0;
        }
        self.gate = gate;
        let out = hz * (self.semitones * self.amount / 12.0).exp2();
        self.amount -= self.amount * self.k;
        out
    }
}

impl Node<BLOCK> for PitchSweep {
    fn process(&mut self, inputs: &mut HashMap<usize, Input<BLOCK>>, output: &mut [Buffer<BLOCK>]) {
        let freq = self.ports.get(inputs, 0).copied().unwrap_or([0.0; BLOCK]);
        let gate = self.ports.get(inputs, 1).copied().unwrap_or([0.0; BLOCK]);
        for ((out, f), g) in output[0].iter_mut().zip(freq).zip(gate) {
            *out = self.next(f, g > 0.5);
        }
    }

    fn send_msg(&mut self, msg: Message) {
        self.ports.handle(&msg);
    }
}

// ── Filter ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterMode {
    #[default]
    LowPass,
    HighPass,
    BandPass,
}

/// How the filter's cutoff moves per note, on top of the live base cutoff.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilterMod {
    pub mode: FilterMode,
    /// Octaves the filter envelope opens the cutoff at its peak.
    pub env_amount: f32,
    /// 0 = fixed cutoff, 1 = cutoff follows the note (relative to middle C).
    pub key_track: f32,
    /// Octaves of extra cutoff at full velocity versus velocity 0.
    pub velocity: f32,
}

impl Default for FilterMod {
    fn default() -> Self {
        Self { mode: FilterMode::LowPass, env_amount: 0.0, key_track: 0.0, velocity: 0.0 }
    }
}

/// Zero-delay-feedback state-variable filter (Simper's SVF).
///
/// Inputs: 0 = audio, 1 = envelope (0–1), 2 = note frequency, 3 = velocity.
/// Messages: 0 = base cutoff in Hz, 1 = Q. Base cutoff and Q glide to new
/// values over ~10 ms, so live game-driven sweeps never zipper.
pub struct Svf {
    modulation: FilterMod,
    /// When true, input 1 is the cutoff in Hz (glicol's `lpf ~signal q`)
    /// instead of the envelope/key/velocity modulation inputs.
    driven: bool,
    cutoff: f32,
    q: f32,
    target_cutoff: f32,
    target_q: f32,
    smooth: f32,
    sr: f32,
    ic1: f32,
    ic2: f32,
    ports: Ports,
}

impl Svf {
    pub fn new(modulation: FilterMod, cutoff: f32, q: f32, sr: usize) -> Self {
        let sr = sr as f32;
        Self {
            modulation,
            driven: false,
            cutoff,
            q,
            target_cutoff: cutoff,
            target_q: q,
            smooth: one_pole(0.01, sr / CONTROL as f32),
            sr,
            ic1: 0.0,
            ic2: 0.0,
            ports: Ports::default(),
        }
    }

    /// A filter whose cutoff (Hz) comes from input 1, per sample block.
    pub fn driven(mode: FilterMode, q: f32, sr: usize) -> Self {
        Self { driven: true, ..Self::new(FilterMod { mode, ..FilterMod::default() }, 1_000.0, q, sr) }
    }

    fn coeffs(&self, cutoff: f32) -> SvfCoeffs {
        let g = (PI * cutoff / self.sr).tan();
        let k = 1.0 / self.q;
        let a1 = 1.0 / (1.0 + g * (g + k));
        SvfCoeffs { k, a1, a2: g * a1, a3: g * g * a1 }
    }

    fn tick(&mut self, x: f32, &SvfCoeffs { k, a1, a2, a3 }: &SvfCoeffs) -> f32 {
        let v3 = x - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        match self.modulation.mode {
            FilterMode::LowPass => v2,
            FilterMode::HighPass => x - k * v1 - v2,
            FilterMode::BandPass => v1,
        }
    }
}

struct SvfCoeffs {
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
}

/// The filter recomputes its cutoff every this many samples (6 kHz at 48 kHz):
/// smooth to the ear, and `tan`/`exp2` stay off the per-sample path.
const CONTROL: usize = 8;

impl Node<BLOCK> for Svf {
    fn process(&mut self, inputs: &mut HashMap<usize, Input<BLOCK>>, output: &mut [Buffer<BLOCK>]) {
        let Some(audio) = self.ports.get(inputs, 0).copied() else {
            output[0].fill(0.0);
            return;
        };
        let env = self.ports.get(inputs, 1).copied();
        let freq = self.ports.get(inputs, 2).copied();
        let vel = self.ports.get(inputs, 3).copied();
        let FilterMod { env_amount, key_track, velocity, .. } = self.modulation;
        let max = self.sr * 0.45;
        for i in (0..BLOCK).step_by(CONTROL) {
            // Glide in the log domain: equal times for equal musical intervals.
            if self.cutoff != self.target_cutoff {
                self.cutoff *= (self.target_cutoff / self.cutoff).powf(self.smooth);
                if (self.cutoff / self.target_cutoff - 1.0).abs() < 1e-4 {
                    self.cutoff = self.target_cutoff;
                }
            }
            self.q += (self.target_q - self.q) * self.smooth;
            let mut octaves = env.map_or(0.0, |e| e[i] * env_amount);
            octaves += vel.map_or(0.0, |v| (v[i] - 1.0) * velocity);
            if let Some(f) = freq
                && key_track != 0.0
                && f[i] > 0.0
            {
                octaves += key_track * (f[i] / 261.63).log2();
            }
            let cutoff = match (self.driven, env) {
                (true, Some(signal)) => signal[i],
                _ => self.cutoff * octaves.exp2(),
            };
            let coeffs = self.coeffs(cutoff.clamp(20.0, max));
            for j in i..i + CONTROL {
                output[0][j] = self.tick(audio[j], &coeffs);
            }
        }
        // Flush denormals / recover from a blown-up state rather than going silent forever.
        if !self.ic1.is_finite() || !self.ic2.is_finite() {
            self.ic1 = 0.0;
            self.ic2 = 0.0;
        }
    }

    fn send_msg(&mut self, msg: Message) {
        self.ports.handle(&msg);
        match msg {
            Message::SetToNumber(0, v) => self.target_cutoff = v.clamp(20.0, self.sr * 0.45),
            Message::SetToNumber(1, v) => self.target_q = v.clamp(0.3, 20.0),
            _ => {}
        }
    }
}

// ── Sum ──────────────────────────────────────────────────────────────────────

/// Adds up to four inputs (glicol's `add` takes exactly two).
#[derive(Default)]
pub struct Sum {
    ports: Ports,
}

impl Node<BLOCK> for Sum {
    fn process(&mut self, inputs: &mut HashMap<usize, Input<BLOCK>>, output: &mut [Buffer<BLOCK>]) {
        output[0].fill(0.0);
        for port in 0..self.ports.len {
            if let Some(input) = self.ports.get(inputs, port) {
                for (out, x) in output[0].iter_mut().zip(input) {
                    *out += x;
                }
            }
        }
    }

    fn send_msg(&mut self, msg: Message) {
        self.ports.handle(&msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: usize = 48_000;

    fn run_env(env: &mut Env, gate: bool, samples: usize) -> Vec<f32> {
        (0..samples).map(|_| env.next(gate)).collect()
    }

    #[test]
    fn retriggered_envelope_never_jumps() {
        let mut env = Env::new(Adsr::new(0.01, 0.1, 0.7, 0.2), SR);
        let mut out = run_env(&mut env, true, 2_000);
        out.extend(run_env(&mut env, false, 1)); // the engine's one-sample retrigger gap
        out.extend(run_env(&mut env, true, 2_000));
        out.extend(run_env(&mut env, false, 500));
        out.extend(run_env(&mut env, true, 2_000)); // re-attack during release
        let max_step = out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        assert!(max_step < 0.003, "envelope jumped by {max_step}");
    }

    #[test]
    fn envelope_shape() {
        let mut env = Env::new(Adsr::new(0.01, 0.05, 0.5, 0.1), SR);
        let held = run_env(&mut env, true, SR / 2);
        let peak = held.iter().position(|l| *l >= 1.0).unwrap();
        assert!((470..=490).contains(&peak), "attack took {peak} samples");
        assert!((held.last().unwrap() - 0.5).abs() < 1e-3, "sustain level");
        let released = run_env(&mut env, false, SR);
        assert!(released[SR / 10] < 0.5 * 0.05, "release too slow: {}", released[SR / 10]);
        assert_eq!(*released.last().unwrap(), 0.0);
    }

    /// Energy of a signal at `hz`, via a single DFT bin.
    fn bin(signal: &[f32], hz: f32) -> f32 {
        let (mut re, mut im) = (0.0, 0.0);
        for (n, s) in signal.iter().enumerate() {
            let w = TAU * hz * n as f32 / SR as f32;
            re += s * w.cos();
            im += s * w.sin();
        }
        (re * re + im * im).sqrt() / signal.len() as f32
    }

    #[test]
    fn polyblep_saw_aliases_less_than_a_naive_one() {
        // 2.5 kHz saw at 48 kHz: the 10th harmonic (25 kHz) folds back to 23 kHz.
        let hz = 2_500.0;
        let mut osc = Osc::new(Wave::Saw, SR);
        let blep: Vec<f32> = (0..SR / 4).map(|_| osc.sample(hz, 0.0)).collect();
        let naive: Vec<f32> = (0..SR / 4).map(|n| 2.0 * (n as f32 * hz / SR as f32).fract() - 1.0).collect();
        let alias = 23_000.0;
        assert!(bin(&blep, alias) < bin(&naive, alias) * 0.5, "{} vs {}", bin(&blep, alias), bin(&naive, alias));
        // ...without losing the fundamental.
        assert!(bin(&blep, hz) > bin(&naive, hz) * 0.9);
    }

    #[test]
    fn svf_low_pass_attenuates_highs_and_smooths_cutoff_changes() {
        let mut f = Svf::new(FilterMod::default(), 500.0, 0.707, SR);
        let tone = |hz: f32, n: usize| (TAU * hz * n as f32 / SR as f32).sin();
        let c = f.coeffs(500.0);
        let low: Vec<f32> = (0..SR / 4).map(|n| f.tick(tone(100.0, n), &c)).collect();
        let high: Vec<f32> = (0..SR / 4).map(|n| f.tick(tone(8_000.0, n), &c)).collect();
        assert!(bin(&low, 100.0) > 0.4, "passband lost: {}", bin(&low, 100.0));
        assert!(bin(&high, 8_000.0) < 0.005, "stopband leaks: {}", bin(&high, 8_000.0));

        // A cutoff jump is approached gradually, not applied at once.
        f.send_msg(Message::SetToNumber(0, 8_000.0));
        f.cutoff *= (f.target_cutoff / f.cutoff).powf(f.smooth);
        assert!(f.cutoff < 600.0, "cutoff jumped to {}", f.cutoff);
    }

    #[test]
    fn pitch_sweep_falls_back_to_the_note() {
        let mut sweep = PitchSweep::new(24.0, 0.05, SR);
        let out: Vec<f32> = (0..SR / 4).map(|_| sweep.next(100.0, true)).collect();
        assert!((out[0] - 400.0).abs() < 1.0, "starts two octaves up");
        assert!((out.last().unwrap() - 100.0).abs() < 0.5, "settles on the note");
        assert!(out.windows(2).all(|w| w[1] <= w[0]), "falls monotonically");
        // Only a new gate restarts it.
        assert!((sweep.next(100.0, true) - 100.0).abs() < 0.5);
        sweep.next(100.0, false);
        assert!((sweep.next(100.0, true) - 400.0).abs() < 1.0);
    }
}
