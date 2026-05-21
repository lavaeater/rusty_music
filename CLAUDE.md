# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Build
cargo build

# Run examples
cargo run --example simple
cargo run --example custom_instrument

# Check/lint
cargo check
cargo clippy

# Test
cargo test
```

## Architecture

`rusty_music` is a Bevy plugin (`MusicPlugin`) for generative/improvisational game music. It uses `bevy_kira_audio` for audio playback (feature-gated as `kira`, enabled by default).

**Core data flow:**
1. `Clock` (resource) accumulates delta time and fires `Beat` events at regular intervals based on BPM/time signature.
2. `play_sound_on_the_beat` system reads `Beat` events and calls `MusicPlayer::play` on every `Musician` entity.
3. Each `Musician` holds a boxed `dyn MusicPlayer` that decides what to play based on the current beat, chord, and intensity.

**Key abstractions:**
- `MusicPlayer` trait — implement this to create custom instruments. Receives `Beat`, `Audio`, `base_intensity` (0.0–1.0), and the current `Chord`.
- `Musician` component — wraps any `MusicPlayer` and is spawned as a Bevy entity.
- `Conductor` resource — holds the chord progression as `Vec<Chord>`. Bars cycle through it.
- `Intensity` resource (`f32` 0–1) — game code writes to this to affect musical density/activity.
- `Note` — a `midi_note_diff` (semitone offset from root) plus a `strength` (0–1). Instruments filter notes by comparing `strength` against the current intensity.

**Built-in musicians (`src/musicians/`):**
- `Drummer` / `SuperDrummer` — plays samples at specific `(beat, sixteenth)` grid positions; `SuperDrummer` wraps multiple `Drummer`s.
- `Bassist` — plays chord root notes.
- `Soloist` — records and replays randomly-selected melodic phrases, influenced by intensity.
- `Arpeggiator` — arpeggiated chord tones.

**Workspace:** The `macros/` crate (`bevy-rusty-music-macros`) is a proc-macro crate included in the workspace but not yet meaningfully used by the main crate.

**Timing:** The clock subdivides bars into beats and sixteenths. `(beat, sixteenth)` pairs index drum patterns. The beat grid is driven by `beats` (beats per bar) and `note_type` parameters — these names are somewhat misleading; in practice `beats=4, note_type=4` gives standard 4/4 time.
