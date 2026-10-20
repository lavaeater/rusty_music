# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Build
cargo build

# Run examples
cargo run --example simple
cargo run --example custom_instrument
cargo run --example synth_band --features synth      # -- --samples for sampled drums
cargo run --example synth_presets --features synth   # -- <name> to hear one preset

# Check/lint
cargo check
cargo clippy

# Test
cargo test
```

## Architecture

`rusty_music` is a Bevy plugin (`MusicPlugin`) for generative/improvisational game music. Audio goes through `bevy_seedling` (Firewheel); the optional `synth` feature adds glicol-based synthesizers (`src/synth/`). See `synth-roadmap.md` for where this is heading.

**Core data flow:**
1. `Clock` (resource) runs on the *audio* clock (`Time<Audio>`). `progress_clock_system` emits each step as a `Beat` message `lookahead` (0.1 s) before it sounds, stamped with its exact audio time (`Beat::time`, `Beat::step_secs`).
2. `play_sound_on_the_beat` reads `Beat`s, asks every `Musician`'s `MusicPlayer` for `NoteEvent`s, and hands them to that musician's `Instrument` with an exact start time (`beat.time + offset * step_secs`).
3. Instruments schedule sound sample-accurately: samples via `PlaybackSettings::play_at`, synths via timestamped Firewheel events.

**Key abstractions:**
- `MusicPlayer` trait — decides *what* to play: `play(beat, &mut Vec<NoteEvent>, intensity, &Chord)`. Optional `take_instrument()` lets a player bring a default instrument (built-in musicians hand over the `Sampler` passed to their constructor).
- `NoteEvent` (`src/instrument.rs`) — absolute MIDI `pitch`, `velocity`, `length`/`offset` in steps, `gain_db`, `articulation`, `channel`. `NoteEvent::from_diff(d)` = `SAMPLE_ROOT` (60) + d, so a `Sampler` plays offset-0 notes unshifted.
- `Instrument` trait — decides *how it sounds*. Impls: `Sampler` (one sample, pitch-shifted), `MultiSampler` (velocity → dynamic layer, articulation → role), `Channels` (route by `note.channel`: drum kits, doublings), `Silent`, and `synth::SynthInstrument` (plays the `SynthNode` on the same entity).
- `Musician` component — `player` + `instrument`. `Musician::new(name, player)` uses the player's own instrument; `.with_instrument(..)` overrides it. `synth::synth_musician(..)` bundles a musician with a `SynthNode`/`SynthConfig`.
- `Conductor` resource — holds the chord progression as `Vec<Chord>`. Bars cycle through it.
- `Intensity` resource (`f32` 0–1) — game code writes to this to affect musical density/activity.
- `Note` — a `midi_note_diff` (semitone offset from root) plus a `strength` (0–1). Instruments filter notes by comparing `strength` against the current intensity.

**Built-in musicians (`src/musicians/`):**
- `Drummer` / `SuperDrummer` — plays at `(beat, sixteenth)` grid positions; `SuperDrummer` wraps multiple `Drummer`s and builds a `Channels` kit from their samplers.
- `Bassist` — plays chord root notes.
- `Soloist` — records and replays randomly-selected melodic phrases, influenced by intensity.
- `Arpeggiator` — arpeggiated chord tones.
- `Bassist::default()`, `Arpeggiator::default()`, `Soloist::with_bars(n)` build them without a sampler, for use with another instrument (e.g. a synth).

**Synths (`--features synth`):** `src/synth/engine.rs` is a Firewheel-free, unit-tested voice pool of `glicol_synth` graphs (constant 128-frame latency, allocation-free render path; poly with stealing, or mono/legato with a note stack and glide). `src/synth/dsp.rs` holds our own glicol `Node`s (PolyBLEP `Osc`, click-free `Env`, smoothed `Svf`, `PitchSweep`, `Sum`), used instead of glicol's `adsr`/`rlpf`/oscillators. `src/synth/patch.rs` defines `SynthPatch` (plain data, presets as functions like `SynthPatch::acid_bass()`). `src/synth/mod.rs` wraps the engine as the `SynthNode` Firewheel node (live `volume`/`cutoff`/`resonance`, smoothed on the audio thread), plus `synth(patch, db)`, `synth_musician`, `SynthInstrument::on(entity)` and `spawn_synth_kit` for drum kits. Examples needing it declare `required-features = ["synth"]`.

**Workspace:** The `macros/` crate (`bevy-rusty-music-macros`) is a proc-macro crate included in the workspace but not yet meaningfully used by the main crate.

**Timing:** The clock subdivides bars into beats and sixteenths. `(beat, sixteenth)` pairs index drum patterns. The beat grid is driven by `beats` (beats per bar) and `note_type` parameters — these names are somewhat misleading; in practice `beats=4, note_type=4` gives standard 4/4 time. Change tempo by changing `Clock::beat_length` (seconds per step). Because beats are emitted ahead of time, UI reading `Clock` runs ~`lookahead` ahead of what is heard.

**Running examples outside `cargo run`:** Bevy resolves `assets/` relative to `CARGO_MANIFEST_DIR` (or the executable); set `CARGO_MANIFEST_DIR` to the project root when launching a built binary directly, or samples silently fail to load.
