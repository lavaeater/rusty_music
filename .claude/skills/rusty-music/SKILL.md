---
name: rusty-music
description: Add situationally aware, generative music to a Bevy 0.19 game with the rusty_music crate (lavaeater/rusty_music) — dependency setup, plugins, a band of synth/sample musicians, mapping game state to mood, mixing, stingers on game events, and glicol instrument files. Use when the user wants background/adaptive/dynamic/generative music, music that reacts to gameplay, or mentions rusty_music.
---

# Integrating rusty_music into a Bevy game

rusty_music generates the music at runtime. The game never plays tracks; it
**describes the situation** (a `MusicState` mood) and **fires stingers** on
events. Musicians improvise notes on a beat clock, instruments (synths or
samples) play them sample-accurately, and a mixer gives it space.

Work through the steps in order. Read the game's code first: find its states,
danger/health/enemy-distance logic, menus, and event types, because step 4
(mapping) is where the integration lives or dies.

## 1. Dependencies (Bevy 0.19 only)

```toml
[dependencies]
rusty_music = { git = "https://github.com/lavaeater/rusty_music", branch = "main", features = ["synth"] }

# REQUIRED: [patch] in a dependency does not propagate. bevy_seedling has no
# crates.io release for Bevy 0.19, so the game must patch it too, at the same rev.
[patch.crates-io]
bevy_seedling = { git = "https://github.com/corvusprudens/bevy_seedling", rev = "9db62d12daa11a2ffbd9da1565154127eac0a241" }

# Strongly recommended: audio DSP in an unoptimised build stutters.
[profile.dev.package."*"]
opt-level = 3
```

- Check the seedling rev against rusty_music's own `Cargo.toml` `[patch.crates-io]`.
- `features = ["synth"]` enables synths and glicol patches. Without it you
  get samples only; most of this guide assumes it.
- For hot-reloading `.glicol` files during development, enable Bevy's
  `file_watcher` feature.
- **Do not add `SeedlingPlugins` yourself**: `MusicPlugin` adds it. Adding it
  twice panics. If the game already uses bevy_seedling for sound effects, keep
  using it — just remove its `SeedlingPlugins` and let `MusicPlugin` add it
  (same crate version via the patch above).

## 2. Plugins (order matters)

```rust
use rusty_music::{MusicPlugin, mix::MixPlugin, situation::SituationPlugin, synth::SynthPlugin};

app.add_plugins((
    MusicPlugin { beats: 4, note_type: 4, bpm: 100.0 }, // 4/4; bpm is the starting tempo
    SituationPlugin, // MusicState mood, Harmony, layers, tempo, stingers
    MixPlugin,       // section buses, reverb/echo, ducking
    SynthPlugin,     // synths + .glicol assets
));
```

`MusicPlugin` must come first. `SituationPlugin` inserts a default `Harmony`
(A minor, i–VI–III–VII) and builds the chords from it; insert your own
`Harmony` resource to change key or progression.

## 3. The band

Spawn musicians in a `Startup` system. Every musician gets a mix `Section`.
Typical band (all names exist in the crate):

```rust
use rusty_music::harmony::Harmony;
use rusty_music::mix::{Mixer, Section, Sidechain};
use rusty_music::musicians::{arpeggiator::Arpeggiator, bassist::Bassist, soloist::Soloist, Muted};
use rusty_music::musicians::drummer::{Drummer, SuperDrummer, generate_kick_beat, generate_snare_beat, generate_hihat_beat};
use rusty_music::situation::{Dim, Layer, MusicState};
use rusty_music::synth::{synth, synth_musician, spawn_synth_kit_with, Responds, SynthPatch};

fn setup_music(mut commands: Commands, mixer: Res<Mixer>) {
    commands.insert_resource(MusicState::default().with_response(1.5)); // seconds to follow mood changes
    commands.insert_resource(Harmony { key: -3, progression: vec![0, 5, 2, 6], ..default() }); // A: i VI III VII

    // synth_musician(name, player, transpose_semitones, node, config)
    let (node, config) = synth(SynthPatch::sub_bass(), -10.0); // (patch, volume dB)
    commands.spawn((synth_musician("Bass", Bassist::default(), -24.0, node, config), Section::Bass));

    let (node, config) = synth(SynthPatch::pad(), -6.0);
    commands.spawn((synth_musician("Pad", MyPadPlayer, -12.0, node, config), Section::Harmony,
        Responds::default().cutoff(Dim::Valence, 400.0, 2_500.0)));

    // Layers: start Muted; they enter/leave at bar lines as the mood crosses the threshold.
    let (node, config) = synth(SynthPatch::pluck(), -16.0);
    commands.spawn((synth_musician("Arp", Arpeggiator::default(), 0.0, node, config), Section::Harmony,
        Responds::default().cutoff(Dim::Tension, 350.0, 4_000.0), Layer::above(Dim::Intensity, 0.35), Muted));

    let (node, config) = synth(SynthPatch::glide_lead(), -15.0);
    commands.spawn((synth_musician("Lead", Soloist::with_bars(2), 12.0, node, config), Section::Lead,
        Layer::above(Dim::Intensity, 0.6), Muted));

    // Synth drum kit: (patch, MIDI pitch, volume dB) per drum = channel 0, 1, 2. Kick pumps the pads.
    let drums = SuperDrummer::new(vec![Drummer::pattern(generate_kick_beat()),
        Drummer::pattern(generate_snare_beat()), Drummer::pattern(generate_hihat_beat())]);
    let kit = vec![(SynthPatch::kick(), 36.0, -8.0), (SynthPatch::snare(), 55.0, -12.0), (SynthPatch::hat(), 60.0, -18.0)];
    let pump = |kit| Box::new(Sidechain::new(kit).channel(0).ducks(&mixer, Section::Harmony, 7.0, 0.22)) as Box<_>;
    let drums = spawn_synth_kit_with(&mut commands, "Drums", drums, kit, pump);
    commands.entity(drums).insert((Section::Drums, Layer::above(Dim::Intensity, 0.25), Muted));
}
```

- **Prefer the improvisers** in `rusty_music::improv` for real music: they
  develop motifs, walk the bass into chord changes, voice-lead pads and
  generate drum fills, deterministically per seed, over any `Harmony`:
  `MotifSoloist::new(seed)` (lead, transpose 0), `WalkingBass::new(seed)` (-24),
  `VoiceLedPad::new(seed).voices(3)` (-12; `.stabs()` for rhythmic chords),
  `ChimeArp::new(seed)` (0), `GrooveDrummer::new(seed)` (kit channels
  `improv::kit::{KICK, SNARE, HAT}` = 0, 1, 2; `.with_toms()`/`.with_crash()`
  add channels 3/4), `Drone { interval: -12 }` (a held root, for things that hum).
  Use them in place of `Bassist`/`Soloist`/`Arpeggiator`/`SuperDrummer` above,
  e.g. `synth_musician("Bass", WalkingBass::new(1), -24.0, node, config)` and
  `spawn_synth_kit_with(&mut commands, "Drums", GrooveDrummer::new(5), kit, pump)`.
- **Transpose** puts a musician in its synth's register: musicians write
  around middle C, so bass `-24`, pads `-12`, leads `+12`.
- **Presets**: `SynthPatch::{saw_lead, glide_lead, pluck, sub_bass, acid_bass,
  pad, chip, bell, e_piano, kick, snare, hat}()`. Pick for the game's style:
  chiptune → `chip`; dark/tense → `pad`, `sub_bass`, `acid_bass`; magical → `bell`, `e_piano`.
- **Samples instead of synths**: `rusty_music::create_bassist(name, handle, volume_db)`
  (also `create_arpeggiator`, `create_soloist(.., record_bars)`, `create_drummer(.., pattern)`;
  `create_drummer_only` for `SuperDrummer`), or `Musician::new(name, player)
  .with_instrument(Sampler { handle, volume })`. Samples need `.wav` assets
  (seedling's `wav` feature is on). Add a `Section` to route them through the mixer.
- A **custom player** is a `MusicPlayer` impl that pushes `NoteEvent`s:

```rust
use rusty_music::{clock::Beat, instrument::NoteEvent, musicians::{Chord, MusicPlayer}};
struct MyPadPlayer;
impl MusicPlayer for MyPadPlayer {
    fn play(&mut self, beat: Beat, notes: &mut Vec<NoteEvent>, intensity: f32, chord: &Chord) {
        if beat.beat == 0 && beat.sixteenth == 0 { // once per bar
            for n in chord.chord_notes.iter().filter(|n| n.strength >= 0.8) {
                // from_diff = semitones from middle C; length/offset in 16th steps
                notes.push(NoteEvent::from_diff(n.midi_note_diff).velocity(0.5 + 0.5 * intensity).length(16.0));
            }
        }
    }
}
```

## 4. Map game state to mood — the important part

Write **one** system that describes the situation every frame by setting
`MusicState::target`. Never touch audio elsewhere. All values are 0–1:

| Dimension   | Means                     | Typical sources                                          |
|-------------|---------------------------|----------------------------------------------------------|
| `intensity` | how much is going on      | enemies in range, combat active, objective progress      |
| `tension`   | danger, suspense          | distance to nearest threat, low health, timer running out |
| `valence`   | dark (0) … bright (1)     | safe zones, winning/losing, story mood, hit → near 0      |
| `energy`    | calm … frantic            | player speed, chase, combo                               |

```rust
fn describe_situation(mut state: ResMut<MusicState>, player: Single<&Transform, With<Player>>,
                      enemies: Query<&Transform, With<Enemy>>) {
    let pos = player.translation.truncate();
    let nearest = enemies.iter().map(|e| e.translation.truncate().distance(pos)).fold(f32::MAX, f32::min);
    let danger = (1.0 - nearest / 300.0).clamp(0.0, 1.0);
    let mood = &mut state.target;
    mood.tension = danger;
    mood.intensity = 0.2 + 0.6 * danger;
    mood.energy = 0.3 + 0.5 * danger;
    mood.valence = 0.45 - 0.3 * danger;
}
```

Then decide what each dimension *does*:
- `intensity` already drives every musician's density (they filter notes by strength) and the `Layer`s.
- `valence` picks the mode at each phrase start (dark Phrygian → bright Lydian) when `Harmony::mode_follows_valence` (default on).
- `tension` adds 7ths/9ths to the chords; map it to filters with `Responds`.
- `energy` → tempo: `app.insert_resource(TempoFollows { dim: Dim::Energy, slow: 90.0, fast: 135.0 })`.

Rules of thumb:
- Keep values **continuous** (distances, ratios), not just booleans, so the music breathes.
- Sudden events: `state.jolt(Dim::Tension, 1.0)` skips the glide. Scene change / load: `state.snap()`.
- Menus/pause: lower intensity/energy, or set `Clock::playing = false` to stop the music.
- Changing key for a new level: `harmony.key += 2` (applies at the next phrase).

## 5. Stingers for game events

Use a dedicated stinger instrument per sound: a **muted** musician whose
player plays nothing. Stingers play on it anyway, quantised and in key.

```rust
struct StingerOnly;
impl MusicPlayer for StingerOnly { fn play(&mut self, _: Beat, _: &mut Vec<NoteEvent>, _: f32, _: &Chord) {} }

// setup: let chime = commands.spawn((synth_musician("Chime", StingerOnly, 24.0, node, config), Section::Fx, Muted)).id();
// store the Entity in a resource.

fn on_pickup(mut stingers: MessageWriter<Stinger>, band: Res<Band>) {
    stingers.write(Stinger { musician: band.chime, gesture: Gesture::arpeggio_up(5, 1.0, 0.8), quantize: Quantize::Step });
}
```

Gestures: `arpeggio_up(count, every_steps, velocity)` (rewards), `chord_stab(length_steps, velocity)`
(level up, use `Quantize::Bar`), `dissonant_hit(length)` (damage), `Gesture::custom(|chord| …)`.
Quantize: `Step` (next 16th, feels immediate), `Beat`, `Bar` (big moments).

## 5b. Things in the world that make music

Put a synth musician *on a game entity* (it needs a `Transform`) and give it
`Proximity`; mark the player or camera with `MusicListener`. It then fades in
and brightens as the listener approaches, in time and in key with the band:

```rust
use rusty_music::{improv::Drone, synth::{MusicListener, Proximity}};
// player: commands.spawn((Player, MusicListener, Transform::default()));
let (node, config) = synth(SynthPatch::saw_lead().named("hum"), -60.0);
commands.spawn((Enemy, Transform::from_xyz(300.0, 0.0, 0.0),
    synth_musician("Enemy hum", Drone { interval: -12 }, 0.0, node, config.voices(2)), Section::Fx,
    Proximity::new(40.0, 320.0).volume(-16.0, -60.0).cutoff(250.0, 1_800.0)));
```

## 6. Mixing

`Mix` is a resource; change fields and the audio graph follows.
- Dialogue/cutscene ducking: `mix.duck_db = 14.0;` (back to `0.0` after).
- Places: define named mixes once and switch between them; the mix glides:
  `app.insert_resource(MixScenes { glide: 1.5, ..default() }.with("open", open_mix).with("cave", cave_mix))`,
  then `scenes.activate("cave")` from a game system (`ResMut<MixScenes>`). Build
  each scene from `Mix::default()` and change e.g. `reverb.room_size`, section sends.
- One-off tweaks: `mix.reverb.room_size = 0.9; mix.section(Section::Harmony).reverb_db = -6.0;`
- Per-section level/pan/sends: `mix.section(Section::Lead).volume_db = -3.0;`
- Echo length is in steps (`mix.echo.steps = 3.0` = dotted eighth) and follows tempo.
- Only write `Mix` when a value actually changes (change detection drives graph updates).

## 7. Custom instruments as `.glicol` files (optional)

Put patches in `assets/synths/` and spawn with
`glicol_musician(name, player, transpose, GlicolSynth::new(assets.load("synths/x.glicol")).volume_db(-8.0))`.
The format, supported nodes, `#knob`s and `@` directives are documented at the
top of rusty_music's `src/synth/glicol.rs`; ready-made examples are in its
`assets/synths/`. With `file_watcher`, saving a file changes the sound live.
Map knobs to mood with `Responds::default().knob(Dim::Tension, index, from, to)`
(`index` = order of first appearance of `#name`, `#cutoff` excluded).

## 8. Verify

1. `cargo build` — the most common failure is the missing `[patch.crates-io]`
   (symptoms: two versions of `bevy_seedling`/`bevy`, or seedling not
   compiling against Bevy 0.19).
2. Run the game: music should start within a second. No sound at all → the
   cpal feature/backend (rusty_music enables it), or a second
   `SeedlingPlugins`. Samples silent but synths fine → assets not found; when
   running a built binary directly, set `CARGO_MANIFEST_DIR` to the project root.
3. Exercise the mapping: approach danger, enter safe areas, trigger events;
   the music should change within a bar or two (mode/key at phrase starts).

## Known limitations

- The music advances only while Bevy's `Update` runs. The clock bridges slow
  frames by scheduling further ahead (up to `Clock::max_lookahead`, 1.5 s), so
  a hidden window updating once a second still plays normally; a game whose
  updates stop entirely goes quiet.
- Beats are scheduled ahead (~0.1 s, more when frames are slow), so UI that
  reads `Clock` runs slightly early, and mood changes are heard that much later.
- Synth cost is ~0.2–0.3 % of a core per sounding voice; keep `voices` modest
  (`SynthConfig::voices`, default 8; mono/legato patches use 1).

Reference implementations in the rusty_music repo: `examples/synth_game.rs`
(complete game integration: improvising band, mood mapping, layers, stingers,
humming enemies with `Proximity`, mix scenes, ducking),
`examples/synth_live.rs` (glicol files + knobs), `examples/synth_band.rs`.
