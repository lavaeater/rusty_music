//! Improvising musicians: the ideas from `examples/improv_orchestra.rs`
//! (Game Audio Programming 3, ch. 14) as reusable players that work over any
//! [`Chord`] — any key, any mode, hand-written or from [`Harmony`](crate::harmony::Harmony).
//!
//! - [`MotifSoloist`]: invents a 2-bar motif and *develops* it (repeat,
//!   sequence, inversion, retrograde, fragmentation, ornament, cadence),
//!   bending it onto chord tones on strong beats. Motifs mutate each chorus.
//! - [`WalkingBass`]: a remembered rhythm per section over chord tones, with
//!   chromatic approach notes into the next bar's chord.
//! - [`VoiceLedPad`]: held chords or rhythmic stabs that move each voice to the
//!   nearest tone of the next chord; more voices as intensity rises.
//! - [`ChimeArp`]: arpeggios over chord tones in seeded modes or little index cells.
//! - [`GrooveDrummer`]: Euclidean grooves per section with ghost notes, hats,
//!   and a fresh fill at every section end.
//! - [`Drone`]: a held root per bar, for things that hum.
//!
//! All of them are deterministic for a given seed (and the same intensity
//! moves). They share a simple song form: sections of `section_bars` bars
//! (default 8) following a `form` such as AABA; part A and part B get
//! different material. Pitches are `NoteEvent::from_diff` offsets around
//! middle C — transpose them on the instrument (bass `-24`, lead `+12`).

use rand::rngs::StdRng;
use rand::seq::IndexedRandom;
use rand::{RngExt, SeedableRng};

use crate::clock::Beat;
use crate::instrument::NoteEvent;
use crate::musicians::{Chord, MusicPlayer, TonalPlayer};

/// Default form: two A sections, a contrasting B, A again.
pub const AABA: [usize; 4] = [0, 0, 1, 0];

// ── Shared helpers ───────────────────────────────────────────────────────────

fn pick<T: Copy>(rng: &mut StdRng, options: &[(T, f32)]) -> T {
    options.choose_weighted(rng, |o| o.1).map(|o| o.0).unwrap_or(options[0].0)
}

/// How strongly a 16th step (within a 2-bar span) is felt: downbeat > half bar >
/// quarters > eighths > sixteenths. The book's strong/weak beats as a number.
pub fn metric_weight(step: u32) -> f32 {
    let w = match step % 16 {
        0 => 1.0,
        8 => 0.8,
        4 | 12 => 0.6,
        s if s % 2 == 0 => 0.35,
        _ => 0.15,
    };
    if step >= 16 { w * 0.9 } else { w }
}

/// Where a bar sits in the song form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormPosition {
    /// Which material (0 = A, 1 = B, …).
    pub part: usize,
    pub bar_in_section: u32,
    pub section_bars: u32,
    /// How many times the whole form has gone round.
    pub chorus: u32,
}

impl FormPosition {
    pub fn of(bar: u32, form: &[usize], section_bars: u32) -> Self {
        let section_bars = section_bars.max(1);
        let form_bars = section_bars * form.len().max(1) as u32;
        let section = ((bar % form_bars) / section_bars) as usize;
        Self {
            part: form.get(section).copied().unwrap_or(0).min(1),
            bar_in_section: bar % section_bars,
            section_bars,
            chorus: bar / form_bars,
        }
    }

    pub fn is_last_bar(&self) -> bool {
        self.bar_in_section == self.section_bars - 1
    }
}

/// A chord seen the way an improviser needs it: tones, root and scale as
/// pitch classes, with scale *positions* for stepwise melody.
#[derive(Debug, Clone, PartialEq)]
pub struct ChordView {
    /// Root as a `midi_note_diff` (semitones from middle C).
    pub root: i32,
    /// Pitch classes of the main tones (strength ≥ 0.75): root, third, fifth.
    pub tones: Vec<i32>,
    /// Every chord pitch class, including colour tones (sevenths, ninths).
    pub colours: Vec<i32>,
    /// The scale, as sorted pitch classes (falls back to the chord's).
    pub scale: Vec<i32>,
}

impl ChordView {
    pub fn of(chord: &Chord) -> Self {
        let pc = |d: i32| d.rem_euclid(12);
        let mut tones = Vec::new();
        let mut colours = Vec::new();
        for n in &chord.chord_notes {
            let p = pc(n.midi_note_diff);
            if n.strength >= 0.75 && !tones.contains(&p) {
                tones.push(p);
            }
            if !colours.contains(&p) {
                colours.push(p);
            }
        }
        let root = chord.chord_notes.first().map_or(0, |n| n.midi_note_diff);
        if tones.is_empty() {
            tones.push(pc(root));
        }
        let mut scale: Vec<i32> = chord.scale_notes.iter().map(|n| pc(n.midi_note_diff)).chain(tones.iter().copied()).collect();
        scale.sort_unstable();
        scale.dedup();
        if scale.len() < 5 {
            scale = colours.clone();
            scale.sort_unstable();
        }
        Self { root, tones, colours, scale }
    }

    /// The pitch (`midi_note_diff`) of scale position `pos`; 0 is the first
    /// scale tone at or above middle C, and `scale.len()` positions make an octave.
    pub fn pitch(&self, pos: i32) -> i32 {
        let n = self.scale.len() as i32;
        self.scale[pos.rem_euclid(n) as usize] + 12 * pos.div_euclid(n)
    }

    pub fn octave(&self) -> i32 {
        self.scale.len() as i32
    }

    pub fn is_tone(&self, pitch: i32, colours: bool) -> bool {
        let set = if colours { &self.colours } else { &self.tones };
        set.contains(&pitch.rem_euclid(12))
    }

    /// Nearest scale position to `pos` that is a chord tone.
    pub fn snap_to_tone(&self, pos: i32) -> i32 {
        [0, 1, -1, 2, -2, 3, -3].into_iter().map(|o| pos + o).find(|&p| self.is_tone(self.pitch(p), false)).unwrap_or(pos)
    }

    /// Nearest scale position to `pos` that is the root.
    pub fn snap_to_root(&self, pos: i32) -> i32 {
        let root = self.root.rem_euclid(12);
        (0..=self.octave())
            .flat_map(|o| [pos + o, pos - o])
            .find(|&p| self.pitch(p).rem_euclid(12) == root)
            .unwrap_or(pos)
    }

    /// The root folded into -5..=6 around middle C, so bass roots stay in one register.
    pub fn root_near_c(&self) -> i32 {
        let r = self.root.rem_euclid(12);
        if r > 6 { r - 12 } else { r }
    }
}

// ── Motifs ───────────────────────────────────────────────────────────────────

/// A motif spans two bars of 16ths.
pub const MOTIF_STEPS: u32 = 32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotifNote {
    pub step: u32,
    /// Scale steps from the previous note (the first note's is ignored).
    pub interval: i32,
    /// 0–1: how essential the note is; low-weight notes drop out at low intensity.
    pub weight: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Motif(pub Vec<MotifNote>);

/// Melodic contour step, favouring stepwise motion and filling gaps after leaps.
fn contour(rng: &mut StdRng, prev: i32) -> i32 {
    if prev.abs() >= 3 {
        return -prev.signum();
    }
    pick(rng, &[(-1, 30.0), (1, 30.0), (-2, 10.0), (2, 10.0), (0, 6.0), (-3, 4.0), (3, 4.0), (4, 2.0), (-4, 2.0)])
}

/// Ways to develop a motif, from the book's list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Transform {
    Statement,
    Repeat,
    Sequence(i32),
    Inversion,
    Retrograde,
    Fragment,
    Ornament,
    Thin,
    Cadence,
}

impl Motif {
    pub fn generate(rng: &mut StdRng, density: f32) -> Self {
        let mut notes = Vec::new();
        let mut prev = 0;
        // Leave the last beat open so phrases breathe.
        for step in 0..MOTIF_STEPS - 4 {
            let w = metric_weight(step);
            if step != 0 && rng.random::<f32>() >= w * density {
                continue;
            }
            let interval = if notes.is_empty() { 0 } else { contour(rng, prev) };
            prev = interval;
            let weight = if step == 0 { 1.0 } else { (w * rng.random_range(0.7..1.4)).min(1.0) };
            notes.push(MotifNote { step, interval, weight });
        }
        Motif(notes)
    }

    /// Keep only notes matching `keep`, folding dropped intervals into the next
    /// kept note so the contour still lands where it would have.
    fn filtered(&self, keep: impl Fn(&MotifNote) -> bool) -> Self {
        let mut out = Vec::new();
        let mut carry = 0;
        for (i, n) in self.0.iter().enumerate() {
            carry += n.interval;
            if i == 0 || keep(n) {
                out.push(MotifNote { interval: if i == 0 { 0 } else { carry }, ..*n });
                carry = 0;
            }
        }
        Motif(out)
    }

    pub fn transform(&self, t: Transform, rng: &mut StdRng) -> Self {
        let mut m = self.clone();
        match t {
            Transform::Statement | Transform::Repeat | Transform::Sequence(_) => {}
            Transform::Inversion => m.0.iter_mut().for_each(|n| n.interval = -n.interval),
            Transform::Retrograde => {
                // Same rhythm, contour played backwards.
                let mut ints: Vec<i32> = m.0.iter().skip(1).map(|n| -n.interval).collect();
                ints.reverse();
                m.0.iter_mut().skip(1).zip(ints).for_each(|(n, i)| n.interval = i);
            }
            Transform::Fragment => {
                // First half, then again a step higher or lower.
                let head: Vec<MotifNote> = m.0.iter().filter(|n| n.step < 16).copied().collect();
                let travelled: i32 = head.iter().skip(1).map(|n| n.interval).sum();
                let shift = if rng.random_bool(0.5) { 1 } else { -1 };
                let mut tail: Vec<MotifNote> = head.iter().map(|n| MotifNote { step: n.step + 16, ..*n }).collect();
                if let Some(first) = tail.first_mut() {
                    first.interval = shift - travelled;
                }
                m.0 = head.into_iter().chain(tail).collect();
            }
            Transform::Ornament => {
                // Neighbour notes in the gaps: up-and-back embellishments.
                let mut out = Vec::new();
                for (i, n) in self.0.iter().enumerate() {
                    out.push(*n);
                    let Some(next) = self.0.get(i + 1) else { continue };
                    if next.step - n.step >= 3 && rng.random_bool(0.6) {
                        let dir = if rng.random_bool(0.5) { 1 } else { -1 };
                        out.push(MotifNote { step: n.step + 1, interval: dir, weight: 0.3 });
                        // Undo the neighbour step so the next note lands as before.
                        out.push(MotifNote { interval: next.interval - dir, ..*next });
                    }
                }
                // The loop pushed every "next" twice where it embellished; dedupe by step.
                out.dedup_by_key(|n| n.step);
                m.0 = out;
            }
            Transform::Thin => m = self.filtered(|n| n.weight >= 0.55),
            Transform::Cadence => {
                // Opening of the idea, then a long arrival on beat 3 of bar 2.
                let mut head = self.filtered(|n| n.step < 20);
                head.0.push(MotifNote { step: 24, interval: 0, weight: 1.0 });
                m = head;
            }
        }
        m
    }

    /// Small, memorable change between choruses: one interval or one onset.
    pub fn mutate(&mut self, rng: &mut StdRng) {
        if self.0.len() > 2 && rng.random_bool(0.6) {
            let k = rng.random_range(1..self.0.len());
            self.0[k].interval = contour(rng, 0);
        } else {
            let step = rng.random_range(1..MOTIF_STEPS - 4);
            if let Some(i) = self.0.iter().position(|n| n.step == step) {
                self.0.remove(i);
            } else {
                let at = self.0.iter().position(|n| n.step > step).unwrap_or(self.0.len());
                let interval = contour(rng, 0);
                self.0.insert(at, MotifNote { step, interval, weight: metric_weight(step) });
            }
        }
    }
}

// ── Soloist ──────────────────────────────────────────────────────────────────

/// A melody that develops motifs: each section is a run of 2-bar statements,
/// idea → repetition/sequence → development → cadence.
pub struct MotifSoloist {
    rng: StdRng,
    pub form: Vec<usize>,
    pub section_bars: u32,
    /// Lowest and highest pitch (`midi_note_diff`) the melody may use.
    pub range: (i32, i32),
    /// Below this intensity the soloist rests.
    pub enter: f32,
    motifs: [Motif; 2],
    current: Motif,
    transform: Transform,
    statement_key: Option<u32>,
    chorus: Option<u32>,
    anchor: i32,
    pos: i32,
}

impl MotifSoloist {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: StdRng::seed_from_u64(seed),
            form: AABA.to_vec(),
            section_bars: 8,
            range: (-5, 17),
            enter: 0.2,
            motifs: [Motif::default(), Motif::default()],
            current: Motif::default(),
            transform: Transform::Statement,
            statement_key: None,
            chorus: None,
            anchor: 0,
            pos: 0,
        }
    }

    pub fn with_form(self, form: &[usize], section_bars: u32) -> Self {
        Self { form: form.to_vec(), section_bars, ..self }
    }

    /// The development being played right now.
    pub fn transform(&self) -> Transform {
        self.transform
    }

    fn new_chorus(&mut self, chorus: u32) {
        match self.chorus {
            None => {
                let density = self.rng.random_range(0.55..1.1);
                self.motifs[0] = Motif::generate(&mut self.rng, density);
                // The bridge contrasts: busier if A is sparse and vice versa.
                let b_density = if self.motifs[0].0.len() > 7 { 0.5 } else { 1.15 };
                self.motifs[1] = Motif::generate(&mut self.rng, b_density);
            }
            Some(_) => {
                self.motifs[0].mutate(&mut self.rng);
                if chorus.is_multiple_of(2) {
                    let density = self.rng.random_range(0.5..1.2);
                    self.motifs[1] = Motif::generate(&mut self.rng, density);
                }
            }
        }
        self.chorus = Some(chorus);
    }

    fn plan(&mut self, statement: u32, statements: u32, part: usize, view: &ChordView, intensity: f32) {
        let rng = &mut self.rng;
        self.transform = if statement == 0 {
            Transform::Statement
        } else if statement + 1 == statements {
            Transform::Cadence
        } else if statement == 1 {
            pick(rng, &[(Transform::Repeat, 3.0), (Transform::Sequence(1), 2.0), (Transform::Sequence(-1), 2.0), (Transform::Sequence(2), 1.0)])
        } else {
            pick(rng, &[
                (Transform::Fragment, 3.0),
                (Transform::Inversion, 2.0),
                (Transform::Retrograde, 2.0),
                (Transform::Ornament, 0.5 + 3.0 * intensity),
                (Transform::Thin, 3.0 * (1.0 - intensity) + 0.01),
            ])
        };
        self.anchor = match self.transform {
            Transform::Statement => view.snap_to_tone(if part == 0 { view.octave() / 2 } else { view.octave() / 2 + 2 }),
            Transform::Sequence(n) => self.anchor + n,
            _ => self.anchor,
        };
        self.current = self.motifs[part].transform(self.transform, rng);
    }
}

impl MusicPlayer for MotifSoloist {
    fn play(&mut self, beat: Beat, out: &mut Vec<NoteEvent>, intensity: f32, chord: &Chord) {
        let at = FormPosition::of(beat.bar_count, &self.form, self.section_bars);
        if self.chorus != Some(at.chorus) {
            self.new_chorus(at.chorus);
        }
        let view = ChordView::of(chord);
        let statement = at.bar_in_section / 2;
        if self.statement_key != Some(beat.bar_count / 2) {
            self.statement_key = Some(beat.bar_count / 2);
            self.plan(statement, (at.section_bars / 2).max(1), at.part, &view, intensity);
        }

        let step = (at.bar_in_section % 2) * 16 + TonalPlayer::flat_step(&beat);
        let Some(idx) = self.current.0.iter().position(|n| n.step == step) else { return };
        let note = self.current.0[idx];

        // Walk the contour even for notes we skip, so the shape is preserved.
        self.pos = if idx == 0 { self.anchor } else { self.pos + note.interval };
        let last = idx == self.current.0.len() - 1;
        if last && self.transform == Transform::Cadence {
            self.pos = view.snap_to_root(self.pos);
        } else if step.is_multiple_of(8) {
            // Strong beats land on chord tones: the motif bends to the harmony.
            self.pos = view.snap_to_tone(self.pos);
        }
        // Fold back into range by octaves, which keeps chord tones chord tones.
        let octave = view.octave();
        while view.pitch(self.pos) > self.range.1 {
            self.pos -= octave;
        }
        while view.pitch(self.pos) < self.range.0 {
            self.pos += octave;
        }

        if intensity < self.enter || note.weight < 1.0 - intensity * 1.2 {
            return;
        }
        let gap = self.current.0.get(idx + 1).map_or(8, |n| n.step - step);
        let velocity = (0.55 + 0.45 * note.weight) * (0.6 + 0.4 * intensity);
        out.push(NoteEvent::from_diff(view.pitch(self.pos)).velocity(velocity).length(gap as f32));
    }
}

// ── Bass ─────────────────────────────────────────────────────────────────────

fn bass_rhythm(rng: &mut StdRng) -> [f32; 16] {
    let mut r = [0.0; 16];
    for (s, v) in r.iter_mut().enumerate() {
        let w = metric_weight(s as u32);
        if rng.random::<f32>() < w {
            *v = (w * rng.random_range(0.5..1.2)).min(1.0);
        }
    }
    r[0] = 1.0;
    r
}

/// Chord tones over a remembered rhythm per section, walking chromatically
/// into the next bar's chord. Plays around middle C: transpose it down.
pub struct WalkingBass {
    rng: StdRng,
    pub form: Vec<usize>,
    pub section_bars: u32,
    rhythm: [[f32; 16]; 2],
    chorus: Option<u32>,
}

impl WalkingBass {
    pub fn new(seed: u64) -> Self {
        let mut rng = StdRng::seed_from_u64(seed);
        let rhythm = [bass_rhythm(&mut rng), bass_rhythm(&mut rng)];
        Self { rng, form: AABA.to_vec(), section_bars: 8, rhythm, chorus: None }
    }

    pub fn with_form(self, form: &[usize], section_bars: u32) -> Self {
        Self { form: form.to_vec(), section_bars, ..self }
    }

    pub fn rhythm(&self, part: usize) -> &[f32; 16] {
        &self.rhythm[part.min(1)]
    }
}

impl MusicPlayer for WalkingBass {
    fn play(&mut self, beat: Beat, out: &mut Vec<NoteEvent>, intensity: f32, chord: &Chord) {
        self.play_with_next(beat, out, intensity, chord, chord);
    }

    fn play_with_next(&mut self, beat: Beat, out: &mut Vec<NoteEvent>, intensity: f32, chord: &Chord, next: &Chord) {
        let at = FormPosition::of(beat.bar_count, &self.form, self.section_bars);
        if self.chorus != Some(at.chorus) {
            if self.chorus.is_some() && at.chorus.is_multiple_of(2) {
                self.rhythm[1] = bass_rhythm(&mut self.rng);
            }
            self.chorus = Some(at.chorus);
        }
        if intensity < 0.13 {
            return;
        }
        let rhythm = self.rhythm[at.part];
        let s = TonalPlayer::flat_step(&beat) as usize;
        let view = ChordView::of(chord);
        let next = ChordView::of(next);
        let r = view.root_near_c();
        let third = view.tones.get(1).map_or(r, |t| r + (t - r).rem_euclid(12));
        let fifth = view.tones.get(2).map_or(r + 7, |t| r + (t - r).rem_euclid(12));

        let changes = next.root.rem_euclid(12) != view.root.rem_euclid(12);
        let approach = s == 14 && changes && intensity > 0.35 && self.rng.random_bool(0.6);
        let semis = if approach {
            // A semitone below or above where the next chord's root will be.
            let target = next.root_near_c();
            if self.rng.random_bool(0.6) { target - 1 } else { target + 1 }
        } else {
            if rhythm[s] == 0.0 || rhythm[s] < 1.0 - intensity {
                return;
            }
            if s == 0 { r } else { pick(&mut self.rng, &[(r, 3.0), (fifth, 3.0), (r + 12, 2.0), (third, 1.0)]) }
        };
        let gap = (s + 1..16).find(|&k| rhythm[k] > 0.0).map_or(16 - s, |k| k - s);
        out.push(NoteEvent::from_diff(semis).velocity(0.6 + 0.4 * intensity).length(gap as f32 * 0.9));
    }
}

// ── Pads ─────────────────────────────────────────────────────────────────────

/// Move each voice of `prev` to the nearest unused tone of `view` — classic
/// voice leading, so the harmony glides instead of jumping.
pub fn voice_lead(prev: &[i32], view: &ChordView, n: usize, center: i32, colours: bool) -> Vec<i32> {
    let pcs = if colours { &view.colours } else { &view.tones };
    let nearest = |pc: i32, to: i32| {
        let up = to + (pc - to).rem_euclid(12);
        if up - to > 6 { up - 12 } else { up }
    };
    let seeds: Vec<i32> = if prev.len() == n { prev.to_vec() } else { (0..n as i32).map(|k| center + k * 4).collect() };
    // Voices stay in a comfortable band around the centre.
    let in_band = |q: i32| (center - 8..=center + 12).contains(&q);
    let mut used: Vec<i32> = Vec::new();
    seeds
        .iter()
        .map(|&p| {
            let best = pcs
                .iter()
                .copied()
                .filter(|pc| !used.contains(pc))
                .flat_map(|pc| {
                    let q = nearest(pc, p);
                    [q - 12, q, q + 12]
                })
                .filter(|&q| in_band(q))
                .min_by_key(|q| (q - p).abs())
                .unwrap_or_else(|| nearest(pcs[0], p));
            used.push(best.rem_euclid(12));
            best
        })
        .collect()
}

fn stab_rhythm(rng: &mut StdRng) -> [f32; 16] {
    let mut r = [0.0; 16];
    let hits = rng.random_range(2..=5);
    for s in 0..16 {
        // Euclidean spread, rotated by a random 8th so stabs syncopate.
        if (s * hits) % 16 < hits {
            r[(s + 2 * rng.random_range(0..2)) % 16] = metric_weight(s as u32).max(0.3);
        }
    }
    r[0] = 1.0;
    r
}

/// Voice-led chords: held for the bar, or as rhythmic stabs. Enters above
/// `enter` intensity and adds voices (up to `max_voices`) as intensity rises;
/// above 0.6 it adds sevenths and other colour tones.
pub struct VoiceLedPad {
    rng: StdRng,
    pub form: Vec<usize>,
    pub section_bars: u32,
    /// Centre pitch of the voicing (`midi_note_diff`).
    pub center: i32,
    pub enter: f32,
    pub max_voices: usize,
    pub stabs: bool,
    rhythm: [[f32; 16]; 2],
    voicing: Vec<i32>,
    voiced_bar: Option<u32>,
}

impl VoiceLedPad {
    pub fn new(seed: u64) -> Self {
        let mut rng = StdRng::seed_from_u64(seed);
        let rhythm = [stab_rhythm(&mut rng), stab_rhythm(&mut rng)];
        Self {
            rng,
            form: AABA.to_vec(),
            section_bars: 8,
            center: 0,
            enter: 0.0,
            max_voices: 4,
            stabs: false,
            rhythm,
            voicing: Vec::new(),
            voiced_bar: None,
        }
    }

    /// Rhythmic stabs (brass, synth chords) instead of held chords.
    pub fn stabs(self) -> Self {
        Self { stabs: true, ..self }
    }

    pub fn enter(self, enter: f32) -> Self {
        Self { enter, ..self }
    }

    pub fn voices(self, max_voices: usize) -> Self {
        Self { max_voices: max_voices.max(1), ..self }
    }

    pub fn center(self, center: i32) -> Self {
        Self { center, ..self }
    }

    pub fn voicing(&self) -> &[i32] {
        &self.voicing
    }
}

impl MusicPlayer for VoiceLedPad {
    fn play(&mut self, beat: Beat, out: &mut Vec<NoteEvent>, intensity: f32, chord: &Chord) {
        if intensity < self.enter {
            return;
        }
        let at = FormPosition::of(beat.bar_count, &self.form, self.section_bars);
        if at.bar_in_section == 0 && TonalPlayer::flat_step(&beat) == 0 && at.chorus > 0 && at.chorus.is_multiple_of(2) {
            self.rhythm[1] = stab_rhythm(&mut self.rng);
        }
        let span = ((intensity - self.enter) / (1.0 - self.enter).max(0.01)).clamp(0.0, 1.0);
        let voices = 1 + (span * (self.max_voices - 1) as f32).round() as usize;
        let s = TonalPlayer::flat_step(&beat) as usize;

        let new_bar = self.voiced_bar != Some(beat.bar_count) || self.voicing.len() != voices;
        if new_bar {
            self.voiced_bar = Some(beat.bar_count);
            self.voicing = voice_lead(&self.voicing, &ChordView::of(chord), voices, self.center, intensity > 0.6);
        }
        let play = if self.stabs {
            let v = self.rhythm[at.part][s];
            v > 0.0 && v >= 1.0 - span
        } else {
            new_bar && s == 0
        };
        if !play {
            return;
        }
        let length = if self.stabs { 1.0 } else { 16.0 };
        for (k, &pitch) in self.voicing.iter().enumerate() {
            let velocity = (0.5 + 0.5 * intensity) * (1.0 - k as f32 * 0.08);
            out.push(NoteEvent::from_diff(pitch).velocity(velocity).length(length));
        }
    }
}

// ── Arpeggio ─────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArpMode {
    Up,
    Down,
    UpDown,
    Random,
    /// A remembered index cell, e.g. [0, 2, 1, 3] — a tiny motif over chord tones.
    Cell,
}

/// The book's "spacey chimes": chord tones in a seeded mode per section,
/// faster as intensity rises (quarters → eighths → sixteenths).
pub struct ChimeArp {
    rng: StdRng,
    pub form: Vec<usize>,
    pub section_bars: u32,
    /// Lowest pitch (`midi_note_diff`); it spans 16 semitones up from here.
    pub low: i32,
    pub enter: f32,
    modes: [ArpMode; 2],
    cells: [Vec<usize>; 2],
    chorus: Option<u32>,
    idx: usize,
}

impl ChimeArp {
    pub fn new(seed: u64) -> Self {
        let mut rng = StdRng::seed_from_u64(seed);
        let (a, b) = (Self::invent(&mut rng), Self::invent(&mut rng));
        Self {
            rng,
            form: AABA.to_vec(),
            section_bars: 8,
            low: 0,
            enter: 0.3,
            modes: [a.0, b.0],
            cells: [a.1, b.1],
            chorus: None,
            idx: 0,
        }
    }

    pub fn mode(&self, part: usize) -> ArpMode {
        self.modes[part.min(1)]
    }

    fn invent(rng: &mut StdRng) -> (ArpMode, Vec<usize>) {
        let mode = pick(rng, &[(ArpMode::Up, 2.0), (ArpMode::Down, 1.0), (ArpMode::UpDown, 2.0), (ArpMode::Random, 1.0), (ArpMode::Cell, 3.0)]);
        let cell = (0..rng.random_range(3..=6)).map(|_| rng.random_range(0..5)).collect();
        (mode, cell)
    }
}

impl MusicPlayer for ChimeArp {
    fn play(&mut self, beat: Beat, out: &mut Vec<NoteEvent>, intensity: f32, chord: &Chord) {
        let at = FormPosition::of(beat.bar_count, &self.form, self.section_bars);
        if self.chorus != Some(at.chorus) {
            if self.chorus.is_some() {
                let p = self.rng.random_range(0..2);
                (self.modes[p], self.cells[p]) = Self::invent(&mut self.rng);
            }
            self.chorus = Some(at.chorus);
        }
        if intensity < self.enter {
            return;
        }
        let rate = if intensity < 0.5 { 4 } else if intensity < 0.8 { 2 } else { 1 };
        if !TonalPlayer::flat_step(&beat).is_multiple_of(rate) {
            return;
        }
        let view = ChordView::of(chord);
        let pitches: Vec<i32> = (self.low..self.low + 16).filter(|&m| view.is_tone(m, intensity > 0.6)).collect();
        let len = pitches.len();
        if len == 0 {
            return;
        }
        let k = self.idx;
        self.idx = self.idx.wrapping_add(1);
        let i = match self.modes[at.part] {
            ArpMode::Up => k % len,
            ArpMode::Down => len - 1 - k % len,
            ArpMode::UpDown => {
                let period = (2 * len - 2).max(1);
                let p = k % period;
                if p < len { p } else { period - p }
            }
            ArpMode::Random => self.rng.random_range(0..len),
            ArpMode::Cell => {
                let cell = &self.cells[at.part];
                cell[k % cell.len()] % len
            }
        };
        out.push(NoteEvent::from_diff(pitches[i]).velocity(0.5 + 0.4 * intensity).length(rate as f32));
    }
}

// ── Drone ────────────────────────────────────────────────────────────────────

/// Holds the chord's root (plus `interval` semitones) for each whole bar: the
/// hum of a machine, a shrine, or an enemy (see `synth::Proximity`).
pub struct Drone {
    pub interval: i32,
}

impl MusicPlayer for Drone {
    fn play(&mut self, beat: Beat, out: &mut Vec<NoteEvent>, _intensity: f32, chord: &Chord) {
        if beat.beat == 0 && beat.sixteenth == 0 {
            let root = ChordView::of(chord).root_near_c();
            out.push(NoteEvent::from_diff(root + self.interval).length(16.0));
        }
    }
}

// ── Drums ────────────────────────────────────────────────────────────────────

/// Kit channels used by [`GrooveDrummer`]: build its instrument as
/// `Channels` (or a synth kit) in this order.
pub mod kit {
    pub const KICK: usize = 0;
    pub const SNARE: usize = 1;
    pub const HAT: usize = 2;
    /// Optional: pitched toms for fills (`GrooveDrummer::with_toms`).
    pub const TOM: usize = 3;
    /// Optional: a crash into each section (`GrooveDrummer::with_crash`).
    pub const CRASH: usize = 4;
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Hit {
    Snare,
    Tom(i32),
}

/// Euclidean kick (k hits spread over 16) + backbeat snare with ghost notes.
fn groove(rng: &mut StdRng, part: usize) -> ([f32; 16], [f32; 16]) {
    let k = rng.random_range(2..=5);
    let mut kick = [0.0; 16];
    for (s, v) in kick.iter_mut().enumerate() {
        if (s * k) % 16 < k {
            *v = (metric_weight(s as u32) + rng.random_range(0.0..0.3)).min(1.0);
        }
    }
    kick[0] = 1.0;
    let mut snare = [0.0; 16];
    if part == 1 && rng.random_bool(0.5) {
        snare[8] = 1.0; // half-time bridge
    } else {
        snare[4] = 1.0;
        snare[12] = 1.0;
    }
    for s in 0..16 {
        if snare[s] == 0.0 && kick[s] == 0.0 && rng.random_bool(0.3) {
            snare[s] = rng.random_range(0.05..0.3);
        }
    }
    (kick, snare)
}

/// A fill for a section's last bar: starts earlier and denser as intensity rises.
fn compose_fill(rng: &mut StdRng, intensity: f32, toms: bool) -> Vec<(usize, Hit)> {
    let start = if intensity > 0.75 { 4 } else if intensity > 0.5 { 8 } else { 12 };
    let use_toms = toms && rng.random_bool(0.5);
    let shape = pick(rng, &[(0u8, 2.0), (1, 1.0), (2, 1.0)]);
    let mut hits = Vec::new();
    for s in start..16 {
        let t = (s - start) as f32 / (16 - start) as f32;
        let p = match shape {
            0 => 0.3 + 0.7 * t,                                     // ramp
            1 => if (s - start) % 3 == 0 { 1.0 } else { 0.15 },   // threes
            _ => if s % 2 == 0 { 0.9 } else { 0.4 * intensity },  // straight
        };
        if rng.random::<f32>() < p {
            // Toms tumble downwards in pitch across the fill.
            hits.push((s, if use_toms { Hit::Tom(5 - (t * 10.0) as i32) } else { Hit::Snare }));
        }
    }
    hits
}

/// Generated grooves per section and a fresh fill at every section end.
/// Plays [`kit`] channels; velocity carries the dynamics.
pub struct GrooveDrummer {
    rng: StdRng,
    pub form: Vec<usize>,
    pub section_bars: u32,
    pub toms: bool,
    pub crash: bool,
    kick: [[f32; 16]; 2],
    snare: [[f32; 16]; 2],
    fill: Vec<(usize, Hit)>,
    fill_bar: Option<u32>,
    chorus: Option<u32>,
}

impl GrooveDrummer {
    pub fn new(seed: u64) -> Self {
        let mut rng = StdRng::seed_from_u64(seed);
        let (ka, sa) = groove(&mut rng, 0);
        let (kb, sb) = groove(&mut rng, 1);
        Self {
            rng,
            form: AABA.to_vec(),
            section_bars: 8,
            toms: false,
            crash: false,
            kick: [ka, kb],
            snare: [sa, sb],
            fill: Vec::new(),
            fill_bar: None,
            chorus: None,
        }
    }

    pub fn with_toms(self) -> Self {
        Self { toms: true, ..self }
    }

    pub fn with_crash(self) -> Self {
        Self { crash: true, ..self }
    }

    pub fn with_form(self, form: &[usize], section_bars: u32) -> Self {
        Self { form: form.to_vec(), section_bars, ..self }
    }

    pub fn kick(&self, part: usize) -> &[f32; 16] {
        &self.kick[part.min(1)]
    }
}

impl MusicPlayer for GrooveDrummer {
    fn play(&mut self, beat: Beat, out: &mut Vec<NoteEvent>, intensity: f32, _chord: &Chord) {
        use kit::*;
        let at = FormPosition::of(beat.bar_count, &self.form, self.section_bars);
        if self.chorus != Some(at.chorus) {
            if self.chorus.is_some() && at.chorus.is_multiple_of(2) {
                (self.kick[1], self.snare[1]) = groove(&mut self.rng, 1);
            }
            self.chorus = Some(at.chorus);
        }
        if intensity < 0.08 {
            return;
        }
        let p = at.part;
        let s = TonalPlayer::flat_step(&beat) as usize;
        let gate = 1.0 - intensity;
        let hit = |channel: usize, velocity: f32| NoteEvent::from_diff(0).velocity(velocity.clamp(0.05, 1.0)).channel(channel).length(1.0);

        if self.kick[p][s] > 0.0 && self.kick[p][s] >= gate {
            out.push(hit(KICK, 0.7 + 0.3 * self.kick[p][s]));
        }

        let fill_bar = at.is_last_bar() && intensity > 0.3;
        if fill_bar && self.fill_bar != Some(beat.bar_count) {
            self.fill_bar = Some(beat.bar_count);
            self.fill = compose_fill(&mut self.rng, intensity, self.toms);
        }
        let fill_start = self.fill.first().map_or(16, |(s, _)| *s);
        if fill_bar && s >= fill_start {
            if let Some(&(_, fill_hit)) = self.fill.iter().find(|(h, _)| *h == s) {
                let ramp = (s - fill_start) as f32 / 8.0;
                match fill_hit {
                    Hit::Snare => out.push(hit(SNARE, 0.55 + 0.45 * ramp)),
                    Hit::Tom(semis) => out.push(NoteEvent::from_diff(semis).velocity(0.6 + 0.4 * ramp).channel(TOM).length(1.0)),
                }
            }
        } else if self.snare[p][s] > 0.0 && self.snare[p][s] >= gate {
            let v = self.snare[p][s];
            out.push(hit(SNARE, if v >= 0.8 { 1.0 } else { 0.2 + v }));
        }

        // Hats: eighths from 0.3, sixteenths from 0.7, accented on the beat.
        let hat_rate = if intensity >= 0.7 { 1 } else if intensity >= 0.3 { 2 } else { 0 };
        if hat_rate > 0 && s.is_multiple_of(hat_rate) {
            out.push(hit(HAT, if s.is_multiple_of(4) { 0.8 } else { 0.45 }));
        }
        if self.crash && at.bar_in_section == 0 && s == 0 && intensity > 0.5 {
            out.push(hit(CRASH, 0.5 + 0.5 * intensity));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harmony::{Harmony, Mode};

    fn beat(bar: u32, step: u32) -> Beat {
        Beat { bar_count: bar, beat: step / 4, sixteenth: step % 4, ..Default::default() }
    }

    /// Run a player over `bars` bars of a progression, returning (bar, step, note).
    fn perform(player: &mut dyn MusicPlayer, chords: &[Chord], bars: u32, intensity: f32) -> Vec<(u32, u32, NoteEvent)> {
        let mut played = Vec::new();
        let mut notes = Vec::new();
        for bar in 0..bars {
            let chord = &chords[bar as usize % chords.len()];
            let next = &chords[(bar as usize + 1) % chords.len()];
            for step in 0..16 {
                notes.clear();
                player.play_with_next(beat(bar, step), &mut notes, intensity, chord, next);
                played.extend(notes.iter().map(|n| (bar, step, *n)));
            }
        }
        played
    }

    fn diff(n: &NoteEvent) -> i32 {
        (n.pitch - crate::instrument::SAMPLE_ROOT).round() as i32
    }

    #[test]
    fn chord_view_reads_harmony_chords() {
        let chords = Harmony { mode: Mode::Aeolian, ..Harmony::default() }.chords(0.0);
        let am = ChordView::of(&chords[0]);
        assert_eq!(am.root, -3);
        assert_eq!(am.tones, vec![9, 0, 4]); // A C E
        assert_eq!(am.scale, vec![0, 2, 4, 5, 7, 9, 11]); // A minor = C major's notes
        assert_eq!(am.pitch(0), 0);
        assert_eq!(am.pitch(7), 12);
        assert_eq!(am.pitch(-1), -1);
        assert!(am.is_tone(am.pitch(am.snap_to_tone(1)), false));
    }

    #[test]
    fn soloist_lands_on_chord_tones_on_strong_beats() {
        let chords = Harmony::default().chords(0.3);
        let mut solo = MotifSoloist::new(11);
        let played = perform(&mut solo, &chords, 64, 0.9);
        assert!(played.len() > 100, "plays plenty at high intensity: {}", played.len());
        for (bar, step, note) in &played {
            let view = ChordView::of(&chords[*bar as usize % chords.len()]);
            if step % 8 == 0 {
                assert!(view.is_tone(diff(note), false), "bar {bar} step {step}: {} not a chord tone", diff(note));
            }
            assert!((-5..=17).contains(&diff(note)), "in range");
        }
    }

    #[test]
    fn soloist_is_deterministic_and_quiet_at_low_intensity() {
        let chords = Harmony::default().chords(0.3);
        let a = perform(&mut MotifSoloist::new(5), &chords, 32, 0.7);
        let b = perform(&mut MotifSoloist::new(5), &chords, 32, 0.7);
        assert_eq!(a, b);
        assert!(perform(&mut MotifSoloist::new(5), &chords, 32, 0.1).is_empty());
    }

    #[test]
    fn bass_walks_chromatically_into_chord_changes() {
        let chords = Harmony::default().chords(0.0); // Am F C G: every bar changes
        let played = perform(&mut WalkingBass::new(3), &chords, 64, 0.9);
        let approaches: Vec<_> = played.iter().filter(|(_, step, _)| *step == 14).collect();
        assert!(!approaches.is_empty(), "some approach notes");
        for (bar, _, note) in approaches {
            let next_root = ChordView::of(&chords[(*bar as usize + 1) % 4]).root_near_c();
            let view = ChordView::of(&chords[*bar as usize % 4]);
            let d = diff(note);
            assert!((d - next_root).abs() == 1 || view.is_tone(d, false), "step 14 is an approach or a chord tone");
        }
        for (bar, step, note) in &played {
            if *step == 0 {
                assert_eq!(diff(note), ChordView::of(&chords[*bar as usize % 4]).root_near_c(), "downbeats are roots");
            }
        }
    }

    #[test]
    fn voice_leading_moves_little() {
        let chords = Harmony::default().chords(0.0);
        let (am, f) = (ChordView::of(&chords[0]), ChordView::of(&chords[1]));
        let first = voice_lead(&[], &am, 3, 0, false);
        let next = voice_lead(&first, &f, 3, 0, false);
        let moved: i32 = first.iter().zip(&next).map(|(a, b)| (a - b).abs()).sum();
        assert!(moved <= 4, "{first:?} → {next:?}");
        assert!(next.iter().all(|&m| f.is_tone(m, false)));
    }

    #[test]
    fn pads_add_voices_with_intensity() {
        let chords = Harmony::default().chords(0.0);
        let count = |i: f32| perform(&mut VoiceLedPad::new(1), &chords, 1, i).len();
        assert_eq!(count(0.0), 1);
        assert_eq!(count(1.0), 4);
    }

    #[test]
    fn arp_plays_only_chord_tones() {
        let chords = Harmony::default().chords(0.0);
        let played = perform(&mut ChimeArp::new(9), &chords, 16, 0.5);
        assert!(!played.is_empty());
        for (bar, _, note) in played {
            assert!(ChordView::of(&chords[bar as usize % 4]).is_tone(diff(&note), false));
        }
    }

    #[test]
    fn drummer_grooves_and_fills_at_section_ends() {
        let chords = Harmony::default().chords(0.0);
        let played = perform(&mut GrooveDrummer::new(4).with_toms(), &chords, 16, 0.8);
        // Every bar starts with a kick.
        for bar in 0..16 {
            assert!(played.iter().any(|(b, s, n)| *b == bar && *s == 0 && n.channel == kit::KICK));
        }
        // Fills (snare or toms in the back half) happen in section-ending bars.
        let fill_hits = |bar: u32| played.iter().filter(|(b, s, n)| *b == bar && *s >= 8 && matches!(n.channel, kit::SNARE | kit::TOM) && n.velocity > 0.5).count();
        assert!(fill_hits(7) > fill_hits(3), "bar 8 has a fill");
        assert!(played.iter().all(|(_, _, n)| n.channel <= kit::TOM));
    }

    #[test]
    fn transforms_keep_steps_sorted_unique_and_in_range() {
        let mut rng = StdRng::seed_from_u64(3);
        let all = [
            Transform::Statement, Transform::Repeat, Transform::Sequence(1), Transform::Inversion,
            Transform::Retrograde, Transform::Fragment, Transform::Ornament, Transform::Thin, Transform::Cadence,
        ];
        for _ in 0..200 {
            let m = Motif::generate(&mut rng, 1.0);
            for t in all {
                let out = m.transform(t, &mut rng);
                assert!(!out.0.is_empty());
                assert!(out.0.windows(2).all(|w| w[0].step < w[1].step), "{t:?}: {:?}", out.0);
                assert!(out.0.iter().all(|n| n.step < MOTIF_STEPS));
            }
        }
    }

    #[test]
    fn form_positions() {
        let at = |bar| FormPosition::of(bar, &AABA, 8);
        assert_eq!(at(0).part, 0);
        assert_eq!(at(16).part, 1);
        assert_eq!(at(24).part, 0);
        assert_eq!(at(33).chorus, 1);
        assert!(at(7).is_last_bar() && !at(8).is_last_bar());
    }
}
