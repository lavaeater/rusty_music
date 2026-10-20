# Synth roadmap — rusty_music × glicol

Goal: musicians in `rusty_music` can play **synthesised instruments** (anything
that produces a waveform, defined the way glicol defines patches) alongside the
existing **sampled instruments**, all driven by the game's situation.

The north star remains: *situationally aware, generated music for games.* Synths
matter because every parameter of a synth is a knob the game can turn: filter
cutoff can follow danger, detune can follow tension, and reverb size can follow
the room. A sample can only change pitch and volume.

---

## 1. What we have today

### rusty_music (Bevy 0.19, bevy_seedling 0.8 / Firewheel)
- `Clock` runs on `Time` in `Update` and fires `Beat` messages, so a beat
  arrives **up to one frame late**. `Beat::overshoot` is computed but unused.
- Each `MusicPlayer` decides what to play **and how to make the sound itself**.
  It spawns `SamplePlayer` entities directly with a pitch-shift speed.
- Notes have no duration. A note is a one-shot sample, and "sustain" means
  "pick a longer recording".
- `improv_orchestra` shows the generative side (seeded lead sheet, motif
  development, voice leading, generated grooves), but all of its sound is samples.

### bevy_glicol (Bevy 0.18)
- `bevy_glicol` is a 140-line plugin. It opens **its own cpal 0.15 stream** on a
  thread and runs `glicol::Engine` (the parser plus live-coding diff engine)
  behind a `parking_lot::Mutex`. The only API is `update_with_code(&str)`.
- `bevy_glicol_ui` loads, parses and visualises `.glicol` graphs as Bevy
  assets (phases 1–2 of its roadmap are done).
- `glicol/rs/synth` (`glicol_synth`) is a **plain Rust DSP graph library**.
  It has `AudioContext<N>` over petgraph, block size `N` as a const generic,
  oscillators, filters, envelopes, plate reverb, delays, a sampler, and
  `send_msg(node, Message)`. It has **no cpal dependency**, and
  `cargo check -p glicol_synth` passes on its own.

### How glicol plays notes (important for the design)
In glicol, notes are **signals**, not events:
- `seq` outputs a one-sample **impulse** whose value is the pitch ratio
  `2^((midi-60)/12)`, and outputs 0 everywhere else.
- `envperc` triggers when its input is > 0.
- `adsr` treats its input as a **gate**: attack/decay/sustain while > 0,
  release when it drops back to 0.
- Pitch comes from `~trigger >> mul 261.63` (the ratio times the frequency of C4).

So a glicol patch becomes "an instrument" once something feeds it trigger, gate
and pitch signals. That is the bridge between the two projects.

---

## 2. Hard constraints found while investigating

1. **We can't depend on `bevy_glicol` as it is.** Its cpal 0.15 links
   `alsa-sys 0.3`, while bevy_seedling's cpal 0.18 links `alsa-sys 0.4`. This is
   the exact conflict we just removed with `tunes`. It is also on Bevy 0.18.
2. **There must be only one audio engine.** Two cpal streams means two clocks,
   no shared mixing or effects, and no sample-accurate sync between samples
   and synths. Synths have to run **inside Firewheel**.
3. **`glicol::Engine` is not realtime safe.** Parsing, diffing and graph edits
   allocate, and it holds samples as `&'static [f32]`. It may build graphs, but
   must never run on the audio thread while editing.
4. **Block sizes differ.** glicol processes fixed blocks of `N` (128); Firewheel
   hands nodes a variable number of frames. We need a small ring-buffer adapter
   (at most `N` frames of extra latency, about 3 ms at 44.1 kHz).
5. Frame-based timing (the current `Clock`) is fine for samples with soft attacks,
   but noticeable on synth plucks and drums. Synths raise the bar for timing.

---

## 3. Target architecture

```
 Game state ──► MusicState (intensity, tension, mood, location, …)
                    │
                    ▼
 Conductor / Composer  (lead sheet, form, key, tempo)          [ECS, Update]
                    │
                    ▼
 Musicians (MusicPlayer)  ──►  NoteEvent { pitch, velocity,      [ECS, Update]
                                 start: MusicalTime, length,
                                 articulation }
                    │
                    ▼
 Scheduler (look-ahead, converts musical time → Time<Audio>)     [ECS, Update]
                    │
          ┌─────────┴──────────┐
          ▼                    ▼
 SampleInstrument        SynthInstrument                          [ECS → audio]
 (SamplePlayer +         (Firewheel node: voice pool of
  MultiSampler, as        compiled glicol_synth patches,
  today)                  timestamped NoteOn/NoteOff events)
          └─────────┬──────────┘
                    ▼
        Firewheel buses / effects (reverb, ducking, …) ──► cpal
```

Key decisions:

- **Musicians stop making sounds.** They emit `NoteEvent`s; an `Instrument`
  turns them into audio. This one refactor makes samples and synths
  interchangeable, adds note durations, and enables proper scheduling.
- **Synths are Firewheel nodes.** Each `SynthInstrument` is one custom
  `AudioNode`, registered with `app.register_node::<SynthNode>()`, that owns a
  **voice pool**. Each voice is a compiled `glicol_synth::AudioContext`.
  NoteOn/NoteOff arrive as timestamped `NodeEventType::Custom` events, drained
  with `drain_with_timestamps` for sample-accurate starts.
- **Glicol text is the instrument definition format.** A voice patch is a
  `.glicol` file with a reserved interface the voice drives:
  - `~freq` is the held frequency in Hz
  - `~gate` is 1.0 while the note is held (for `adsr`)
  - `~trig` is the glicol-style impulse carrying the pitch ratio (for `envperc`, `seq`-style patches)
  - `~vel` is velocity, 0–1
  - `#name` placeholders are **exposed parameters** the game and musicians can set
- **Graphs are compiled off the audio thread** (parser plus `glicol_synth`
  graph construction in a Bevy task), then sent to the node as a boxed custom
  event. Hot reload and editing in `bevy_glicol_ui` use the same path.
- **Samples stay on bevy_seedling's sampler.** Glicol's `sp`/`\sample` path
  needs `&'static` data. Sample support inside glicol patches is optional and
  later; if we add it, we leak each sample once at asset load time.

### Code placement
Start **inside rusty_music** as `src/synth/` behind a `synth` cargo feature
(`glicol_synth` + `glicol_parser` as git dependencies on the lavaeater fork).
That gives one Bevy version and fast iteration. Once the node API settles,
extract the "glicol inside Firewheel" part into `bevy_glicol`, replacing its cpal
backend, so `bevy_glicol_ui` can edit the same instruments live.

---

## 4. Phases

Each phase ends with something audible and a runnable example.

### Phase 0 — Spike: one glicol voice inside Firewheel  *(de-risk everything)*
| # | Task | Done when |
|---|------|-----------|
| 0.1 | Add `glicol_synth` (git, lavaeater fork) under a `synth` feature; check that petgraph, hashbrown and edition don't clash | `cargo check --features synth` passes |
| 0.2 | `SynthNode` Firewheel node running a **hardcoded** `glicol_synth` graph: saw → lpf → adsr | A tone plays through `SeedlingPlugins` |
| 0.3 | Block adapter: glicol `N=128` ↔ Firewheel's variable frames | No clicks at different buffer sizes |
| 0.4 | NoteOn/NoteOff as timestamped custom events driving `~gate` / `~freq` | `examples/synth_spike.rs` plays a scale on the beat |
| 0.5 | Audit `AudioContext::next_block` and the nodes we use for allocations; count voices per core | Notes on realtime safety and CPU per voice |

**Exit criteria:** it sounds right, doesn't allocate on the audio thread, and
handles a reasonable number of voices (target ≥ 32 simple voices). If glicol
fails here, fall back to native DSP (Firewheel already ships `svf`,
`noise_generator`, `beep_test`, `freeverb`). The rest of the roadmap doesn't
depend on which DSP backend is used.

#### Phase 0 results ✅ — glicol passes, we keep it

Code: `src/synth/engine.rs` (Firewheel-free engine, unit tested),
`src/synth/mod.rs` (`SynthNode` / `SynthConfig` / `SynthPlugin` / `send_note`),
`examples/synth_spike.rs`. Everything is behind the `synth` feature.

| Check | Result |
|-------|--------|
| Dependencies | `glicol_synth` (lavaeater fork, `main`) compiles alongside bevy_seedling. Adds petgraph 0.6 and hashbrown 0.14 duplicates (harmless) and pulls in `glicol_parser`/pest anyway. A direct `firewheel` dependency (same rev as seedling) is required by name for the `Diff`/`Patch` derives. |
| Control inputs | A custom glicol node (`LaneNode`) reads per-sample frequency/gate/velocity lanes the engine writes before each block. No locks; the atomics just satisfy `Send`. |
| Block adapter | Engine keeps one rendered 128-frame block and runs a constant **128 frames (2.7 ms @ 48 kHz)** behind, so on-time events are always applied on their exact frame. Tested with uneven sub-chunks (1, 13, 37, …, 512 frames). |
| Timing | Unit test: first sound lands on frame `event + LATENCY` (±2 frames of saw ramp). Recorded from the speakers: onsets sit within 1–2 ms of the 16th grid, which is the detector's own resolution. |
| Realtime safety | Counting-allocator test over 200 note-ons/offs plus filter changes: **0 allocations** while rendering. glicol allocates its processor maps on a graph's *first* block, so every voice is primed at construction. Without that, 21 allocations happened on the audio thread. |
| CPU | Release build, all voices sounding: **~0.11–0.16 % of one core per voice**. 64 voices ≈ 7–10 % of a core. Comfortably beats the 32-voice target. |
| Events | Notes travel as `NodeEventType::CustomBytes` (36 bytes, no heap), scheduled with `EventInstant::AtClockSeconds`. Parameters go through normal seedling diffing. |

**glicol quirks found (fix upstream in the fork when they start to matter):**
- `Adsr` attack always ramps from 0, not from the current level, so a
  retriggered or stolen voice clicks (`envelope/adsr.rs`, attack branch).
- `ResonantLowPassFilter` never updates `x0` and recomputes its coefficients
  every sample. In two-input mode it samples the cutoff only once per block.
- Oscillators are naive (non-band-limited), so high saw/square notes alias.
  A PolyBLEP saw/square would be a small, valuable fork patch.
- No parameter smoothing yet: fast cutoff moves can zipper. Smooth in the
  engine per block in Phase 3.
- Packaging: the fork's `main` is plain upstream. The `dev` branch (used by
  bevy_glicol) inherits `workspace = true` from the bevy_glicol repo root, so
  it can't be used as a git dependency on its own. Give the fork its own
  workspace root before we depend on `dev`.

### Phase 1 — Notes and instruments  *(the big refactor; no new sounds)*
| # | Task |
|---|------|
| 1.1 | `NoteEvent { pitch: f32 (midi), velocity, start: MusicalTime, length: Option<MusicalTime>, articulation: Articulation }` |
| 1.2 | `trait Instrument { fn play(&mut self, note: &NoteEvent, at: InstantSeconds, cmds: &mut Commands) }`, plus `Instrument` component. Musicians target an instrument entity |
| 1.3 | `SampleInstrument` wraps today's `Sampler` / `MultiSampler` logic (role picking, dynamics, residual pitch shift) |
| 1.4 | `MusicPlayer::play` returns or pushes `NoteEvent`s instead of spawning; port the built-in musicians |
| 1.5 | Port `real_orchestra` and `improv_orchestra` and keep them sounding identical |

This also gives the book's "sustained notes" homework for free, because notes
now have a `length`.

### Phase 2 — Audio-clock scheduling
| # | Task |
|---|------|
| 2.1 | Drive musical time from `Time<Audio>` (Firewheel's clock) instead of `Time` deltas |
| 2.2 | Look-ahead scheduler: each frame, ask musicians for notes in the next ~100 ms window, stamp them with exact `InstantSeconds` (the book's `BarsToEngineTime`) |
| 2.3 | Samples are scheduled with `PlaybackSettings::play_at`; synths get timestamped events |
| 2.4 | Tempo changes and quantised "start at next bar" requests become trivial |

#### Phase 1 + 2 results ✅

Done as one breaking pass, as planned. Differences from the plan:
- **Instrument lives on the `Musician`**, not on a separate entity:
  `Musician { player, instrument }`. A synth musician is one entity carrying
  `Musician` + `SynthNode` + `SynthConfig` (`synth::synth_musician`), and the
  routing system passes that entity's `FirewheelNode` to the instrument.
- **`MusicPlayer::take_instrument()`** lets built-in musicians keep their
  sampler-taking constructors (`Drummer::new(sampler, …)`, `create_bassist`, …)
  and hand the sampler over as the default instrument. `SuperDrummer` builds a
  `Channels` kit from its drummers. Existing user code that only used the
  constructors compiles unchanged.
- `NoteEvent` uses absolute MIDI pitch, with offset and length in steps. Musicians
  written in `midi_note_diff` use `NoteEvent::from_diff`, where 60 is a single
  sample's native pitch, so sampled examples sound as before.
- `MultiSampler` maps `velocity` → dynamic layer, and `articulation` → role.
  The orchestra examples pass intensity as velocity, as they did before.

**Timing, measured from the speakers** (same probe, 808 hi-hat on every 16th
at 120 bpm, game loop deliberately stuttering between 1 and 30 ms frames):

| Clock | 16th-note interval error |
|-------|--------------------------|
| Old: frame-driven `Time` deltas | −40 ms … +46 ms |
| New: `Time<Audio>` + 100 ms look-ahead | ±0.1 ms (one resampled frame) |

Also fixed along the way: the old clock never emitted bar 0's downbeat, and
`sample_browser` / `bevy_integration` compile again on Bevy 0.19.
`examples/synth_band.rs` runs the stock `Bassist` / `Arpeggiator` / `Soloist`
on synths (with a sampled 808 kit), with filter cutoffs following intensity.

### Phase 3 — Real synth instruments
| # | Task |
|---|------|
| 3.1 | Voice pool: polyphony limit, voice stealing (oldest or quietest), mono/legato mode, glide |
| 3.2 | Voice interface: `~freq`, `~gate`, `~trig`, `~vel`, and `#param` exposure |
| 3.3 | Exposed parameters as a component (`SynthParams`) using Firewheel `Diff`/`Patch`, smoothed per block |
| 3.4 | Built-in preset library as `.glicol` assets: sub bass, saw lead, pad (detuned saws + slow lpf), pluck (envperc), chip square, FM bell, noise hat/snare/kick (glicol's `bd`/`sn`/`hh` compounds are a head start) |
| 3.5 | `examples/synth_band.rs`: improv_orchestra's musicians on synths. Hybrid variant: sampled drums plus synth everything else |

#### Phase 3 results ✅ (with two items moved to Phase 4)

- **Our own DSP nodes** (`src/synth/dsp.rs`), written as glicol `Node`s so they
  sit in ordinary glicol graphs: PolyBLEP saw/pulse oscillators (plus sine,
  triangle, noise, FM input), an ADSR that re-attacks from its current level
  with exponential decay/release (no clicks on stolen or repeated notes), a
  Simper state-variable filter (LP/HP/BP) with smoothed cutoff/Q, envelope
  amount, key tracking and velocity, a pitch sweep for drums, and a 4-input
  sum. They replace glicol's `adsr`, `rlpf` and naive oscillators, whose
  problems (attack from 0, the `x0` bug, aliasing, no smoothing) are now gone.
- **Patches are data** (`SynthPatch` in `src/synth/patch.rs`): up to four
  oscillators (detune in cents, levels, pulse width, 2-op FM with its own
  envelope), amp and filter envelopes, filter modulation, pitch sweep, gain,
  `VoiceMode::{Poly, Mono, Legato}` and glide. Presets are functions:
  `saw_lead`, `glide_lead`, `pluck`, `sub_bass`, `acid_bass`, `pad`, `chip`,
  `bell`, `e_piano`, `kick`, `snare`, `hat` (`SynthPatch::presets()`).
- **Voicing** (3.1): poly with stealing (idle → oldest released → oldest
  held); mono/legato with a note stack (releasing the top note falls back to the
  one still held) and log-frequency glide. A note-off and note-on on the same
  frame are ordered off-first, so only *overlapping* notes slide.
- **Live parameters** (3.3): `SynthNode { volume, cutoff, resonance }` stays the
  `Diff`/`Patch` component; cutoff/Q glide over ~10 ms and volume ramps per
  block, so game systems can write them every frame.
- **Synth drum kits**: `SynthInstrument::on(entity)` plays another entity's
  synth; `spawn_synth_kit` builds one synth per drum and a `Channels` kit, and
  `Drummer::pattern` makes sampler-less drummers.
- **Examples**: `synth_band` now runs a generative acid bassline (legato
  slides), the stock arpeggiator/soloist, a pad, FM bell sparkles at high
  intensity and a synth kit (`-- --samples` for the hybrid 808 variant).
  `synth_presets` auditions every preset (`-- acid` for one).
- **Measured**: 0.18–0.30% of one core per voice (release build; ~7% for a
  30-voice band). The filter computes coefficients every 8 samples; doing it
  per sample cost twice as much. Still zero allocations on the render path. In
  a recording of `synth_band`, the level and the share of energy above 5 kHz
  rise and fall with intensity, with no clipping.
- **Moved to Phase 4**: 3.2's `#param` exposure and 3.4's presets *as `.glicol`
  assets* both need a glicol-text front end. Presets are Rust functions for
  now; the Phase 4 loader will build the same `SynthPatch`/graph from text.
  The custom nodes will need registering with that front end.
- **Not done**: improv_orchestra's own musicians on synths (they live in that
  example file). `synth_band` covers the idea with the library's musicians.

### Phase 4 — Instruments as assets and the editor
| # | Task |
|---|------|
| 4.1 | `SynthPatch` asset loader for `.glicol` files: parse, validate the interface, compile in a task |
| 4.2 | Hot reload: editing a `.glicol` file swaps the patch in place (new notes use the new patch, sounding voices finish) |
| 4.3 | Extract the Firewheel glicol node into `bevy_glicol` (Bevy 0.19, cpal stream removed); `bevy_glicol_ui` edits instruments that rusty_music plays |
| 4.4 | Optional: glicol sample nodes (`sp`) with samples leaked once on load |

### Phase 5 — Mixing and effects
| # | Task |
|---|------|
| 5.1 | A bus per section (drums, bass, harmony, lead) with volume and pan |
| 5.2 | Shared send effects: reverb (Firewheel `freeverb` or glicol `plate`), delay |
| 5.3 | Sidechain-style ducking: pads duck on kick; music ducks under dialogue |

### Phase 6 — Situational awareness  *(the fun part)*
| # | Task |
|---|------|
| 6.1 | `MusicState` resource the game writes: `intensity`, `tension`, `valence` (dark ↔ bright), `energy`, plus tags such as `Location::Cave` |
| 6.2 | Declarative **mappings** from state to music: `tension → lead.cutoff (400..4000 Hz, exp)`, `valence → mode (aeolian ↔ dorian ↔ ionian)`, `energy → tempo (80..140)`, `location == Cave → reverb.size 0.9` |
| 6.3 | The composer reacts to state: key/mode changes at section boundaries, harmonic rhythm speeds up with tension, cadences on "victory" |
| 6.4 | **Stingers and transitions**: game events (`PlayerHit`, `BossAppears`, `LevelCleared`) trigger musical gestures quantised to the next beat or bar (a fill, a riser, a held chord, a key change) |
| 6.5 | Per-entity music: a `Musician` attached to an enemy whose synth gets louder and brighter as it approaches (spatial nodes already exist in seedling) |
| 6.6 | `examples/synth_game.rs`: a small Bevy 2D scene where moving towards danger audibly changes the music |

#### Phase 6 results ✅ (done before Phases 4 and 5)

- **6.1 `MusicState`** (`src/situation.rs`): the game writes a target `Mood`
  (`intensity`, `tension`, `valence`, `energy`); `current()` glides towards it
  over `response` seconds. `jolt(dim, v)` skips the glide for hits and
  explosions; `snap()` for scene changes. `SituationPlugin` drives the old
  `Intensity` resource from it, so every existing musician responds unchanged.
  Location tags are not done yet: games map location to mood values themselves.
- **6.2 Mappings**:
  - `Layer::above(dim, threshold)`: musicians enter and leave at bar lines, with hysteresis (vertical layering).
  - `TempoFollows { dim, slow, fast }` drives the tempo (`Clock::set_bpm`).
  - `synth::Responds` maps dimensions onto `SynthNode` cutoff (exponential), resonance and volume.
- **6.3 Harmony** (`src/harmony.rs`): the progression is in scale degrees.
  `Harmony` rebuilds the conductor's chords at each phrase start. Mode follows
  valence (Phrygian → Aeolian → Dorian → Mixolydian → Ionian → Lydian), tension
  strengthens sevenths and ninths, and `key` changes land at the next phrase.
- **6.4 Stingers**: send a `Stinger { musician, gesture, quantize }` message;
  it plays on the next step, beat or bar, built from the chord sounding then
  (`Gesture::arpeggio_up`, `chord_stab`, `dissonant_hit`, `custom`, `Notes`).
  Stinger-only instruments are muted musicians: stingers play anyway.
- **6.6 `examples/synth_game.rs`**: orbs, hunters and a garden sanctuary;
  the game only writes mood and sends stingers. `-- --demo` adds an autopilot
  and logs the musical state per bar. In a 70 s recording of the demo, layers
  entered one by one, resting in the garden moved Aeolian → Ionian and 126 → 105
  bpm, a level-up modulated A → B, and the peak stayed at 0.75.
- **Not done**: 6.5 (per-entity music, e.g. an enemy's synth getting louder as
  it approaches) works today by writing that entity's `SynthNode` from game
  code; there is no dedicated helper yet.
- **Found: the music only advances while `Update` runs.** A window hidden on
  Wayland with vsync blocks every frame, and the music stalls with it (a bar
  took 15 s). Games that keep playing music while minimised need a fix:
  scheduling from a separate thread or schedule, or a longer look-ahead while
  unfocused. Visible windows are unaffected.


---

## 5. Open questions

1. **Glicol text or a Rust builder for instruments?** I recommend glicol text as
   the canonical format (editable, hot-reloadable, works with `bevy_glicol_ui`)
   plus a thin Rust builder that emits glicol text, for presets defined in code.
2. **`glicol_parser` on the critical path?** It pulls in pest and rhai (`meta`
   node). We can turn the `use-meta` feature off for game builds.
3. **One node per instrument, or one per voice?** I recommend one per instrument
   with an internal voice pool: cheaper, and mono/legato/glide become possible.
   Seedling's sampler pools work the other way; keep that for samples.
4. **wasm?** Firewheel supports web audio. Glicol originally targets wasm, so it
   should be feasible. Verify in Phase 0 if web builds matter.
5. **Where does the composer's "memory" live** once musicians emit events?
   Probably unchanged (inside each musician), but the scheduler's look-ahead
   means musicians are asked about the future. Seeded RNGs keep that deterministic.

---

## 6. Suggested order of work

*Status: Phases 0, 1, 2, 3 and 6 are done; 4 (glicol text, hot reload) and
5 (buses, reverb, ducking) remain.*

Phase 0 first, because it decides the DSP backend. Then Phase 1 and Phase 2
together, since both change the `MusicPlayer` contract and are best done in one
breaking pass. Phase 3 is where it starts being fun to listen to; Phase 6 is the
point of the whole thing.
