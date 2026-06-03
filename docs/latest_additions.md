# Latest additions — multisampled real orchestra

This documents the work adding a real, multisampled orchestral example driven by
the Philharmonia / Sonatina sample set in `assets/all-samples`, plus the reusable
crate feature that powers it.

## Summary

- **New crate module `src/sampler.rs`** — a reusable multisampling layer: scans a
  sample folder, parses the filename grammar, and serves nearest-pitch / dynamic /
  articulation sample selection.
- **New example `examples/real_orchestra.rs`** — an `epic_orchestra`-style,
  intensity-driven orchestral build, but voiced with *real recorded* strings,
  brass, woodwinds, and percussion instead of pitch-shifted synth samples.
- **Cargo.toml** — registered the `real_orchestra` example.

## The sample library

`assets/all-samples` follows the Philharmonia / Sonatina filename grammar:

```
<instrument>_<pitch>_<duration>_<dynamic>_<articulation>.<ext>
violin_Ds5_1_piano_arco-sul-tasto.wav        (pitched)
snare-drum__025_fortissimo_with-snares.wav   (unpitched — empty pitch field)
```

- **pitch** — scientific note name, `s` = sharp, no flats, octaves 0–8 (C4 = 60).
  Empty for percussion.
- **duration** — `025 / 05 / 1 / 15` (seconds) or `phrase / long / very-long`.
- **dynamic** — `pianissimo → piano → mezzo-piano → mezzo-forte → forte →
  fortissimo`, plus swell recordings (`crescendo` / `decrescendo` / `cresc-decresc`).
- **articulation** — instrument-specific (`arco-normal`, `pizz-normal`,
  `arco-tremolo`, `normal`, percussion `struck-singly` / `roll` / `rhythm`, …).

58 leaf instruments, ~13.6k unique samples (each present as both `.wav` and `.mp3`).
One instrument per leaf folder; `percussion/` contains a subfolder per instrument.

## Design: how the three "variant" axes are used

Each variant axis maps onto a musical decision:

1. **pitch → nearest-neighbour selection (real multisampling).** Instead of
   stretching one recording across a range, a musician picks the closest *recorded*
   note to its target pitch and shifts only the residual semitone or two (usually 0).
   This is the biggest quality jump over `epic_orchestra`.
2. **dynamic → intensity, as a sample-layer swap.** As intensity rises we swap
   `piano → mezzo-forte → fortissimo` *samples*, so the timbre hardens — not just
   the volume.
3. **articulation → a playing mode a section switches between as it builds.** This
   is what the per-instrument variants buy you. Where an instrument lacks an
   articulation, a fallback chain degrades gracefully.

Percussion subfolders are treated as a **section of distinct unpitched
instruments**, with articulation encoding *function* (`struck-singly` = hit,
`roll` = swell, `rhythm` / `phrase` = pre-made fill).

## `src/sampler.rs` API

- `parse_note_to_midi(&str) -> Option<i32>` — scientific note name → MIDI (C4 = 60).
- `Dynamic` — ordered enum (`Pianissimo`…`Fortissimo`) with `from_token`, `rank`,
  and `for_intensity(f32)` (maps 0–1 intensity onto a dynamic level).
- `SampleSpec { instrument, asset_path, midi: Option<i32>, duration, dynamic, articulation }`
  — one parsed file.
- `SampleLibrary::scan(assets_root, sub)` — walks `assets_root/sub` recursively
  (metadata only, cheap even at 27k files), dedupes `.wav`/`.mp3` pairs preferring
  **wav**, indexes by instrument (leaf-folder name, e.g. `percussion/snare drum`
  → `"snare drum"`). Asset paths are stored relative to `assets_root`, ready for
  `AssetServer::load`.
  - `instruments()`, `specs(instrument)`, `len()`, `is_empty()`.
  - `resolve(asset_server, instrument, articulations_fallback, dynamic_pref)
    -> Option<Handle<AudioSample>>` — one representative handle (used to feed
    percussion into the existing `Drummer` / `SuperDrummer`).
- `VoiceFilter { role, articulations, durations, midi_range }` — describes which
  samples a voice is built from (articulations are fallback-ordered; empty
  `durations` = any).
- `MultiSampler` — a curated, loaded subset for one instrument:
  - `new(instrument, volume_db)`, `add_voice(library, asset_server, filter,
    dynamics) -> usize`, `is_empty()`, `sample_count()`, `has_role(role)`.
  - `pick(target_midi, role, dynamic) -> Option<(Handle<AudioSample>, i32)>` —
    nearest pitch for the preferred role + nearest dynamic, returning the handle
    plus the residual semitone offset for fine pitch-shifting. Falls back across
    roles/dynamics so callers always get something while the sampler is non-empty.

Only the curated subset of handles is ever loaded (a few hundred), never the full
13k files.

## `examples/real_orchestra.rs`

An intensity-layered TUI (ratatui), mirroring `epic_orchestra`. Controls: `↑`/`↓`
intensity, `Space` auto-build (64-bar rise/fall), `q`/`Esc` quit. Chord
progression: Dm (i) → C (VII) → Bb (VI) → C (VII), 4 bars, BPM 88.

### Sections

- **Violins** (`StringSection`, `root_midi: 74`) — articulation switches with
  intensity: `pizz` (quarters) → spiccato `ostinato` (short `arco-normal`, 8ths) →
  legato `sustain` → `tremolo` bed + 16th drive at the climax.
- **Cellos** (`StringSection`, `is_bass`, `root_midi: 50`) — low sustained
  root/fifth, switching to `tremolo` at the climax.
- **Brass** (`BrassSection`, trombone, `root_midi: 50`) — `normal` chord stabs,
  stacking 1–3 simultaneous chord tones on strong beats as intensity grows.
- **Woodwind choir** (`WoodwindSection`, three roles) — almost all `normal`
  articulation, so the variant axes used are *duration* (short `run` vs. long
  `sustain`) and *dynamic* layers:
  - **Clarinet** — enters 0.30, `Harmony` (mid sustained inner chord tone pad).
  - **Bassoon** — enters 0.40, `BassDouble` (low sustained root, doubling cellos).
  - **Flute** — enters 0.45, `RunTop` (offbeat ascending scale runs + sustained
    top line at the climax).
  - **Oboe** — enters 0.72, `Harmony` an octave above the clarinet (reedy upper
    climax colour).
- **Percussion** — bass drum (`struck-singly`) + snare (`with-snares`, `roll`
  fills) resolved from the library and fed into the existing `SuperDrummer`.
- **Cymbals** (`CymbalLayer`) — clash-cymbal accents (`struck-together`) and
  suspended-cymbal `roll` swells before chord changes at the climax.

### Intensity arc

```
0.00–0.13  Silence
0.13–0.30  Cellos sustain + violin pizzicato
0.30–0.45  Clarinet harmony pad joins
0.45–0.58  Violin spiccato ostinato + flute runs
0.58–0.78  Brass stabs + bassoon + string sustains
0.78–1.00  Climax: string tremolo, oboe, 16ths, full percussion, ff samples
```

## Tests

`src/sampler.rs` has 8 unit tests, including `scans_real_asset_tree_when_present`,
which scans the actual `assets/all-samples` tree (no-op on checkouts without the
samples) and asserts:

- many samples parse, dedupe keeps only `.wav`, violin pitches parse, percussion is
  unpitched;
- every exact `(instrument, articulation, duration, dynamic)` combination the
  example requests actually exists — so no voice silently loads empty (violin
  pizz/short-arco/tremolo, cello long arco, trombone normal, flute short+long
  normal, clarinet/bassoon/oboe normal, and the percussion handles).

## How to run

```bash
cargo run --example real_orchestra
```

Raise intensity with `↑` (or `Space` for the auto-build) and the sections layer in.
Note: the example needs a real terminal (raw mode) and an audio device, so it can't
run headless.

## Follow-ups / extension ideas

- Trade woodwind melodies around (flute/oboe alternating phrases instead of
  doubling).
- Per-section mute keys to solo a section while testing.
- More articulations per section, or additional instruments (`viola`, `double
  bass`, `cor anglais`, `bass clarinet`, more percussion).

## Note

`CLAUDE.md` states the crate uses `bevy_kira_audio`, but the code actually uses
`bevy_seedling` — worth updating.
