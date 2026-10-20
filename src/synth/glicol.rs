//! Synth voices written in glicol.
//!
//! A voice patch is ordinary glicol code. The voice provides these references:
//!
//! | reference | what it is                                            |
//! |-----------|-------------------------------------------------------|
//! | `~freq`   | the note's frequency in Hz (glides in mono/legato)    |
//! | `~gate`   | 1 while the note is held, 0 after release             |
//! | `~trig`   | a single-sample impulse at each note start            |
//! | `~vel`    | velocity 0–1 (the output is also scaled by it)        |
//!
//! and `#name` is a live knob: `#cutoff` follows `SynthNode::cutoff`; any
//! other `#name` becomes one of `SynthNode::params` (up to 8, in order of
//! appearance). Lines whose name has no `~` are the voice's output.
//!
//! Comment directives set what glicol itself has no syntax for:
//! `// @name acid`, `// @mode poly|mono|legato`, `// @glide 0.06`,
//! `// @gain 0.3`, `// @param drive 0.4` (a knob's default).
//!
//! ```text
//! // @mode legato
//! // @glide 0.06
//! // @param env 2.5
//! ~env: ~gate >> envperc 0.002 0.2 >> mul #env >> add 1
//! ~cut: ~env >> mul #cutoff
//! out: saw ~freq >> lpf ~cut 6.0 >> mul ~amp
//! ~amp: ~gate >> adsr 0.002 0.3 0.6 0.05
//! ```
//!
//! Supported nodes: `sin saw squ tri` (band-limited; frequency as a number or
//! reference), `noise`, `sig`, `mul add` (number or reference), `lpf hpf`
//! (cutoff as a number or reference, then Q), `adsr` and `envperc` (driven by
//! their input as a gate, click-free), `onepole`, `delayms`, `mix`, and plain
//! references (`~a: ~freq >> mul 2`). Oscillators, `noise`, `sig` and `mix`
//! start a chain; envelopes and effects follow one.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use glicol_parser::nodes::{Component, NumberOrRef, Signal};
use glicol_synth::delay::DelayMs;
use glicol_synth::filter::OnePole;
use glicol_synth::operator::{Add, Mul};
use glicol_synth::signal::ConstSig;
use glicol_synth::AudioContext;
use petgraph::graph::NodeIndex;

use super::dsp::{Adsr, Env, FilterMode, Osc, Sum, Svf, Wave};
use super::engine::BLOCK;
use super::patch::{SynthPatch, VoiceMode};

/// Knobs a glicol patch can expose besides `#cutoff`.
pub const MAX_PARAMS: usize = 8;

/// References every voice provides.
const RESERVED: [&str; 4] = ["~freq", "~gate", "~trig", "~vel"];
const CUTOFF_REF: &str = "~p_cutoff";

/// A number, or a reference to another chain / input.
#[derive(Debug, Clone, PartialEq)]
pub enum Src {
    Num(f32),
    Ref(String),
}

/// One node of a chain, owned and validated.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Osc(Wave, Src),
    Noise,
    Sig(f32),
    Mul(Src),
    Add(Src),
    Filter(FilterMode, Src, f32),
    Env(Adsr),
    /// `envperc attack decay`: one-shot, fired by a rising edge.
    Perc(f32, f32),
    OnePole(f32),
    DelayMs(f32),
    Mix(Vec<String>),
    /// A reference used as a chain's input.
    Get(String),
}

impl Op {
    /// Whether this op produces sound by itself and must start a chain.
    fn is_source(&self) -> bool {
        matches!(self, Op::Osc(..) | Op::Noise | Op::Sig(_) | Op::Mix(_) | Op::Get(_))
    }

    /// References this op reads besides its chain input.
    fn side_refs(&self) -> Vec<&str> {
        match self {
            Op::Osc(_, Src::Ref(r)) | Op::Mul(Src::Ref(r)) | Op::Add(Src::Ref(r)) | Op::Filter(_, Src::Ref(r), _) => vec![r],
            Op::Mix(refs) => refs.iter().map(String::as_str).collect(),
            _ => vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub name: String,
    pub ops: Vec<Op>,
}

/// A parsed, validated glicol voice, ready to be built once per voice.
#[derive(Debug, Clone, PartialEq)]
pub struct GlicolProgram {
    pub source: String,
    pub lines: Vec<Line>,
    /// Exposed knobs (`#name`), in `SynthNode::params` order, with defaults.
    pub params: Vec<(String, f32)>,
    /// Default for `#cutoff`, if the patch uses it.
    pub cutoff: Option<f32>,
    /// Longest envelope release/decay: how long a released voice may ring.
    pub release: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GlicolError(pub String);

impl fmt::Display for GlicolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GlicolError {}

fn err<T>(msg: impl Into<String>) -> Result<T, GlicolError> {
    Err(GlicolError(msg.into()))
}

// ── Parsing ──────────────────────────────────────────────────────────────────

/// Parse a glicol voice patch into a [`SynthPatch`]. `fallback_name` names it
/// unless the code has a `// @name` directive.
pub fn parse_patch(source: &str, fallback_name: &str) -> Result<SynthPatch, GlicolError> {
    let mut patch = SynthPatch { name: fallback_name.to_string(), gain: 1.0, ..SynthPatch::new(fallback_name) };
    let mut defaults: HashMap<String, f32> = HashMap::new();

    for (n, line) in source.lines().enumerate() {
        let Some(directive) = line.trim().strip_prefix("//").map(str::trim).and_then(|d| d.strip_prefix('@')) else { continue };
        let mut words = directive.split_whitespace();
        let (key, value) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
        let number = |v: &str| v.parse::<f32>().map_err(|_| GlicolError(format!("line {}: `@{key}` needs a number, got `{v}`", n + 1)));
        match key {
            "name" => patch.name = directive["name".len()..].trim().to_string(),
            "mode" => {
                patch.mode = match value {
                    "poly" => VoiceMode::Poly,
                    "mono" => VoiceMode::Mono,
                    "legato" => VoiceMode::Legato,
                    other => return err(format!("line {}: unknown @mode `{other}` (poly, mono or legato)", n + 1)),
                }
            }
            "glide" => patch.glide = number(value)?,
            "gain" => patch.gain = number(value)?,
            "param" => {
                let default = number(words.next().unwrap_or(""))?;
                defaults.insert(value.to_string(), default);
            }
            other => return err(format!("line {}: unknown directive `@{other}`", n + 1)),
        }
    }

    let (code, knobs) = expand_knobs(source)?;
    let program = parse_program(source, &code, &knobs, &defaults)?;
    if let Some(cutoff) = program.cutoff {
        patch.cutoff = cutoff;
    }
    patch.glicol = Some(Arc::new(program));
    Ok(patch)
}

/// Replace `#name` with `~p_name` (glicol has no `#`), collecting knob names in order.
fn expand_knobs(source: &str) -> Result<(String, Vec<String>), GlicolError> {
    let mut out = String::with_capacity(source.len());
    let mut knobs: Vec<String> = Vec::new();
    for line in source.lines() {
        // Leave comments (and directives) alone.
        let (code, comment) = match line.find("//") {
            Some(i) => line.split_at(i),
            None => (line, ""),
        };
        let mut chars = code.char_indices().peekable();
        while let Some((_, c)) = chars.next() {
            if c != '#' {
                out.push(c);
                continue;
            }
            let mut name = String::new();
            while let Some(&(_, c)) = chars.peek() {
                if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' {
                    name.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            if name.is_empty() || !name.starts_with(|c: char| c.is_ascii_lowercase()) {
                return err(format!("`#{name}`: knob names are lowercase letters, digits and `_`, starting with a letter"));
            }
            if name != "cutoff" && !knobs.contains(&name) {
                if knobs.len() == MAX_PARAMS {
                    return err(format!("`#{name}`: at most {MAX_PARAMS} knobs besides #cutoff"));
                }
                knobs.push(name.clone());
            }
            out.push_str("~p_");
            out.push_str(&name);
        }
        out.push_str(comment);
        out.push('\n');
    }
    Ok((out, knobs))
}

fn src(value: &NumberOrRef<&str>) -> Src {
    match value {
        NumberOrRef::Number(n) => Src::Num(*n),
        NumberOrRef::Ref(r) => Src::Ref(r.to_string()),
    }
}

fn component_name(c: &Component) -> &'static str {
    match c {
        Component::Points(_) => "points",
        Component::Delayn(_) => "delayn",
        Component::Imp(_) => "imp",
        Component::Pan(_) => "pan",
        Component::Seq(_) => "seq",
        Component::Choose(_) => "choose",
        Component::Arrange(_) => "arrange",
        Component::Sp(_) => "sp",
        Component::Speed(_) => "speed",
        Component::Adc(_) => "adc",
        Component::Bd(_) => "bd",
        Component::Sn(_) => "sn",
        Component::Hh(_) => "hh",
        Component::SawSynth(_) => "sawsynth",
        Component::SquSynth(_) => "squsynth",
        Component::TriSynth(_) => "trisynth",
        Component::MsgSynth(_) => "msgsynth",
        Component::PatternSynth(_) => "psynth",
        Component::PSampler(_) => "psampler",
        Component::Balance(_) => "balance",
        Component::ApfmsGain(_) => "apfmsgain",
        Component::Reverb(_) => "reverb",
        Component::Plate(_) => "plate",
        Component::Meta(_) => "meta",
        Component::Expr(_) => "expr",
        Component::Eval(_) => "eval",
        _ => "this node",
    }
}

fn hint(name: &str) -> &'static str {
    match name {
        "imp" => " (use ~trig: an impulse at each note start)",
        "bd" | "sn" | "hh" => " (use ~trig >> envperc …, or the kick/snare/hat presets)",
        "reverb" | "plate" | "pan" | "balance" => " (effects and panning belong on the mixer: rusty_music::mix)",
        "seq" | "choose" | "arrange" | "psynth" | "msgsynth" | "sawsynth" | "squsynth" | "trisynth" => {
            " (notes come from the musician: use ~freq and ~gate)"
        }
        _ => "",
    }
}

fn parse_program(original: &str, code: &str, knobs: &[String], defaults: &HashMap<String, f32>) -> Result<GlicolProgram, GlicolError> {
    let ast = glicol_parser::get_ast(code).map_err(|e| GlicolError(format!("glicol syntax error:\n{e}")))?;

    let mut lines = Vec::new();
    let mut release: f32 = 0.0;
    for (name, components) in &ast.nodes {
        let mut ops = Vec::new();
        for c in components {
            let op = match c {
                Component::Sin(n) => Op::Osc(Wave::Sine, src(&n.param)),
                Component::Saw(n) => Op::Osc(Wave::Saw, src(&n.param)),
                Component::Squ(n) => Op::Osc(Wave::Square, src(&n.param)),
                Component::Tri(n) => Op::Osc(Wave::Triangle, src(&n.param)),
                Component::Noise(_) => Op::Noise,
                Component::ConstSig(n) => Op::Sig(n.value),
                Component::Mul(n) => Op::Mul(src(&n.param)),
                Component::Add(n) => Op::Add(src(&n.param)),
                Component::Lpf(n) => {
                    let cutoff = match &n.signal {
                        Signal::Number(v) => Src::Num(*v),
                        Signal::Reference(r) => Src::Ref(r.to_string()),
                        _ => return err(format!("{name}: lpf patterns aren't supported in voice patches; use a number or reference")),
                    };
                    Op::Filter(FilterMode::LowPass, cutoff, n.qvalue)
                }
                Component::Rhpf(n) => Op::Filter(FilterMode::HighPass, src(&n.cutoff), n.qvalue),
                Component::Adsr(a) => {
                    release = release.max(a.release);
                    Op::Env(Adsr::new(a.attack, a.decay, a.sustain, a.release))
                }
                Component::EnvPerc(e) => {
                    release = release.max(e.decay);
                    Op::Perc(e.attack, e.decay)
                }
                Component::Onepole(n) => match src(&n.param) {
                    Src::Num(rate) => Op::OnePole(rate),
                    Src::Ref(_) => return err(format!("{name}: onepole takes a number here")),
                },
                Component::Delayms(n) => match src(&n.param) {
                    Src::Num(ms) => Op::DelayMs(ms),
                    Src::Ref(_) => return err(format!("{name}: delayms takes a number here")),
                },
                Component::Mix(m) => Op::Mix(m.nodes.iter().map(|r| r.to_string()).collect()),
                Component::Get(g) => Op::Get(g.reference.to_string()),
                other => {
                    let n = component_name(other);
                    return err(format!("{name}: `{n}` isn't supported in voice patches{}", hint(n)));
                }
            };
            ops.push(op);
        }
        for (k, op) in ops.iter().enumerate() {
            if k == 0 && !op.is_source() {
                return err(format!("{name}: a chain must start with a source (an oscillator, noise, sig, mix or a reference like ~gate)"));
            }
            if k > 0 && op.is_source() {
                return err(format!("{name}: sources like oscillators can't take an input; start a new line for it"));
            }
            if let Op::Mix(refs) = op
                && refs.len() > 4
            {
                return err(format!("{name}: mix takes at most 4 references"));
            }
        }
        lines.push(Line { name: name.to_string(), ops });
    }
    // Stable order (the parser's map isn't), so equal sources give equal programs.
    lines.sort_by(|a, b| a.name.cmp(&b.name));

    validate_refs(&lines)?;
    if !lines.iter().any(|l| !l.name.starts_with('~')) {
        return err("no output: name at least one line without `~`, like `out: …`");
    }
    let uses_cutoff = original.contains("#cutoff");
    Ok(GlicolProgram {
        source: original.to_string(),
        lines,
        params: knobs.iter().map(|k| (k.clone(), defaults.get(k).copied().unwrap_or(0.5))).collect(),
        cutoff: uses_cutoff.then(|| defaults.get("cutoff").copied().unwrap_or(2_000.0)),
        release: if release > 0.0 { release } else { 0.05 },
    })
}

/// Every reference must exist, and chains may not depend on themselves.
fn validate_refs(lines: &[Line]) -> Result<(), GlicolError> {
    let defined: HashMap<&str, &Line> = lines.iter().map(|l| (l.name.as_str(), l)).collect();
    let deps = |line: &Line| -> Vec<String> {
        line.ops
            .iter()
            .flat_map(|op| {
                let mut refs: Vec<String> = op.side_refs().into_iter().map(str::to_string).collect();
                if let Op::Get(r) = op {
                    refs.push(r.clone());
                }
                refs
            })
            .collect()
    };
    for line in lines {
        for r in deps(line) {
            let known = RESERVED.contains(&r.as_str()) || r.starts_with("~p_") || defined.contains_key(r.as_str());
            if !known {
                return err(format!("{}: unknown reference `{r}` (inputs are {}, or define `{r}: …`)", line.name, RESERVED.join(" ")));
            }
        }
    }
    // Depth-first search for cycles.
    fn visit<'a>(
        name: &'a str,
        defined: &HashMap<&'a str, &'a Line>,
        deps: &dyn Fn(&Line) -> Vec<String>,
        stack: &mut Vec<String>,
        done: &mut HashSet<String>,
    ) -> Result<(), GlicolError> {
        if done.contains(name) {
            return Ok(());
        }
        if stack.iter().any(|s| s == name) {
            stack.push(name.to_string());
            return err(format!("feedback loop: {}", stack.join(" → ")));
        }
        let Some(line) = defined.get(name) else { return Ok(()) };
        stack.push(name.to_string());
        for dep in deps(line) {
            if let Some((key, _)) = defined.get_key_value(dep.as_str()) {
                visit(key, defined, deps, stack, done)?;
            }
        }
        stack.pop();
        done.insert(name.to_string());
        Ok(())
    }
    let mut done = HashSet::new();
    for line in lines {
        visit(&line.name, &defined, &deps, &mut Vec::new(), &mut done)?;
    }
    Ok(())
}

// ── Building ─────────────────────────────────────────────────────────────────

/// The glicol nodes feeding a voice's reserved references and knobs.
pub struct Inputs {
    pub freq: NodeIndex,
    pub gate: NodeIndex,
    pub trig: NodeIndex,
    pub vel: NodeIndex,
    pub cutoff: NodeIndex,
    pub params: [NodeIndex; MAX_PARAMS],
}

/// Build `program` into `ctx`; returns the node carrying the voice's output.
pub fn build(program: &GlicolProgram, ctx: &mut AudioContext<BLOCK>, sr: usize, inputs: &Inputs) -> NodeIndex {
    let mut fixed: HashMap<String, NodeIndex> = HashMap::from([
        ("~freq".to_string(), inputs.freq),
        ("~gate".to_string(), inputs.gate),
        ("~trig".to_string(), inputs.trig),
        ("~vel".to_string(), inputs.vel),
        (CUTOFF_REF.to_string(), inputs.cutoff),
    ]);
    for (k, (name, _)) in program.params.iter().enumerate() {
        fixed.insert(format!("~p_{name}"), inputs.params[k]);
    }

    // Pass 1: create every node.
    let nodes: Vec<Vec<Option<NodeIndex>>> = program
        .lines
        .iter()
        .map(|line| {
            line.ops
                .iter()
                .map(|op| match op {
                    Op::Osc(wave, freq) => {
                        let osc = Osc::new(*wave, sr);
                        let osc = match freq {
                            Src::Num(hz) => osc.fixed(*hz),
                            Src::Ref(_) => osc,
                        };
                        Some(ctx.add_mono_node(osc))
                    }
                    Op::Noise => Some(ctx.add_mono_node(Osc::new(Wave::Noise, sr))),
                    Op::Sig(v) => Some(ctx.add_mono_node(ConstSig::new(*v))),
                    Op::Mul(s) => Some(ctx.add_mono_node(Mul::new(if let Src::Num(v) = s { *v } else { 0.0 }))),
                    Op::Add(s) => Some(ctx.add_mono_node(Add::new(if let Src::Num(v) = s { *v } else { 0.0 }))),
                    Op::Filter(mode, cutoff, q) => Some(ctx.add_mono_node(match cutoff {
                        Src::Num(hz) => Svf::new(super::dsp::FilterMod { mode: *mode, ..Default::default() }, *hz, *q, sr),
                        Src::Ref(_) => Svf::driven(*mode, *q, sr),
                    })),
                    Op::Env(adsr) => Some(ctx.add_mono_node(Env::new(*adsr, sr))),
                    Op::Perc(attack, decay) => Some(ctx.add_mono_node(Env::perc(*attack, *decay, sr))),
                    Op::OnePole(rate) => Some(ctx.add_mono_node(OnePole::new(*rate))),
                    Op::DelayMs(ms) => Some(ctx.add_mono_node(DelayMs::new().delay(*ms, 1).sr(sr))),
                    Op::Mix(_) => Some(ctx.add_mono_node(Sum::default())),
                    Op::Get(_) => None,
                })
                .collect()
        })
        .collect();

    let line_index: HashMap<&str, usize> = program.lines.iter().enumerate().map(|(i, l)| (l.name.as_str(), i)).collect();
    // A line's output: its last node, or what its lone reference points at.
    fn resolve(
        name: &str,
        fixed: &HashMap<String, NodeIndex>,
        program: &GlicolProgram,
        nodes: &[Vec<Option<NodeIndex>>],
        line_index: &HashMap<&str, usize>,
    ) -> NodeIndex {
        if let Some(n) = fixed.get(name) {
            return *n;
        }
        let i = line_index[name];
        match nodes[i].iter().rev().flatten().next() {
            Some(n) => *n,
            // Validation guarantees a chain of only a reference points somewhere acyclic.
            None => match &program.lines[i].ops[0] {
                Op::Get(r) => resolve(r, fixed, program, nodes, line_index),
                _ => unreachable!("validated"),
            },
        }
    }
    let resolve = |name: &str| resolve(name, &fixed, program, &nodes, &line_index);

    // Pass 2: chain inputs (port 0) first...
    for (line, line_nodes) in program.lines.iter().zip(&nodes) {
        let mut prev: Option<NodeIndex> = None;
        for (op, node) in line.ops.iter().zip(line_nodes) {
            match (op, node) {
                (Op::Get(r), _) => prev = Some(resolve(r)),
                (_, Some(n)) => {
                    if let Some(p) = prev {
                        ctx.connect(p, *n);
                    }
                    prev = Some(*n);
                }
                _ => {}
            }
        }
    }
    // Pass 3: ...then the referenced side inputs, so they land on the later ports.
    for (line, line_nodes) in program.lines.iter().zip(&nodes) {
        for (op, node) in line.ops.iter().zip(line_nodes) {
            if let Some(n) = node {
                for r in op.side_refs() {
                    ctx.connect(resolve(r), *n);
                }
            }
        }
    }

    let outputs: Vec<NodeIndex> = program.lines.iter().filter(|l| !l.name.starts_with('~')).map(|l| resolve(&l.name)).collect();
    if outputs.len() == 1 {
        return outputs[0];
    }
    let sum = ctx.add_mono_node(Sum::default());
    for out in outputs.into_iter().take(4) {
        ctx.connect(out, sum);
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACID: &str = "\
// @name acid
// @mode legato
// @glide 0.06
// @param env 2.5
~env: ~gate >> envperc 0.002 0.2 >> mul #env >> add 1
~cut: ~env >> mul #cutoff
out: saw ~freq >> lpf ~cut 6.0 >> mul ~amp
~amp: ~gate >> adsr 0.002 0.3 0.6 0.05
";

    #[test]
    fn parses_directives_knobs_and_lines() {
        let patch = parse_patch(ACID, "file").unwrap();
        assert_eq!(patch.name, "acid");
        assert_eq!(patch.mode, VoiceMode::Legato);
        assert_eq!(patch.glide, 0.06);
        let program = patch.glicol.as_ref().unwrap();
        assert_eq!(program.params, vec![("env".to_string(), 2.5)]);
        assert_eq!(program.cutoff, Some(2_000.0));
        assert_eq!(program.lines.len(), 4);
        assert!((program.release - 0.2).abs() < 1e-6, "longest envelope tail");
    }

    #[test]
    fn helpful_errors() {
        let e = |code: &str| parse_patch(code, "t").unwrap_err().0;
        assert!(e("out: saw ~frq").contains("unknown reference `~frq`"));
        assert!(e("out: saw ~freq >> reverb 0.5 0.5 0.5 0.5 0.5").contains("mixer"));
        assert!(e("~a: ~freq >> mul 2").contains("no output"));
        assert!(e("~a: ~b >> mul 2\n~b: ~a >> mul 2\nout: ~a").contains("feedback loop"));
        assert!(e("out: ~gate >> adsr 0.1 0.1 0.5 0.1 >> saw 440").contains("can't take an input"));
        assert!(e("// @mode wobbly\nout: saw ~freq").contains("unknown @mode"));
        assert!(e("out: saw ~freq >>> mul 2").contains("syntax error"));
        assert!(e("out: saw ~freq >> mul #Bad").contains("knob names"));
    }

    #[test]
    fn comments_keep_their_hashes() {
        let (code, knobs) = expand_knobs("out: sin ~freq // #notaknob\n").unwrap();
        assert!(knobs.is_empty());
        assert!(code.contains("// #notaknob"));
    }
}
