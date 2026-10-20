//! synth_game — situationally aware music in a tiny 2D game.
//!
//! You are the cyan square. Collect the golden orbs; avoid the red hunters.
//! The game never touches audio directly. It only describes the situation
//! (`MusicState`) and fires `Stinger`s; the music does the rest:
//!
//! | Situation                    | Mood            | What you hear                                  |
//! |------------------------------|-----------------|------------------------------------------------|
//! | hunter close                 | tension ↑       | filters open, sevenths/ninths, arp + lead enter |
//! | moving / being chased        | energy ↑        | tempo rises (92 → 132 bpm)                     |
//! | inside the green garden      | valence ↑       | mode brightens (aeolian → dorian → ionian)     |
//! | orb collected                | stinger         | bell arpeggio on the next 16th, in key         |
//! | hit by a hunter              | stinger, mood ↓ | dissonant stab on the next 16th, darker mode   |
//! | every 6th orb (level up)     | stinger, key ↑  | chord stab on the next bar, key rises a tone   |
//! | inside the garden            | mix             | the reverb opens into a big, soft hall         |
//! | hold Space ("dialogue")      | mix             | the music ducks 14 dB under the "voice"        |
//!
//! The mix (`MixPlugin`) puts each musician on a section bus with shared
//! reverb and echo; the kick sidechains the harmony section, so the pads
//! pump in time.
//!
//! Mode and key changes land at phrase starts (every 4 bars), layers enter
//! and leave at bar lines, stingers are quantised, so it always stays musical.
//!
//! `cargo run --example synth_game --features synth` — WASD / arrows to move, Esc quits.
//! Add `-- --demo` to let an autopilot play (it heads for the nearest orb and
//! ignores the hunters) and print the musical state every bar.

use bevy::prelude::*;
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use rusty_music::clock::{Beat, Clock};
use rusty_music::harmony::Harmony;
use rusty_music::mix::{Mix, MixPlugin, Mixer, Section, Sidechain};
use rusty_music::instrument::NoteEvent;
use rusty_music::musicians::arpeggiator::Arpeggiator;
use rusty_music::musicians::bassist::Bassist;
use rusty_music::musicians::drummer::{generate_hihat_beat, generate_kick_beat, generate_snare_beat, Drummer, SuperDrummer};
use rusty_music::musicians::soloist::Soloist;
use rusty_music::musicians::{Chord, MusicPlayer, Muted};
use rusty_music::situation::{Dim, Gesture, Layer, MusicState, Quantize, SituationPlugin, Stinger, TempoFollows};
use rusty_music::synth::{spawn_synth_kit_with, synth, synth_musician, Responds, SynthPatch, SynthPlugin};
use rusty_music::MusicPlugin;

const ARENA: Vec2 = Vec2::new(900.0, 600.0);
const GARDEN: Vec2 = Vec2::new(-280.0, 150.0);
const GARDEN_RADIUS: f32 = 110.0;
const PLAYER_SPEED: f32 = 260.0;
/// Orbs per level; one is on the field at a time.
const ORBS: u32 = 6;
const HUNTERS: usize = 4;

// ── Musicians the game owns ───────────────────────────────────────────────────

/// Holds the chord's main tones for a whole bar.
struct PadPlayer;

impl MusicPlayer for PadPlayer {
    fn play(&mut self, beat: Beat, notes: &mut Vec<NoteEvent>, intensity: f32, chord: &Chord) {
        if beat.beat == 0 && beat.sixteenth == 0 {
            for note in chord.chord_notes.iter().filter(|n| n.strength >= 0.8) {
                notes.push(NoteEvent::from_diff(note.midi_note_diff).velocity(0.5 + intensity * 0.5).length(16.0));
            }
        }
    }
}

/// Plays nothing by itself: an instrument reserved for stingers.
struct StingerOnly;

impl MusicPlayer for StingerOnly {
    fn play(&mut self, _: Beat, _: &mut Vec<NoteEvent>, _: f32, _: &Chord) {}
}

#[derive(Resource)]
struct Band {
    chime: Entity,
    alarm: Entity,
    fanfare: Entity,
}

// ── Game ─────────────────────────────────────────────────────────────────────

#[derive(Component)]
struct Player {
    hurt: f32,
}

#[derive(Component)]
struct Hunter {
    heading: Vec2,
    turn_in: f32,
}

#[derive(Component)]
struct Orb;

/// Query filter for a non-player entity of kind `T`, so it can coexist with a `&mut Transform` player query.
type Other<T> = (With<T>, Without<Player>);

#[derive(Component)]
struct Hud;

#[derive(Resource)]
struct Game {
    rng: StdRng,
    score: u32,
    level: u32,
    /// Seconds of post-victory glow left.
    triumph: f32,
    speed: f32,
    demo: bool,
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "synth_game — the music follows the situation".into(),
                resolution: (ARENA.x as u32, ARENA.y as u32).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins((MusicPlugin { beats: 4, note_type: 4, bpm: 100.0 }, SituationPlugin, MixPlugin, SynthPlugin))
        .insert_resource(ClearColor(Color::srgb(0.03, 0.03, 0.08)))
        .insert_resource(TempoFollows { dim: Dim::Energy, slow: 92.0, fast: 132.0 })
        .insert_resource(Game { rng: StdRng::seed_from_u64(42), score: 0, level: 1, triumph: 0.0, speed: 0.0, demo: std::env::args().any(|a| a == "--demo") })
        .add_systems(Startup, (setup_world, setup_band))
        .add_systems(Update, (move_player, move_hunters, collisions, describe_situation, shape_mix, hud, quit).chain())
        .run();
}

fn setup_band(mut commands: Commands, mixer: Res<Mixer>) {
    commands.insert_resource(MusicState::default().with_response(1.5));
    commands.insert_resource(Harmony { key: -3, progression: vec![0, 5, 2, 6], ..default() });

    // The core: always there, but colour and level follow the mood.
    let (node, config) = synth(SynthPatch::sub_bass(), -10.0);
    commands.spawn((
        synth_musician("Bass", Bassist::default(), -24.0, node, config),
        Section::Bass,
        Responds::default().volume_db(Dim::Intensity, -16.0, -9.0),
    ));
    let (node, config) = synth(SynthPatch::pad(), -6.0);
    commands.spawn((
        synth_musician("Pad", PadPlayer, -12.0, node, config),
        Section::Harmony,
        Responds::default().cutoff(Dim::Valence, 400.0, 2_500.0).volume_db(Dim::Tension, -8.0, -14.0),
    ));

    // Layers: enter as the situation heats up.
    let (node, config) = synth(SynthPatch::pluck(), -16.0);
    commands.spawn((
        synth_musician("Arp", Arpeggiator::default(), 0.0, node, config),
        Section::Harmony,
        Responds::default().cutoff(Dim::Tension, 350.0, 4_000.0),
        Layer::above(Dim::Intensity, 0.35),
        Muted,
    ));
    let (node, config) = synth(SynthPatch::glide_lead(), -15.0);
    commands.spawn((
        synth_musician("Lead", Soloist::with_bars(2), 12.0, node, config),
        Section::Lead,
        Responds::default().cutoff(Dim::Tension, 800.0, 4_500.0),
        Layer::above(Dim::Intensity, 0.6),
        Muted,
    ));
    let drums = SuperDrummer::new(vec![
        Drummer::pattern(generate_kick_beat()),
        Drummer::pattern(generate_snare_beat()),
        Drummer::pattern(generate_hihat_beat()),
    ]);
    let kit = vec![(SynthPatch::kick(), 36.0, -8.0), (SynthPatch::snare(), 55.0, -12.0), (SynthPatch::hat(), 60.0, -18.0)];
    // The kick (channel 0) pumps the harmony section: pads dip on every hit.
    let pump = |kit| Box::new(Sidechain::new(kit).channel(0).ducks(&mixer, Section::Harmony, 7.0, 0.22)) as Box<_>;
    let drums = spawn_synth_kit_with(&mut commands, "Drums", drums, kit, pump);
    commands.entity(drums).insert((Section::Drums, Layer::above(Dim::Intensity, 0.25), Muted));

    // Stinger-only instruments: muted, so they play nothing until the game asks.
    let mut stinger = |name: &str, patch: SynthPatch, transpose: f32, db: f32| {
        let (node, config) = synth(patch, db);
        commands.spawn((synth_musician(name, StingerOnly, transpose, node, config), Section::Fx, Muted)).id()
    };
    let band = Band {
        chime: stinger("Chime", SynthPatch::bell(), 24.0, -13.0),
        alarm: stinger("Alarm", SynthPatch::saw_lead().named("alarm"), 0.0, -13.0),
        fanfare: stinger("Fanfare", SynthPatch::e_piano(), 12.0, -10.0),
    };
    commands.insert_resource(band);
}

fn setup_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut game: ResMut<Game>,
) {
    commands.spawn(Camera2d);
    commands.spawn((
        Mesh2d(meshes.add(Circle::new(GARDEN_RADIUS))),
        MeshMaterial2d(materials.add(Color::srgba(0.2, 0.8, 0.4, 0.15))),
        Transform::from_translation(GARDEN.extend(-1.0)),
    ));
    commands.spawn((
        Player { hurt: 0.0 },
        Mesh2d(meshes.add(Rectangle::new(26.0, 26.0))),
        MeshMaterial2d(materials.add(Color::srgb(0.3, 0.9, 1.0))),
        Transform::from_xyz(0.0, -200.0, 1.0),
    ));
    let hunter_mesh = meshes.add(Circle::new(16.0));
    let hunter_mat = materials.add(Color::srgb(0.95, 0.2, 0.25));
    for k in 0..HUNTERS {
        let x = -300.0 + k as f32 * 200.0;
        commands.spawn((
            Hunter { heading: Vec2::from_angle(k as f32 * 1.7), turn_in: 0.0 },
            Mesh2d(hunter_mesh.clone()),
            MeshMaterial2d(hunter_mat.clone()),
            Transform::from_xyz(x, 220.0, 1.0),
        ));
    }
    spawn_orb(&mut commands, &mut meshes, &mut materials, &mut game.rng, Vec2::new(0.0, -200.0));
    commands.spawn((
        Text::new(""),
        TextFont { font_size: FontSize::Px(15.0), ..default() },
        TextColor(Color::WHITE),
        Node { position_type: PositionType::Absolute, top: Val::Px(10.0), left: Val::Px(12.0), ..default() },
        Hud,
    ));
}

/// Place the next orb somewhere well away from the player.
fn spawn_orb(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<ColorMaterial>, rng: &mut StdRng, player: Vec2) {
    let mut pos = player;
    while pos.distance(player) < 350.0 {
        pos = Vec2::new(rng.random_range(-400.0..400.0), rng.random_range(-260.0..260.0));
    }
    commands.spawn((
        Orb,
        Mesh2d(meshes.add(Circle::new(9.0))),
        MeshMaterial2d(materials.add(Color::srgb(1.0, 0.85, 0.2))),
        Transform::from_translation(pos.extend(0.5)),
    ));
}

fn move_player(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut game: ResMut<Game>,
    mut player: Single<(&mut Player, &mut Transform)>,
    orbs: Query<&Transform, Other<Orb>>,
    hunters: Query<&Transform, Other<Hunter>>,
) {
    let pressed = |a, b| keys.pressed(a) || keys.pressed(b);
    let mut dir = Vec2::ZERO;
    if pressed(KeyCode::KeyW, KeyCode::ArrowUp) { dir.y += 1.0; }
    if pressed(KeyCode::KeyS, KeyCode::ArrowDown) { dir.y -= 1.0; }
    if pressed(KeyCode::KeyA, KeyCode::ArrowLeft) { dir.x -= 1.0; }
    if pressed(KeyCode::KeyD, KeyCode::ArrowRight) { dir.x += 1.0; }
    let (player, transform) = &mut *player;
    if game.demo {
        // Autopilot: stroll to the orb, shy away from hunters, rest in the garden now and then.
        let pos = transform.translation.truncate();
        let resting = (time.elapsed_secs() % 30.0) > 22.0;
        let goal = if resting { Some(GARDEN) } else { orbs.iter().next().map(|o| o.translation.truncate()) };
        if let Some(goal) = goal.filter(|g| g.distance(pos) > 20.0) {
            dir = (goal - pos).normalize_or_zero();
        }
        for hunter in &hunters {
            let away = pos - hunter.translation.truncate();
            if away.length() < 120.0 {
                dir += away.normalize_or_zero() * 1.5;
            }
        }
    }
    let step = dir.normalize_or_zero() * PLAYER_SPEED * time.delta_secs();
    let pos = (transform.translation.truncate() + step).clamp(-ARENA / 2.0 + 15.0, ARENA / 2.0 - 15.0);
    transform.translation = pos.extend(1.0);
    player.hurt = (player.hurt - time.delta_secs()).max(0.0);
    game.speed = if dir == Vec2::ZERO { 0.0 } else { 1.0 };
}

fn move_hunters(
    time: Res<Time>,
    mut game: ResMut<Game>,
    player: Single<&Transform, With<Player>>,
    mut hunters: Query<(&mut Hunter, &mut Transform), Without<Player>>,
) {
    let dt = time.delta_secs();
    let target = player.translation.truncate();
    let speed = (90.0 + 15.0 * game.level as f32).min(200.0);
    for (mut hunter, mut transform) in &mut hunters {
        let pos = transform.translation.truncate();
        let to_player = target - pos;
        let in_garden = target.distance(GARDEN) < GARDEN_RADIUS;
        if to_player.length() < 260.0 && !in_garden {
            hunter.heading = to_player.normalize_or_zero();
        } else {
            hunter.turn_in -= dt;
            if hunter.turn_in <= 0.0 {
                hunter.heading = Vec2::from_angle(game.rng.random_range(0.0..std::f32::consts::TAU));
                hunter.turn_in = game.rng.random_range(1.0..3.0);
            }
        }
        let mut next = pos + hunter.heading * speed * dt;
        // The garden is a sanctuary: hunters slide around its edge.
        if next.distance(GARDEN) < GARDEN_RADIUS + 16.0 {
            next = GARDEN + (next - GARDEN).normalize_or_zero() * (GARDEN_RADIUS + 16.0);
        }
        let bound = ARENA / 2.0 - 16.0;
        if next.x.abs() > bound.x || next.y.abs() > bound.y {
            hunter.heading = -hunter.heading;
        }
        transform.translation = next.clamp(-bound, bound).extend(1.0);
    }
}

#[allow(clippy::too_many_arguments)]
fn collisions(
    mut commands: Commands,
    mut game: ResMut<Game>,
    mut state: ResMut<MusicState>,
    mut harmony: ResMut<Harmony>,
    band: Res<Band>,
    mut stingers: MessageWriter<Stinger>,
    mut player: Single<(&mut Player, &mut Transform)>,
    hunters: Query<&Transform, Other<Hunter>>,
    orbs: Query<(Entity, &Transform), Other<Orb>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let (player, transform) = &mut *player;
    let pos = transform.translation.truncate();

    for (orb, orb_transform) in &orbs {
        if orb_transform.translation.truncate().distance(pos) < 22.0 {
            commands.entity(orb).despawn();
            game.score += 1;
            stingers.write(Stinger { musician: band.chime, gesture: Gesture::arpeggio_up(5, 1.0, 0.8), quantize: Quantize::Step });
            spawn_orb(&mut commands, &mut meshes, &mut materials, &mut game.rng, pos);
            if game.score.is_multiple_of(ORBS) {
                // Level cleared: a chord on the next downbeat, then up a whole tone.
                stingers.write(Stinger { musician: band.fanfare, gesture: Gesture::chord_stab(14.0, 1.0), quantize: Quantize::Bar });
                harmony.key += 2;
                if harmony.key > 3 {
                    harmony.key -= 12;
                }
                game.level += 1;
                game.triumph = 8.0;
            }
        }
    }

    if player.hurt <= 0.0 {
        for hunter in &hunters {
            let away = pos - hunter.translation.truncate();
            if away.length() < 30.0 {
                player.hurt = 1.5;
                transform.translation += (away.normalize_or_zero() * 60.0).extend(0.0);
                stingers.write(Stinger { musician: band.alarm, gesture: Gesture::dissonant_hit(3.0), quantize: Quantize::Step });
                // A hit lands hard: no gliding on the way into panic.
                state.jolt(Dim::Tension, 1.0);
                state.jolt(Dim::Valence, 0.0);
                break;
            }
        }
    }
}

/// The only place the game talks to the music: describe the situation.
fn describe_situation(
    time: Res<Time>,
    mut game: ResMut<Game>,
    mut state: ResMut<MusicState>,
    player: Single<(&Player, &Transform)>,
    hunters: Query<&Transform, With<Hunter>>,
) {
    let (player, transform) = *player;
    let pos = transform.translation.truncate();
    let nearest = hunters.iter().map(|h| h.translation.truncate().distance(pos)).fold(f32::MAX, f32::min);
    let chasers = hunters.iter().filter(|h| h.translation.truncate().distance(pos) < 260.0).count() as f32;
    let in_garden = pos.distance(GARDEN) < GARDEN_RADIUS;
    game.triumph = (game.triumph - time.delta_secs()).max(0.0);

    let danger = (1.0 - nearest / 300.0).clamp(0.0, 1.0);
    let progress = (game.score % ORBS) as f32 / ORBS as f32;
    let mood = &mut state.target;
    mood.tension = if in_garden { 0.05 } else { danger.max(player.hurt / 1.5) };
    mood.intensity = (0.15 + 0.4 * danger + 0.06 * chasers + 0.3 * progress).min(1.0);
    mood.energy = (0.25 + 0.35 * game.speed + 0.4 * danger).min(1.0);
    mood.valence = if game.triumph > 0.0 || in_garden {
        0.85
    } else if player.hurt > 0.0 {
        0.05
    } else {
        0.45 - 0.25 * danger
    };
}

#[allow(clippy::too_many_arguments)]
fn hud(
    game: Res<Game>,
    mut last_bar: Local<u32>,
    time: Res<Time<Real>>,
    state: Res<MusicState>,
    harmony: Res<Harmony>,
    clock: Res<Clock>,
    mix: Res<Mix>,
    muted: Query<&rusty_music::musicians::Musician, With<Muted>>,
    mut text: Single<&mut Text, With<Hud>>,
) {
    let mood = state.current();
    let bar = |v: f32| format!("{}{}", "█".repeat((v * 20.0) as usize), "·".repeat(20 - (v * 20.0) as usize));
    let resting: Vec<&str> = muted.iter().map(|m| m.name.as_str()).filter(|n| !["Chime", "Alarm", "Fanfare"].contains(n)).collect();
    let notes = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];
    if game.demo && *last_bar != clock.bar_count + 1 {
        *last_bar = clock.bar_count + 1;
        println!(
            "{:>6.1}s bar {:>3} orbs {:>2} | int {:.2} ten {:.2} val {:.2} nrg {:.2} | {} {:?} {:.0} bpm | resting: {}",
            time.elapsed_secs(), *last_bar, game.score, mood.intensity, mood.tension, mood.valence, mood.energy,
            ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"][harmony.key.rem_euclid(12) as usize],
            harmony.mode, clock.tempo_bpm, resting.join(","),
        );
    }
    text.0 = format!(
        "orbs {}   level {}   {}\n\nintensity {}\ntension   {}\nvalence   {}\nenergy    {}\n\n{} {:?}  ·  {:.0} bpm  ·  bar {}\nresting: {}",
        game.score,
        game.level,
        if mix.duck_db > 0.0 { "(music ducked for dialogue)" } else { "" },
        bar(mood.intensity),
        bar(mood.tension),
        bar(mood.valence),
        bar(mood.energy),
        notes[harmony.key.rem_euclid(12) as usize],
        harmony.mode,
        clock.tempo_bpm,
        clock.bar_count + 1,
        if resting.is_empty() { "nobody — full band".to_string() } else { resting.join(", ") },
    );
}

/// The mix responds to place and to "dialogue": a hall in the garden, ducking while Space is held.
fn shape_mix(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    game: Res<Game>,
    player: Single<&Transform, With<Player>>,
    mut mix: ResMut<Mix>,
) {
    let in_garden = player.translation.truncate().distance(GARDEN) < GARDEN_RADIUS;
    let (room, harmony_send, lead_send) = if in_garden { (0.92, -6.0, -9.0) } else { (0.72, -14.0, -16.0) };
    // The demo autopilot "talks" from 40 to 46 s.
    let talking = keys.pressed(KeyCode::Space) || (game.demo && (40.0..46.0).contains(&time.elapsed_secs()));
    let duck = if talking { 14.0 } else { 0.0 };
    // Write only on change: `Mix` change detection drives the audio-graph updates.
    if mix.reverb.room_size != room || mix.duck_db != duck {
        mix.reverb.room_size = room;
        mix.section(Section::Harmony).reverb_db = harmony_send;
        mix.section(Section::Lead).reverb_db = lead_send;
        mix.duck_db = duck;
    }
}

fn quit(keys: Res<ButtonInput<KeyCode>>, mut exit: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}
