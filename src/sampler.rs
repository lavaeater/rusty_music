//! Multisampled-instrument support for sample libraries that follow the
//! Philharmonia / Sonatina filename grammar:
//!
//! ```text
//! <instrument>_<pitch>_<duration>_<dynamic>_<articulation>.<ext>
//! violin_Ds5_1_piano_arco-sul-tasto.wav      (pitched)
//! snare-drum__025_fortissimo_with-snares.wav (unpitched — empty pitch field)
//! ```
//!
//! The point of multisampling is to avoid the artefacts of pitch-shifting one
//! recording across a wide range. With a real note recorded at (almost) every
//! semitone, a musician picks the *nearest recorded pitch* to its target and
//! shifts only the leftover semitone or two — usually zero.
//!
//! Two layers:
//!   * [`SampleLibrary`] — scans a folder once, parsing every filename into a
//!     [`SampleSpec`]. Pure metadata (just strings), so even tens of thousands
//!     of files are cheap. Dedupes `.wav`/`.mp3` pairs, preferring `.wav`.
//!   * [`MultiSampler`] — a *curated, loaded* subset for one instrument: a set
//!     of named [`Voice`]s (e.g. an "ostinato" voice and a "sustain" voice),
//!     each indexed by pitch for nearest-neighbour selection. Only the handles
//!     you actually ask for are loaded, so startup stays bounded.
//!
//! See `examples/real_orchestra.rs` for end-to-end usage.

use std::collections::BTreeMap;
use std::path::Path;

use bevy::asset::{AssetServer, Handle};
use bevy_seedling::prelude::AudioSample;

// ─────────────────────────────────────────────────────────────────────────────
//  Parsing helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Parse a scientific note name (`C4`, `Ds5`, `As0`) into a MIDI note number,
/// using the convention C4 = 60 (A4 = 69). Only sharps are used by this library
/// (no flats). Returns `None` for an empty or malformed token.
pub fn parse_note_to_midi(tok: &str) -> Option<i32> {
    let bytes = tok.as_bytes();
    let letter = match bytes.first()? {
        b'C' => 0,
        b'D' => 2,
        b'E' => 4,
        b'F' => 5,
        b'G' => 7,
        b'A' => 9,
        b'B' => 11,
        _ => return None,
    };
    let (semis, oct_start) = if bytes.get(1) == Some(&b's') {
        (letter + 1, 2)
    } else {
        (letter, 1)
    };
    let octave: i32 = tok.get(oct_start..)?.parse().ok()?;
    Some((octave + 1) * 12 + semis)
}

/// Standard orchestral dynamic levels, ordered soft → loud.
///
/// Swell tokens (`crescendo`, `decrescendo`, `cresc-decresc`) are *not* fixed
/// levels and parse to `None`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum Dynamic {
    Pianissimo,
    Piano,
    MezzoPiano,
    MezzoForte,
    Forte,
    Fortissimo,
}

impl Dynamic {
    pub fn from_token(tok: &str) -> Option<Self> {
        Some(match tok {
            "pianissimo" => Dynamic::Pianissimo,
            "piano" => Dynamic::Piano,
            "mezzo-piano" => Dynamic::MezzoPiano,
            "mezzo-forte" => Dynamic::MezzoForte,
            "forte" => Dynamic::Forte,
            "fortissimo" => Dynamic::Fortissimo,
            _ => return None,
        })
    }

    /// Rank 0 (softest) … 5 (loudest), for nearest-dynamic selection.
    pub fn rank(self) -> i32 {
        self as i32
    }

    /// Map a 0–1 intensity onto a dynamic level, so timbre (not just volume)
    /// hardens as a piece builds.
    pub fn for_intensity(intensity: f32) -> Self {
        match (intensity.clamp(0.0, 1.0) * 6.0) as u32 {
            0 => Dynamic::Pianissimo,
            1 => Dynamic::Piano,
            2 => Dynamic::MezzoPiano,
            3 => Dynamic::MezzoForte,
            4 => Dynamic::Forte,
            _ => Dynamic::Fortissimo,
        }
    }
}

/// One parsed sample file: pure metadata plus its asset path.
#[derive(Clone, Debug)]
pub struct SampleSpec {
    /// Leaf-folder name, e.g. `"violin"` or `"snare drum"`.
    pub instrument: String,
    /// Asset-server path (relative to the `assets/` root).
    pub asset_path: String,
    /// `None` for unpitched percussion.
    pub midi: Option<i32>,
    pub duration: String,
    /// `None` for swell recordings (crescendo/decrescendo).
    pub dynamic: Option<Dynamic>,
    pub articulation: String,
}

impl SampleSpec {
    /// Parse `<instrument>_<pitch>_<duration>_<dynamic>_<articulation>` from a
    /// file stem. `instrument` and `asset_path` come from the caller (the leaf
    /// folder and full asset path), since the leading filename field can use a
    /// different separator (e.g. `snare-drum`).
    fn parse(instrument: &str, asset_path: &str, stem: &str) -> Option<Self> {
        let fields: Vec<&str> = stem.split('_').collect();
        if fields.len() < 5 {
            return None;
        }
        let midi = parse_note_to_midi(fields[1]);
        let dynamic = Dynamic::from_token(fields[3]);
        // Articulation never contains '_' (it uses hyphens), but rejoin defensively.
        let articulation = fields[4..].join("_");
        Some(SampleSpec {
            instrument: instrument.to_string(),
            asset_path: asset_path.to_string(),
            midi,
            duration: fields[2].to_string(),
            dynamic,
            articulation,
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Library
// ─────────────────────────────────────────────────────────────────────────────

const PREFERRED_EXTS: &[&str] = &["wav", "ogg", "flac", "mp3"];

/// An index of every sample beneath a scanned folder, grouped by instrument.
#[derive(Default)]
pub struct SampleLibrary {
    /// instrument → its specs.
    by_instrument: BTreeMap<String, Vec<SampleSpec>>,
}

impl SampleLibrary {
    /// Scan `assets_root/sub` recursively, parsing every audio file. Asset paths
    /// are stored relative to `assets_root` (i.e. prefixed with `sub`), ready for
    /// `AssetServer::load`. When the same sample exists as several extensions,
    /// the one earliest in [`PREFERRED_EXTS`] (wav) wins.
    pub fn scan(assets_root: &Path, sub: &str) -> Self {
        let mut lib = SampleLibrary::default();
        // Dedupe by (instrument, stem); keep the most-preferred extension seen.
        let mut chosen: BTreeMap<(String, String), (usize, SampleSpec)> = BTreeMap::new();
        let root = assets_root.join(sub);
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for entry in rd.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let Some(ext) = path.extension().and_then(|e| e.to_str()) else { continue };
                let Some(ext_rank) = PREFERRED_EXTS
                    .iter()
                    .position(|e| e.eq_ignore_ascii_case(ext))
                else {
                    continue;
                };
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
                let instrument = dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                // Asset path relative to assets_root: strip the root prefix.
                let asset_path = path
                    .strip_prefix(assets_root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .into_owned();
                let Some(spec) = SampleSpec::parse(&instrument, &asset_path, stem) else { continue };
                let key = (instrument, stem.to_string());
                match chosen.get(&key) {
                    Some((rank, _)) if *rank <= ext_rank => {}
                    _ => {
                        chosen.insert(key, (ext_rank, spec));
                    }
                }
            }
        }
        for (_, (_, spec)) in chosen {
            lib.by_instrument
                .entry(spec.instrument.clone())
                .or_default()
                .push(spec);
        }
        lib
    }

    /// All instrument names found, sorted.
    pub fn instruments(&self) -> impl Iterator<Item = &str> {
        self.by_instrument.keys().map(|s| s.as_str())
    }

    /// Specs for one instrument (empty slice if unknown).
    pub fn specs(&self, instrument: &str) -> &[SampleSpec] {
        self.by_instrument
            .get(instrument)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Total sample count across all instruments.
    pub fn len(&self) -> usize {
        self.by_instrument.values().map(|v| v.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.by_instrument.is_empty()
    }

    /// Resolve a single representative handle for an (often unpitched) instrument
    /// — handy for feeding percussion samples into a `Drummer`/`SuperDrummer`.
    ///
    /// `articulations` is a fallback-ordered preference list; the first one with
    /// any matching sample wins. Among matches, the dynamic closest to
    /// `dynamic_pref` is chosen.
    pub fn resolve(
        &self,
        asset_server: &AssetServer,
        instrument: &str,
        articulations: &[&str],
        dynamic_pref: Dynamic,
    ) -> Option<Handle<AudioSample>> {
        let specs = self.by_instrument.get(instrument)?;
        for artic in articulations {
            let best = specs
                .iter()
                .filter(|s| s.articulation == *artic)
                .min_by_key(|s| dynamic_distance(s.dynamic, dynamic_pref));
            if let Some(spec) = best {
                return Some(asset_server.load(&spec.asset_path));
            }
        }
        None
    }
}

/// Distance from a spec's (possibly absent) dynamic to a target, used to rank
/// candidates. A missing dynamic (swell) is treated as far away.
fn dynamic_distance(have: Option<Dynamic>, want: Dynamic) -> i32 {
    match have {
        Some(d) => (d.rank() - want.rank()).abs(),
        None => 100,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  MultiSampler
// ─────────────────────────────────────────────────────────────────────────────

/// Which samples a [`Voice`] should be built from. `articulations` is a
/// fallback-ordered list (first match wins per dynamic); `durations` filters by
/// note length (`"025"`, `"1"`, `"long"`, …) — empty means "any".
pub struct VoiceFilter<'a> {
    /// Label the musician refers to this voice by, e.g. `"ostinato"`.
    pub role: &'a str,
    pub articulations: &'a [&'a str],
    pub durations: &'a [&'a str],
    /// Inclusive MIDI range to load, e.g. `(48, 96)`.
    pub midi_range: (i32, i32),
}

/// One loaded, pitch-indexed layer of a [`MultiSampler`]: a single role at a
/// single dynamic. The keymap maps MIDI note → handle for nearest-pitch lookup.
pub struct Voice {
    pub role: String,
    pub dynamic: Dynamic,
    keymap: BTreeMap<i32, Handle<AudioSample>>,
}

/// A curated, loaded multisample for one instrument: several [`Voice`]s the
/// musician selects between by role + dynamic at play time.
pub struct MultiSampler {
    pub instrument: String,
    pub volume_db: f32,
    voices: Vec<Voice>,
}

impl MultiSampler {
    pub fn new(instrument: impl Into<String>, volume_db: f32) -> Self {
        Self {
            instrument: instrument.into(),
            volume_db,
            voices: Vec::new(),
        }
    }

    /// Build and load one voice per requested dynamic that has matching samples.
    ///
    /// For each dynamic and each MIDI note in range we keep one handle (the first
    /// articulation in the fallback list that has a sample at, or nearest to,
    /// that pitch within the duration filter). Returns the number of voices
    /// actually added (0 if the instrument/articulations yielded nothing).
    pub fn add_voice(
        &mut self,
        library: &SampleLibrary,
        asset_server: &AssetServer,
        filter: &VoiceFilter,
        dynamics: &[Dynamic],
    ) -> usize {
        let specs = library.specs(&self.instrument);
        let (lo, hi) = filter.midi_range;
        let mut added = 0;
        for &dynamic in dynamics {
            let mut keymap: BTreeMap<i32, Handle<AudioSample>> = BTreeMap::new();
            // Fill from the most-preferred articulation first; later (less
            // preferred) articulations only fill pitches still missing.
            for artic in filter.articulations {
                for spec in specs.iter().filter(|s| {
                    s.articulation == *artic
                        && s.dynamic == Some(dynamic)
                        && (filter.durations.is_empty()
                            || filter.durations.contains(&s.duration.as_str()))
                }) {
                    let Some(midi) = spec.midi else { continue };
                    if midi < lo || midi > hi {
                        continue;
                    }
                    keymap
                        .entry(midi)
                        .or_insert_with(|| asset_server.load(&spec.asset_path));
                }
            }
            if !keymap.is_empty() {
                self.voices.push(Voice {
                    role: filter.role.to_string(),
                    dynamic,
                    keymap,
                });
                added += 1;
            }
        }
        added
    }

    pub fn is_empty(&self) -> bool {
        self.voices.is_empty()
    }

    /// Total loaded handles across all voices.
    pub fn sample_count(&self) -> usize {
        self.voices.iter().map(|v| v.keymap.len()).sum()
    }

    /// True if any loaded voice carries this role.
    pub fn has_role(&self, role: &str) -> bool {
        self.voices.iter().any(|v| v.role == role)
    }

    /// Pick the best handle for a target pitch, preferring the named role and the
    /// nearest available dynamic. Returns the handle and the residual semitone
    /// offset (`target - chosen_pitch`) for fine pitch-shifting — usually 0.
    ///
    /// Falls back across roles if the requested one wasn't loaded, so callers
    /// always get *something* as long as the sampler isn't empty.
    pub fn pick(
        &self,
        target_midi: i32,
        role: &str,
        dynamic: Dynamic,
    ) -> Option<(Handle<AudioSample>, i32)> {
        let voice = self
            .voices
            .iter()
            .filter(|v| !v.keymap.is_empty())
            .min_by_key(|v| {
                let role_penalty = if v.role == role { 0 } else { 100 };
                role_penalty + (v.dynamic.rank() - dynamic.rank()).abs()
            })?;
        let (&midi, handle) = nearest_in_keymap(&voice.keymap, target_midi)?;
        Some((handle.clone(), target_midi - midi))
    }
}

/// Nearest key (by absolute semitone distance) in a pitch-sorted keymap.
fn nearest_in_keymap(
    keymap: &BTreeMap<i32, Handle<AudioSample>>,
    target: i32,
) -> Option<(&i32, &Handle<AudioSample>)> {
    let below = keymap.range(..=target).next_back();
    let above = keymap.range(target..).next();
    match (below, above) {
        (Some(b), Some(a)) => {
            if (target - b.0).abs() <= (a.0 - target).abs() {
                Some(b)
            } else {
                Some(a)
            }
        }
        (Some(b), None) => Some(b),
        (None, Some(a)) => Some(a),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_naturals_and_sharps() {
        assert_eq!(parse_note_to_midi("C4"), Some(60));
        assert_eq!(parse_note_to_midi("A4"), Some(69));
        assert_eq!(parse_note_to_midi("Cs4"), Some(61));
        assert_eq!(parse_note_to_midi("Ds5"), Some(75));
        assert_eq!(parse_note_to_midi("As0"), Some(22));
    }

    #[test]
    fn rejects_garbage_and_empty() {
        assert_eq!(parse_note_to_midi(""), None);
        assert_eq!(parse_note_to_midi("H4"), None);
        assert_eq!(parse_note_to_midi("C"), None);
    }

    #[test]
    fn dynamic_tokens_round_trip() {
        assert_eq!(Dynamic::from_token("mezzo-forte"), Some(Dynamic::MezzoForte));
        assert_eq!(Dynamic::from_token("crescendo"), None);
        assert!(Dynamic::Pianissimo < Dynamic::Fortissimo);
    }

    #[test]
    fn intensity_maps_across_full_range() {
        assert_eq!(Dynamic::for_intensity(0.0), Dynamic::Pianissimo);
        assert_eq!(Dynamic::for_intensity(1.0), Dynamic::Fortissimo);
        assert_eq!(Dynamic::for_intensity(0.5), Dynamic::MezzoForte);
    }

    #[test]
    fn parses_pitched_spec() {
        let s = SampleSpec::parse(
            "violin",
            "all-samples/violin/violin_Ds5_1_piano_arco-sul-tasto.wav",
            "violin_Ds5_1_piano_arco-sul-tasto",
        )
        .unwrap();
        assert_eq!(s.midi, Some(75));
        assert_eq!(s.duration, "1");
        assert_eq!(s.dynamic, Some(Dynamic::Piano));
        assert_eq!(s.articulation, "arco-sul-tasto");
    }

    #[test]
    fn parses_unpitched_percussion_spec() {
        let s = SampleSpec::parse(
            "snare drum",
            "all-samples/percussion/snare drum/snare-drum__025_fortissimo_with-snares.wav",
            "snare-drum__025_fortissimo_with-snares",
        )
        .unwrap();
        assert_eq!(s.midi, None);
        assert_eq!(s.dynamic, Some(Dynamic::Fortissimo));
        assert_eq!(s.articulation, "with-snares");
    }

    fn keymap(notes: &[i32]) -> BTreeMap<i32, Handle<AudioSample>> {
        notes.iter().map(|&n| (n, Handle::default())).collect()
    }

    #[test]
    fn scans_real_asset_tree_when_present() {
        // Only runs on a checkout that actually has the (large, un-committed)
        // Philharmonia sample set; a no-op everywhere else.
        let root = std::path::Path::new("assets");
        if !root.join("all-samples").is_dir() {
            return;
        }
        let lib = SampleLibrary::scan(root, "all-samples");
        assert!(lib.len() > 1000, "expected many samples, got {}", lib.len());

        let violins = lib.specs("violin");
        assert!(!violins.is_empty(), "no violin samples parsed");
        // Pitched strings must parse a MIDI note and a real asset path.
        assert!(violins.iter().any(|s| s.midi.is_some()));
        assert!(violins.iter().all(|s| s.asset_path.starts_with("all-samples/violin/")));
        // The wav/mp3 dedupe must collapse pairs: no two specs share an asset
        // path, and every kept path is a .wav (preferred extension).
        assert!(violins.iter().all(|s| s.asset_path.ends_with(".wav")));

        // Percussion lives in subfolders and is unpitched.
        let snare = lib.specs("snare drum");
        assert!(!snare.is_empty(), "no snare drum samples parsed");
        assert!(snare.iter().all(|s| s.midi.is_none()));

        // Guard the exact selections real_orchestra makes, so a voice never
        // silently comes back empty. Each (instrument, articulation, predicate)
        // must match at least one spec.
        let has = |instr: &str, pred: &dyn Fn(&SampleSpec) -> bool| lib.specs(instr).iter().any(pred);
        assert!(has("violin", &|s| s.articulation == "pizz-normal"));
        assert!(has("violin", &|s| s.articulation == "arco-normal"
            && matches!(s.duration.as_str(), "025" | "05")));
        assert!(has("violin", &|s| s.articulation == "arco-tremolo"));
        assert!(has("cello", &|s| s.articulation == "arco-normal"
            && matches!(s.duration.as_str(), "1" | "15" | "long")));
        assert!(has("trombone", &|s| s.articulation == "normal"));
        // Woodwind choir: flute runs/sustains, plus clarinet, bassoon, oboe pads.
        assert!(has("flute", &|s| s.articulation == "normal"
            && matches!(s.duration.as_str(), "025" | "05")));
        assert!(has("flute", &|s| s.articulation == "normal"
            && matches!(s.duration.as_str(), "1" | "15" | "long")));
        assert!(has("clarinet", &|s| s.articulation == "normal"));
        assert!(has("bassoon", &|s| s.articulation == "normal"));
        assert!(has("oboe", &|s| s.articulation == "normal"));
        // The percussion handles real_orchestra resolves.
        assert!(has("bass drum", &|s| s.articulation == "struck-singly"));
        assert!(has("clash cymbals", &|s| s.articulation == "struck-together"));
        assert!(has("suspended cymbal", &|s| s.articulation == "roll"));
    }

    #[test]
    fn nearest_prefers_closest_and_ties_low() {
        let km = keymap(&[60, 64, 67]);
        assert_eq!(*nearest_in_keymap(&km, 61).unwrap().0, 60);
        assert_eq!(*nearest_in_keymap(&km, 66).unwrap().0, 67);
        // Exact tie (62 is equidistant 60/64) resolves to the lower note.
        assert_eq!(*nearest_in_keymap(&km, 62).unwrap().0, 60);
        // Out of range clamps to the nearest end.
        assert_eq!(*nearest_in_keymap(&km, 40).unwrap().0, 60);
        assert_eq!(*nearest_in_keymap(&km, 90).unwrap().0, 67);
    }
}
