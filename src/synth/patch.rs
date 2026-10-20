//! What a synth instrument *is*: oscillators, envelopes, a filter and how
//! notes are voiced. A [`SynthPatch`] is plain data, built once per voice into
//! a glicol graph (see `engine::build_graph`), so presets are just functions.

pub use super::dsp::{Adsr, FilterMod, FilterMode, Wave};

/// Frequency modulation of one oscillator by a sine: bells, e-pianos, metallic hits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fm {
    /// Modulator frequency as a multiple of the note.
    pub ratio: f32,
    /// Peak modulation depth (in cycles of phase).
    pub index: f32,
    /// How the depth evolves per note: bright attack mellowing out.
    pub env: Adsr,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OscSpec {
    pub wave: Wave,
    /// Frequency as a multiple of the note: 2.0 = an octave up, 1.005 = detuned.
    pub ratio: f32,
    pub level: f32,
    /// For [`Wave::Square`]: 0.5 is square, 0.125 is thin and nasal.
    pub pulse_width: f32,
    pub fm: Option<Fm>,
}

impl OscSpec {
    pub fn new(wave: Wave) -> Self {
        Self { wave, ratio: 1.0, level: 1.0, pulse_width: 0.5, fm: None }
    }

    pub fn ratio(self, ratio: f32) -> Self {
        Self { ratio, ..self }
    }

    /// Detune in cents (hundredths of a semitone).
    pub fn cents(self, cents: f32) -> Self {
        self.ratio(self.ratio * (cents / 1200.0).exp2())
    }

    pub fn level(self, level: f32) -> Self {
        Self { level, ..self }
    }

    pub fn pulse_width(self, pulse_width: f32) -> Self {
        Self { pulse_width, ..self }
    }

    pub fn fm(self, ratio: f32, index: f32, env: Adsr) -> Self {
        Self { fm: Some(Fm { ratio, index, env }), ..self }
    }
}

/// How notes are assigned to voices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VoiceMode {
    /// Every note gets its own voice (chords, pads).
    #[default]
    Poly,
    /// One voice; each note re-triggers the envelopes. Overlapping notes glide.
    Mono,
    /// One voice; overlapping notes glide *without* re-triggering: slides,
    /// 303 basslines, singing leads. Releasing a note falls back to the one
    /// still held.
    Legato,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SynthPatch {
    pub name: String,
    /// Mixed together; at most four.
    pub oscs: Vec<OscSpec>,
    /// Each note starts `semitones` sharp and falls to pitch over `seconds`.
    pub pitch_sweep: Option<(f32, f32)>,
    pub amp: Adsr,
    pub filter: FilterMod,
    pub filter_env: Adsr,
    /// Default base cutoff (Hz) and resonance (Q); `SynthNode` changes them live.
    pub cutoff: f32,
    pub resonance: f32,
    pub gain: f32,
    pub mode: VoiceMode,
    /// Seconds to slide between overlapping notes (mono / legato modes).
    pub glide: f32,
}

impl Default for SynthPatch {
    fn default() -> Self {
        Self::saw_lead()
    }
}

impl SynthPatch {
    /// A plain patch to build on: one saw, organ-ish envelope, open filter.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            oscs: vec![OscSpec::new(Wave::Saw)],
            pitch_sweep: None,
            amp: Adsr::new(0.005, 0.1, 0.8, 0.1),
            filter: FilterMod::default(),
            filter_env: Adsr::new(0.005, 0.3, 0.0, 0.2),
            cutoff: 4_000.0,
            resonance: 0.707,
            gain: 0.3,
            mode: VoiceMode::Poly,
            glide: 0.0,
        }
    }

    /// Longest time a voice keeps sounding after its note is released.
    pub(crate) fn release_secs(&self) -> f32 {
        // Exponential release reaches -100 dB after ~3× its nominal time.
        self.amp.release * 3.0
    }

    // ── Presets ──────────────────────────────────────────────────────────────

    /// Two detuned saws, a little filter bite on each note.
    pub fn saw_lead() -> Self {
        Self {
            oscs: vec![OscSpec::new(Wave::Saw).cents(-6.0).level(0.5), OscSpec::new(Wave::Saw).cents(6.0).level(0.5)],
            amp: Adsr::new(0.005, 0.2, 0.7, 0.15),
            filter: FilterMod { env_amount: 1.5, key_track: 0.5, velocity: 1.0, ..FilterMod::default() },
            filter_env: Adsr::new(0.003, 0.25, 0.2, 0.2),
            cutoff: 1_500.0,
            resonance: 1.2,
            gain: 0.3,
            ..Self::new("saw lead")
        }
    }

    /// Mono saw lead that glides between overlapping notes.
    pub fn glide_lead() -> Self {
        Self { mode: VoiceMode::Legato, glide: 0.08, ..Self::saw_lead().named("glide lead") }
    }

    /// Plucked square: the filter snaps shut like a string damping.
    pub fn pluck() -> Self {
        Self {
            oscs: vec![OscSpec::new(Wave::Square).pulse_width(0.35)],
            amp: Adsr::new(0.001, 0.4, 0.0, 0.2),
            filter: FilterMod { env_amount: 3.0, key_track: 0.6, velocity: 1.5, ..FilterMod::default() },
            filter_env: Adsr::perc(0.12),
            cutoff: 500.0,
            resonance: 1.0,
            gain: 0.35,
            ..Self::new("pluck")
        }
    }

    /// Sine with a triangle an octave down: felt more than heard.
    pub fn sub_bass() -> Self {
        Self {
            oscs: vec![OscSpec::new(Wave::Sine).level(0.7), OscSpec::new(Wave::Triangle).ratio(0.5).level(0.4)],
            amp: Adsr::new(0.004, 0.1, 0.85, 0.08),
            cutoff: 800.0,
            gain: 0.4,
            ..Self::new("sub bass")
        }
    }

    /// Resonant saw bass that slides between overlapping notes: acid.
    pub fn acid_bass() -> Self {
        Self {
            oscs: vec![OscSpec::new(Wave::Saw)],
            amp: Adsr::new(0.002, 0.3, 0.6, 0.05),
            filter: FilterMod { env_amount: 3.5, key_track: 0.3, velocity: 1.5, ..FilterMod::default() },
            filter_env: Adsr::new(0.002, 0.18, 0.0, 0.1),
            cutoff: 300.0,
            resonance: 6.0,
            gain: 0.3,
            mode: VoiceMode::Legato,
            glide: 0.06,
            ..Self::new("acid bass")
        }
    }

    /// Three detuned saws, slow swell, slow release.
    pub fn pad() -> Self {
        Self {
            oscs: vec![
                OscSpec::new(Wave::Saw).cents(-11.0).level(0.35),
                OscSpec::new(Wave::Saw).level(0.35),
                OscSpec::new(Wave::Saw).cents(12.0).level(0.35),
            ],
            amp: Adsr::new(0.5, 0.8, 0.75, 1.5),
            filter: FilterMod { env_amount: 1.0, ..FilterMod::default() },
            filter_env: Adsr::new(1.2, 1.5, 0.3, 1.5),
            cutoff: 900.0,
            resonance: 0.8,
            gain: 0.3,
            ..Self::new("pad")
        }
    }

    /// Thin pulse, instant gate: 8-bit chiptune.
    pub fn chip() -> Self {
        Self {
            oscs: vec![OscSpec::new(Wave::Square).pulse_width(0.125)],
            amp: Adsr::new(0.001, 0.05, 0.6, 0.03),
            cutoff: 12_000.0,
            gain: 0.15,
            ..Self::new("chip")
        }
    }

    /// Two-operator FM bell: inharmonic shimmer that mellows as it rings.
    pub fn bell() -> Self {
        Self {
            oscs: vec![OscSpec::new(Wave::Sine).fm(3.5, 1.2, Adsr::new(0.001, 1.5, 0.05, 1.5))],
            amp: Adsr::new(0.001, 2.5, 0.0, 2.0),
            cutoff: 10_000.0,
            gain: 0.35,
            ..Self::new("bell")
        }
    }

    /// FM electric piano: soft bark on the attack, round tone underneath.
    pub fn e_piano() -> Self {
        Self {
            oscs: vec![OscSpec::new(Wave::Sine).fm(1.0, 0.6, Adsr::new(0.001, 0.6, 0.1, 0.4))],
            amp: Adsr::new(0.002, 1.8, 0.2, 0.4),
            filter: FilterMod { velocity: 1.5, ..FilterMod::default() },
            cutoff: 6_000.0,
            gain: 0.4,
            ..Self::new("e-piano")
        }
    }

    /// Sine with a fast downward sweep. Play it around C2 (MIDI 36).
    pub fn kick() -> Self {
        Self {
            oscs: vec![OscSpec::new(Wave::Sine)],
            pitch_sweep: Some((30.0, 0.06)),
            amp: Adsr::perc(0.35),
            cutoff: 5_000.0,
            gain: 0.75,
            ..Self::new("kick")
        }
    }

    /// Noise burst plus a swept triangle body, band-passed.
    pub fn snare() -> Self {
        Self {
            oscs: vec![OscSpec::new(Wave::Noise).level(0.8), OscSpec::new(Wave::Triangle).level(0.6)],
            pitch_sweep: Some((12.0, 0.03)),
            amp: Adsr::perc(0.18),
            filter: FilterMod { mode: FilterMode::BandPass, ..FilterMod::default() },
            cutoff: 1_800.0,
            resonance: 0.6,
            gain: 1.1,
            ..Self::new("snare")
        }
    }

    /// High-passed noise tick. Lengthen `amp` for an open hat.
    pub fn hat() -> Self {
        Self {
            oscs: vec![OscSpec::new(Wave::Noise)],
            amp: Adsr::perc(0.05),
            filter: FilterMod { mode: FilterMode::HighPass, ..FilterMod::default() },
            cutoff: 8_000.0,
            resonance: 0.9,
            gain: 0.35,
            ..Self::new("hat")
        }
    }

    /// Every built-in preset, for browsing.
    pub fn presets() -> Vec<SynthPatch> {
        vec![
            Self::saw_lead(),
            Self::glide_lead(),
            Self::pluck(),
            Self::sub_bass(),
            Self::acid_bass(),
            Self::pad(),
            Self::chip(),
            Self::bell(),
            Self::e_piano(),
            Self::kick(),
            Self::snare(),
            Self::hat(),
        ]
    }

    pub fn named(self, name: impl Into<String>) -> Self {
        Self { name: name.into(), ..self }
    }
}
