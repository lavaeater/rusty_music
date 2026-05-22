# Roadmap: Completing the Implementation vs. Chapter 14

This document compares the current `rusty_music` implementation against the design described in Chapter 14 ("Improvisational Music") of *Game Audio Programming 3*, identifies bugs and missing features, and proposes a plan to address them.

---

## 1. Critical Bugs

### 1.1 Clock accumulator drift
**File:** `src/clock/mod.rs:33`

```rust
self.accumulator = 0.0;  // BUG
// should be:
self.accumulator -= self.beat_length;
```

Resetting to zero discards any overshoot from the previous frame, causing the beat to drift earlier over time. The book's clock design assumes precise scheduling; this undermines it.

### 1.2 Arpeggiator step-size logic is inverted and broken
**File:** `src/musicians/arpeggiator.rs:34-46`

```rust
let step_size = if base_intensity < 0.4 { 4 } else if base_intensity > 0.7 { 8 } else { 16 };
```

The middle band (0.4–0.7) gets `step_size = 16`, making it the *least* active range. Higher intensity should produce *more* notes. The book defines step_size as subdivisions per bar (4 = quarter notes, 16 = 16th notes), so higher stepSize = more frequent. The current code uses step_size as a *wait* between ticks, so the relationship is inverted for the middle band.

Fix: reorder to `{ 4 }` → `{ 8 }` → `{ 16 }` in ascending intensity order, and reconsider whether the logic should flip the rate relationship.

### 1.3 Drummer strength polarity is inconsistent with tonal musicians
**Files:** `src/musicians/drummer.rs:45-47`, `src/musicians/bassist.rs`, `src/musicians/soloist.rs`

The drummer filters with `v.strength <= base_intensity` — a note plays if its strength is *at or below* the current intensity. This means a strength=0.75 note only plays when intensity ≥ 0.75. At intensity=0.5, the downbeat kick (strength=0.75) does **not** play while weaker-tagged hits do.

The book defines strength the opposite way: `minStrength = 1.0 - intensity`, so high-strength notes are essential and always play, while low-strength notes are embellishments that appear at high intensity.

The tonal musicians (Bassist, Soloist) use `strength >= 1.0` filters that match the book's semantics, making the codebase internally inconsistent.

**Options:**
- Adopt the book's convention throughout: `strength >= (1.0 - intensity)` is the play condition, and update all drum beat data accordingly (kick downbeat → strength=1.0).
- Keep the current convention but document it and make the drum data match (kick downbeat → strength=0.0 so it always plays). Currently the data and the filter are mismatched.

### 1.4 Soloist can panic on out-of-bounds access
**File:** `src/musicians/soloist.rs:37`

```rust
self.recorded_melody[recording_index as usize]  // panics if vec not fully populated
```

When `repeat_bar` gets set and the repeat playback branch is entered, `recording_index` can exceed the current length of `recorded_melody`. The book pre-allocates with `InvalidNoteNumber` so the array is always fully indexed. The current code grows the vec dynamically with `push` and only clears after it overflows, leaving a window where a repeat starts before the recording is complete.

Fix: pre-fill `recorded_melody` with `None` to capacity in `new()`, and use indexed assignment instead of `push`.

### 1.5 `macros/src/lib.rs` contains code from a different project
**File:** `macros/src/lib.rs`

The file contains a `YoetzSuggestion` derive macro that belongs to the `bevy_yoetz` crate. It is unrelated to `rusty_music`. The proc-macro crate compiles but exports nothing useful for this project. Either implement the actual macros needed (e.g., deriving `MusicPlayer`) or clear the file.

---

## 2. Architectural Gaps

### 2.1 No scheduled note timing
**Book §14.2.1–14.2.2**

The book's sampler uses `clock.BarsToEngineTime(noteTime)` to schedule playback at a mathematically precise point in time. The current implementation fires notes immediately when a `Beat` event arrives — any frame-time jitter becomes audible timing jitter.

`bevy_kira_audio` supports scheduled playback. The `Clock` should expose a `bars_to_engine_time(bars: f64) -> f64` method, and all `play()` calls should schedule at the computed time rather than playing immediately.

### 2.2 Conductor is passive; chord length is hardcoded to 1 bar
**Book §14.3.1**, `src/musicians/conductor.rs`

The book's `Conductor` actively drives musicians each update, cycles chords based on `chordLengthBars` (a float), and uses a position-based lookup:

```
chordTimeBars = mod(timeBars, chordLengthBars)
for each chord: if chord.posBars > chordTimeBars: break
currentChord = chord
```

The current system hardcodes 1-bar chords via `beat.bar_count % chords.len()`. The `Chord.bar` field exists but is never used for position lookup. This means:
- Chords can't span multiple bars (e.g., a 2-bar vamp).
- Chords can't change mid-bar.
- The chord progression always loops at `chords.len()` bars, not at a configurable `chordLengthBars`.

Fix: add `chord_length_bars: f32` to `Conductor`, change `Chord.bar` to `pos_bars: f32`, and implement position-based lookup in `play_sound_on_the_beat` (or in the Conductor itself if it becomes an active system).

### 2.3 No shared `TonalMusician` base
**Book §14.3.4**

The book proposes a `TonalMusician` base with helpers:

```
GetNote(notes[], minStrength) → Note
GetChordNote(minStrength) → Note
GetScaleNote(minStrength) → Note
PlayNote(noteNumber, timeBars)
```

The current Bassist and Soloist both inline equivalent filter-and-random-select logic. Extract a `TonalPlayer` trait or helper struct to share this code and eliminate the duplication.

### 2.4 Musician interface does not separate chord updates from note updates
**Book §14.3.2**

The book splits musician logic into `SetChord(chord)` (called when the chord changes) and `UpdateNotes(timeBars)` (called every tick). The current `MusicPlayer::play` combines everything into one call. This is a simplification that works, but it means musicians cannot cache or pre-compute anything when the chord changes — every beat re-derives from scratch.

This is low priority but worth restructuring if musicians become more complex.

---

## 3. Incorrect Musician Logic

### 3.1 Bassist note selection doesn't follow the book
**Book §14.3.4**, `src/musicians/bassist.rs`

The book's bass logic:
- 16th 0: always play root (`GetChordNote(1.0)`)
- 16th % 4 == 0 (quarter beats): if `random < intensity`, play strong tone (`GetChordNote(0.5)`)
- 16th % 2 == 0 (8th beats): if `random < intensity - 0.25`, play medium tone (`GetChordNote(0.25)`)
- Otherwise: if `random < intensity - 0.5`, play any chord tone

The current bassist:
- Only filters with `strength >= 1.0` in all branches — no gradation by note strength tier.
- Uses `sixteenth == 3` (the 4th sub-beat) as an embellishment trigger — the book uses 8th-note positions (every 2 16ths).
- Has a final `else if intensity > 0.5` branch with no rhythmic constraint, so it fires on any beat.

### 3.2 Soloist note-selection grid doesn't match the book
**Book §14.3.6**, `src/musicians/soloist.rs:43-52`

Book's soloist selection per 16th position within the bar (0–15):
- Position 0: always play (`GetScaleNote(1.0)`)
- Position % 4 == 0 (quarters): `if random < intensity`
- Position % 2 == 0 (8ths): `if random < intensity - 0.25`
- Any other position: `if random < intensity - 0.5`

Current soloist:
- `beat.beat == 0 && beat.sixteenth == 0` — only the very first position of the bar, not every downbeat of each bar's phrase.
- `beat.sixteenth == 3` — the 4th sub-beat, not quarter positions.
- `beat.sixteenth == 2 || beat.sixteenth == 0` — 1st and 3rd sub-beats of any beat.

The current mapping treats `(beat, sixteenth)` as a 2-level grid where `sixteenth` is 0–3 within a beat, giving 16 positions per bar. The book addresses them as a flat 0–15 index. The conditions don't align.

---

## 4. Missing Features from Chapter 14

### 4.1 Drummer fills and half/double time  
**Book §14.3.3 Homework**
- Add fills triggered every N bars.
- Switch between half-time (kick/snare at half rate), normal, and double-time feels based on intensity.

### 4.2 Scale-tone embellishments for bassist  
**Book §14.3.4 Homework**
- Use scale notes (not just chord tones) for passing tones and embellishments.
- Add melodic "memory" so the bass line has a sense of repetition.

### 4.3 Additional arpeggio modes  
**Book §14.3.5 Homework**
- Up/down (ping-pong) mode.
- Optional scale runs between chord arpeggios.
- Mode tied to intensity (e.g., random at high intensity).

### 4.4 Sustained notes for soloist  
**Book §14.3.6 Homework**
- The current sampler plays one-shot samples. Support for note-on/note-off to allow held notes would require the `AudioControl` handle to be stored and stopped on the next note.

### 4.5 AABA song form for soloist  
**Book §14.3.6 Homework**
- Track two recorded melodies and play them in AABA order (A, A, B, A) before regenerating.

---

## 5. Suggested Implementation Order

1. ✅ **Fix clock accumulator** (§1.1)
2. ✅ **Fix drummer data / strength polarity** (§1.3)
3. ✅ **Fix arpeggiator step-size** (§1.2)
4. ✅ **Fix soloist pre-allocation and panic** (§1.4)
5. ✅ **Implement `TonalMusician` helpers** (§2.3)
6. ✅ **Fix bassist note selection** (§3.1)
7. ✅ **Fix soloist note-selection grid** (§3.2)
8. ✅ **Add `chord_length_bars` to Conductor** (§2.2)
9. ✅ **Add scheduled note timing** (§2.1) — `Beat.overshoot` captures frame jitter. Per-note `play_at` scheduling was attempted but floods Firewheel's message channel; long-term drift is prevented by the accumulator fix instead.
10. ✅ **Drummer fills and time-feel switching** (§4.1) — `SuperDrummer::with_fills(n, fill_drums)` plays an alternate pattern in the last bar of every N bars; `auto_time_feel` with `half_time_drums`/`double_time_drums` swaps full kit by intensity; half-time gating at `intensity < 0.3` also thins the groove.
11. ✅ **Bassist scale embellishments + memory** (§4.2) — 16th positions use scale tones as passing notes; `last_midi_diff` tracking biases toward notes within 5 semitones for smoother lines; `memory_bars`/`memory_repeats` record and replay the bass line.
12. ✅ **Additional arpeggio modes** (§4.3) — `PingPong` (0→1→2→3→2→1→...); `Auto` mode selects Up/PingPong/Random by intensity (default); `use_scale_runs` weaves in scale notes at high intensity.
13. ✅ **AABA soloist form** (§4.5) — `AabaPhase` state machine (`RecordA → PlayA1 → PlayA2 → RecordB → PlayA3 → RecordA`); A melody replayed in positions 1, 2, 4; B freshly generated; `Soloist::new(sampler, record_bars)`.
14. ✅ **Clean up or implement `macros/`** (§1.5).
