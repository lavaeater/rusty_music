//! bevy_integration — epic orchestral score driven by 2D game distance.
//!
//! Move the player (blue square) toward the golden Relic. As you close in, the
//! orchestral score builds in intensity — new layers enter and the existing ones
//! grow louder and more active. Collect the Relic (touch it) and it teleports;
//! the music fades until you hunt it down again.
//!
//! This example shows how to wire rusty_music into a real Bevy game:
//!   1. `Intensity` resource lives in ECS — any game system can write to it.
//!   2. The music engine reads Intensity every beat and adjusts automatically.
//!   3. No audio code in the game loop — just one float.
//!
//! Controls:
//!   WASD / arrow keys   move the player
//!   Esc                 quit

use std::collections::HashMap;

use bevy::prelude::*;
use bevy_seedling::prelude::{AudioSample, PlaybackSettings, SamplePlayer, Volume};

use rusty_music::clock::Beat;
use rusty_music::musicians::conductor::Conductor;
use rusty_music::musicians::drummer::{
    generate_double_time_snare_beat, generate_half_time_kick_beat,
    generate_half_time_snare_beat, generate_snare_fill_beat, SuperDrummer,
};
use rusty_music::musicians::soloist::Soloist;
use rusty_music::musicians::{midi_diff_to_pitch, Chord, MusicPlayer, Note, Sampler, TonalPlayer};
use rusty_music::musicians::Musician;
use rusty_music::player::Intensity;
use rusty_music::{create_drummer_only, MusicPlugin};

// ── Chord progression (D natural minor: i → VII → VI → VII) ──────────────────

fn epic_chords() -> Vec<Chord> {
    let scale: Vec<Note> = [
        (-12i32, 0.9), (-10, 0.5), (-9, 0.6), (-7, 0.7), (-5, 0.8),
        (-4, 0.6), (-2, 0.5), (0, 1.0), (2, 0.5), (3, 0.7),
        (5, 0.6), (7, 0.8), (8, 0.5), (10, 0.4), (12, 0.4), (15, 0.3),
    ]
    .into_iter()
    .map(|(d, s)| Note::new(d, s))
    .collect();

    vec![
        Chord::new(0.0, vec![
            Note::new(-12, 1.0), Note::new(0, 1.0), Note::new(3, 0.9),
            Note::new(7, 0.8), Note::new(-5, 0.7), Note::new(10, 0.5),
            Note::new(12, 0.4), Note::new(15, 0.3),
        ], scale.clone()),
        Chord::new(1.0, vec![
            Note::new(-14, 1.0), Note::new(-2, 1.0), Note::new(2, 0.9),
            Note::new(7, 0.8), Note::new(-5, 0.7), Note::new(10, 0.4),
            Note::new(-9, 0.3),
        ], scale.clone()),
        Chord::new(2.0, vec![
            Note::new(-16, 1.0), Note::new(-4, 1.0), Note::new(-1, 0.9),
            Note::new(3, 0.8), Note::new(-9, 0.7), Note::new(8, 0.5),
            Note::new(-7, 0.4),
        ], scale.clone()),
        Chord::new(3.0, vec![
            Note::new(-14, 1.0), Note::new(-2, 1.0), Note::new(2, 0.9),
            Note::new(7, 0.8), Note::new(-5, 0.7), Note::new(12, 0.4),
            Note::new(0, 0.3),
        ], scale.clone()),
    ]
}

// ── Custom MusicPlayer implementations ───────────────────────────────────────

struct OrchestraPad {
    sampler: Sampler,
}

impl MusicPlayer for OrchestraPad {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, chord: &Chord) {
        if base_intensity < 0.12 || beat.sixteenth != 0 {
            return;
        }
        let active = match beat.beat {
            0 => true,
            2 => base_intensity > 0.28,
            1 | 3 => base_intensity > 0.62,
            _ => false,
        };
        if !active {
            return;
        }
        let min_strength = 1.0 - base_intensity;
        if let Some(note) = TonalPlayer::get_chord_note(chord, min_strength) {
            let vol = self.sampler.volume as f32 - 3.0 + base_intensity * 4.0;
            commands.spawn((
                SamplePlayer::new(self.sampler.handle.clone()).with_volume(Volume::Decibels(vol)),
                PlaybackSettings::default().with_speed(midi_diff_to_pitch(note.midi_note_diff)),
            ));
        }
    }
}

struct OstinatoStrings {
    sampler: Sampler,
    seq_idx: u32,
}

impl MusicPlayer for OstinatoStrings {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, chord: &Chord) {
        if base_intensity < 0.22 {
            return;
        }
        let step: u32 = if base_intensity < 0.42 { 4 }
                        else if base_intensity < 0.68 { 2 }
                        else { 1 };
        if TonalPlayer::flat_step(&beat) % step != 0 {
            return;
        }
        let notes = &chord.chord_notes;
        if notes.is_empty() {
            return;
        }
        let note = &notes[(self.seq_idx as usize) % notes.len()];
        self.seq_idx = self.seq_idx.wrapping_add(1);
        if note.strength < (1.0 - base_intensity) {
            return;
        }
        let vol = self.sampler.volume as f32 - 1.0 + base_intensity * 2.0;
        commands.spawn((
            SamplePlayer::new(self.sampler.handle.clone()).with_volume(Volume::Decibels(vol)),
            PlaybackSettings::default().with_speed(midi_diff_to_pitch(note.midi_note_diff)),
        ));
    }
}

struct BrassSection {
    samples: Vec<Handle<AudioSample>>,
    volume: f32,
}

impl MusicPlayer for BrassSection {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, chord: &Chord) {
        if base_intensity < 0.42 || self.samples.is_empty() {
            return;
        }
        let on_quarter = beat.sixteenth == 0 && match beat.beat {
            0 | 2 => true,
            1 | 3 => base_intensity > 0.75,
            _ => false,
        };
        let syncopated = base_intensity > 0.88 && beat.beat == 1 && beat.sixteenth == 2;
        if !on_quarter && !syncopated {
            return;
        }
        let num_voices: usize = if base_intensity > 0.85 { 3 }
                                else if base_intensity > 0.62 { 2 }
                                else { 1 };
        let min_strength = (1.0 - base_intensity).max(0.0);
        let strong_notes: Vec<&Note> = chord.chord_notes.iter()
            .filter(|n| n.strength >= min_strength)
            .take(num_voices)
            .collect();
        for (i, note) in strong_notes.iter().enumerate() {
            let vol = self.volume - i as f32 * 1.8;
            let si = (beat.beat as usize + beat.bar_count as usize * 3 + i) % self.samples.len();
            commands.spawn((
                SamplePlayer::new(self.samples[si].clone()).with_volume(Volume::Decibels(vol)),
                PlaybackSettings::default().with_speed(midi_diff_to_pitch(note.midi_note_diff)),
            ));
        }
    }
}

struct CymbalLayer {
    crash: Handle<AudioSample>,
    ride: Handle<AudioSample>,
}

impl MusicPlayer for CymbalLayer {
    fn play(&mut self, beat: Beat, commands: &mut Commands, base_intensity: f32, _chord: &Chord) {
        if beat.beat == 0 && beat.sixteenth == 0 && base_intensity > 0.52 {
            if beat.bar_count % 2 == 0 || base_intensity > 0.82 {
                let vol = -8.0 + (base_intensity - 0.52).max(0.0) / 0.48 * 9.0;
                commands.spawn((
                    SamplePlayer::new(self.crash.clone()).with_volume(Volume::Decibels(vol)),
                    PlaybackSettings::default(),
                ));
            }
        }
        if base_intensity > 0.48 && beat.sixteenth == 2 && beat.beat % 2 == 1 {
            commands.spawn((
                SamplePlayer::new(self.ride.clone())
                    .with_volume(Volume::Decibels(-6.0 + base_intensity * 3.0)),
                PlaybackSettings::default(),
            ));
        }
    }
}

// ── Percussion patterns ────────────────────────────────────────────────────────

fn timpani_normal() -> HashMap<(u32, u32), Note> {
    HashMap::from([
        ((0, 0), Note::new(0, 1.0)),
        ((2, 0), Note::new(0, 1.0)),
        ((1, 2), Note::new(0, 0.3)),
        ((3, 2), Note::new(0, 0.3)),
    ])
}

fn timpani_march() -> HashMap<(u32, u32), Note> {
    HashMap::from([
        ((0, 0), Note::new(0, 1.0)),
        ((1, 0), Note::new(0, 0.85)),
        ((2, 0), Note::new(0, 1.0)),
        ((3, 0), Note::new(0, 0.85)),
        ((0, 2), Note::new(0, 0.35)),
        ((2, 2), Note::new(0, 0.35)),
    ])
}

fn snare_orchestral() -> HashMap<(u32, u32), Note> {
    HashMap::from([
        ((1, 0), Note::new(0, 1.0)),
        ((3, 0), Note::new(0, 1.0)),
        ((0, 2), Note::new(0, 0.12)),
        ((1, 2), Note::new(0, 0.12)),
        ((2, 2), Note::new(0, 0.12)),
        ((3, 2), Note::new(0, 0.12)),
    ])
}

// ── Components ────────────────────────────────────────────────────────────────

#[derive(Component)]
struct Player;

#[derive(Component)]
struct Relic;

#[derive(Component)]
struct ZoneRing;

#[derive(Component)]
struct HudText;

// ── Resources ─────────────────────────────────────────────────────────────────

#[derive(Resource)]
struct GameState {
    relics_collected: u32,
    dist: f32,
    relic_mat: Handle<ColorMaterial>,
    ring_mats: [Handle<ColorMaterial>; 3],
    collect_sound: Handle<AudioSample>,
}

// ── Helper ────────────────────────────────────────────────────────────────────

fn section_name(intensity: f32) -> &'static str {
    match (intensity * 6.0) as u32 {
        0 => "Distant silence...",
        1 => "Low strings emerge",
        2 => "Ostinato joins",
        3 => "Brass section enters",
        4 => "Full orchestra",
        _ => "★  C L I M A X  ★",
    }
}

// ── Systems ───────────────────────────────────────────────────────────────────

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut color_mats: ResMut<Assets<ColorMaterial>>,
    asset_server: Res<AssetServer>,
) {
    commands.spawn(Camera2d);

    // Player — blue square
    commands.spawn((
        Sprite::from_color(Color::srgb(0.3, 0.55, 1.0), Vec2::splat(28.0)),
        Transform::from_xyz(-260.0, 0.0, 1.0),
        Player,
    ));

    // Relic — golden circle
    let relic_mat = color_mats.add(ColorMaterial::from(Color::srgb(1.0, 0.82, 0.0)));
    commands.spawn((
        Mesh2d(meshes.add(Circle::new(22.0))),
        MeshMaterial2d(relic_mat.clone()),
        Transform::from_xyz(220.0, 60.0, 1.0),
        Relic,
    ));

    // Zone rings — thin annuli showing proximity thresholds (360 / 240 / 120 px)
    let ring_radii = [360.0f32, 240.0, 120.0];
    let ring_colors: [Color; 3] = [
        Color::srgba(0.3, 1.0, 0.3, 0.08),
        Color::srgba(1.0, 0.9, 0.2, 0.10),
        Color::srgba(1.0, 0.3, 0.1, 0.13),
    ];
    let ring_mats: [Handle<ColorMaterial>; 3] =
        std::array::from_fn(|i| color_mats.add(ColorMaterial::from(ring_colors[i])));

    for (i, &radius) in ring_radii.iter().enumerate() {
        commands.spawn((
            Mesh2d(meshes.add(Annulus::new(radius - 1.2, radius + 1.2))),
            MeshMaterial2d(ring_mats[i].clone()),
            Transform::from_xyz(220.0, 60.0, 0.4),
            ZoneRing,
        ));
    }

    // HUD — absolute-positioned UI text
    commands.spawn((
        Text::new(""),
        TextFont { font_size: 15.0, ..default() },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            left: Val::Px(14.0),
            ..default()
        },
        HudText,
    ));

    // Game state resource
    commands.insert_resource(GameState {
        relics_collected: 0,
        dist: 999.0,
        relic_mat,
        ring_mats,
        collect_sound: asset_server.load("samples/glicol/crash.wav"),
    });

    // ── Orchestral musicians (same stack as epic_orchestra) ─────────────────
    let kick_h: Handle<AudioSample> = asset_server.load("samples/glicol/kick1.wav");
    let kick2_h: Handle<AudioSample> = asset_server.load("samples/glicol/kick2.wav");
    let snare_h: Handle<AudioSample> = asset_server.load("samples/glicol/snare1.wav");
    let crash_h: Handle<AudioSample> = asset_server.load("samples/glicol/crash.wav");
    let ride_h: Handle<AudioSample> = asset_server.load("samples/glicol/ride.wav");
    let pad_h: Handle<AudioSample> = asset_server.load("samples/glicol/pad.wav");
    let pluck_h: Handle<AudioSample> = asset_server.load("samples/glicol/pluck.wav");
    let moog_h: Handle<AudioSample> = asset_server.load("samples/glicol/moog.wav");
    let hit1_h: Handle<AudioSample> = asset_server.load("samples/glicol/hit1.wav");
    let hit2_h: Handle<AudioSample> = asset_server.load("samples/glicol/hit2.wav");
    let hit3_h: Handle<AudioSample> = asset_server.load("samples/glicol/hit3.wav");
    let sax_h: Handle<AudioSample> = asset_server.load("samples/glicol/sax.wav");

    let mut drums = SuperDrummer::new(vec![
        create_drummer_only(kick_h.clone(), 1.0, timpani_normal()),
        create_drummer_only(snare_h.clone(), 0.8, snare_orchestral()),
    ]);
    drums.auto_time_feel = true;
    drums.half_time_drums = vec![
        create_drummer_only(kick_h.clone(), 1.0, generate_half_time_kick_beat()),
        create_drummer_only(snare_h.clone(), 0.7, generate_half_time_snare_beat()),
    ];
    drums.double_time_drums = vec![
        create_drummer_only(kick2_h.clone(), 1.0, timpani_march()),
        create_drummer_only(snare_h.clone(), 0.8, generate_double_time_snare_beat()),
    ];
    let drums = drums.with_fills(4, vec![
        create_drummer_only(kick_h, 1.0, timpani_normal()),
        create_drummer_only(snare_h, 1.0, generate_snare_fill_beat()),
    ]);
    commands.spawn(Musician::new("Timpani".to_string(), drums));

    commands.spawn(Musician::new(
        "Pad".to_string(),
        OrchestraPad { sampler: Sampler { handle: pad_h, volume: -1.0 } },
    ));
    commands.spawn(Musician::new(
        "Strings".to_string(),
        OstinatoStrings { sampler: Sampler { handle: pluck_h, volume: -2.0 }, seq_idx: 0 },
    ));
    commands.spawn(Musician::new(
        "Bass".to_string(),
        OstinatoStrings { sampler: Sampler { handle: moog_h, volume: -1.0 }, seq_idx: 3 },
    ));
    commands.spawn(Musician::new(
        "Brass".to_string(),
        BrassSection { samples: vec![hit1_h, hit2_h, hit3_h], volume: -1.0 },
    ));
    commands.spawn(Musician::new(
        "Cymbals".to_string(),
        CymbalLayer { crash: crash_h, ride: ride_h },
    ));
    commands.spawn(Musician::new(
        "Lead".to_string(),
        Soloist::new(Sampler { handle: sax_h, volume: -1.0 }, 4),
    ));

    commands.insert_resource(Conductor {
        chords: epic_chords(),
        chord_length_bars: 4.0,
    });
}

fn handle_quit(keyboard: Res<ButtonInput<KeyCode>>) {
    if keyboard.just_pressed(KeyCode::Escape) {
        std::process::exit(0);
    }
}

fn move_player(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut player_q: Query<&mut Transform, With<Player>>,
    time: Res<Time>,
) {
    let mut dir = Vec2::ZERO;
    if keyboard.pressed(KeyCode::KeyW) || keyboard.pressed(KeyCode::ArrowUp) {
        dir.y += 1.0;
    }
    if keyboard.pressed(KeyCode::KeyS) || keyboard.pressed(KeyCode::ArrowDown) {
        dir.y -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyA) || keyboard.pressed(KeyCode::ArrowLeft) {
        dir.x -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyD) || keyboard.pressed(KeyCode::ArrowRight) {
        dir.x += 1.0;
    }
    if dir.length_squared() > 0.0 {
        dir = dir.normalize();
    }

    let Ok(mut t) = player_q.single_mut() else { return; };
    t.translation.x = (t.translation.x + dir.x * 220.0 * time.delta_secs()).clamp(-430.0, 430.0);
    t.translation.y = (t.translation.y + dir.y * 220.0 * time.delta_secs()).clamp(-270.0, 270.0);
}

/// The core demo system: distance → Intensity, with asymmetric smoothing.
/// Intensity rises quickly (rate 1.8×) and fades slowly (rate 0.6×) for drama.
fn update_intensity(
    player_q: Query<&Transform, With<Player>>,
    relic_q: Query<&Transform, (With<Relic>, Without<Player>)>,
    mut intensity: ResMut<Intensity>,
    mut game: ResMut<GameState>,
    time: Res<Time>,
) {
    let Ok(pt) = player_q.single() else { return; };
    let Ok(rt) = relic_q.single() else { return; };

    let dist = pt.translation.truncate().distance(rt.translation.truncate());
    game.dist = dist;

    let target = ((1.0 - dist / 480.0).max(0.0)).powf(1.5);
    let rate = if target > intensity.0 { 1.8 } else { 0.6 };
    intensity.0 = (intensity.0 + (target - intensity.0) * rate * time.delta_secs()).clamp(0.0, 1.0);
}

fn check_collection(
    player_q: Query<&Transform, With<Player>>,
    mut relic_q: Query<&mut Transform, (With<Relic>, Without<Player>)>,
    mut game: ResMut<GameState>,
    mut commands: Commands,
) {
    let Ok(pt) = player_q.single() else { return; };
    let Ok(mut rt) = relic_q.single_mut() else { return; };

    if pt.translation.truncate().distance(rt.translation.truncate()) < 42.0 {
        game.relics_collected += 1;
        let n = game.relics_collected as f32;
        // Golden-angle spiral so relics spread evenly around the arena
        let angle = n * std::f32::consts::TAU * 0.618;
        let radius = 90.0 + (n * 70.0) % 270.0;
        rt.translation.x = (angle.cos() * radius).clamp(-400.0, 400.0);
        rt.translation.y = (angle.sin() * radius).clamp(-240.0, 240.0);

        commands.spawn((
            SamplePlayer::new(game.collect_sound.clone())
                .with_volume(Volume::Decibels(-2.0)),
            PlaybackSettings::default(),
        ));
    }
}

/// Keep zone rings centred on the relic each frame.
fn sync_ring_positions(
    relic_q: Query<&Transform, With<Relic>>,
    mut ring_q: Query<&mut Transform, (With<ZoneRing>, Without<Relic>)>,
) {
    let Ok(rt) = relic_q.single() else { return; };
    let pos = rt.translation;
    for mut ring_t in &mut ring_q {
        ring_t.translation.x = pos.x;
        ring_t.translation.y = pos.y;
    }
}

fn update_visuals(
    game: Res<GameState>,
    intensity: Res<Intensity>,
    mut color_mats: ResMut<Assets<ColorMaterial>>,
    mut relic_q: Query<&mut Transform, With<Relic>>,
    mut bg: ResMut<ClearColor>,
) {
    let i = intensity.0;
    let dist = game.dist;

    // Relic pulses in size with intensity
    if let Ok(mut t) = relic_q.single_mut() {
        t.scale = Vec3::splat(1.0 + i * 0.65);
    }

    // Relic colour: dim amber → bright gold-white
    if let Some(mat) = color_mats.get_mut(&game.relic_mat) {
        mat.color = Color::srgb(1.0, 0.65 + i * 0.30, i * 0.35);
    }

    // Zone ring opacity rises as player enters each zone
    let zone_radii = [360.0f32, 240.0, 120.0];
    let zone_rgb: [[f32; 3]; 3] = [
        [0.3, 1.0, 0.3],
        [1.0, 0.85, 0.2],
        [1.0, 0.3, 0.1],
    ];
    for (idx, mat_h) in game.ring_mats.iter().enumerate() {
        let alpha = if dist < zone_radii[idx] {
            0.18 + 0.22 * i
        } else {
            0.05
        };
        if let Some(mat) = color_mats.get_mut(mat_h) {
            let [r, g, b] = zone_rgb[idx];
            mat.color = Color::srgba(r, g, b, alpha);
        }
    }

    // Background: dark navy → deep crimson as intensity rises
    bg.0 = Color::srgb(i * 0.18, 0.01, 0.06 + (1.0 - i) * 0.12);
}

fn update_hud(
    mut hud_q: Query<&mut Text, With<HudText>>,
    game: Res<GameState>,
    intensity: Res<Intensity>,
) {
    let Ok(mut text) = hud_q.single_mut() else { return; };
    let section = section_name(intensity.0);
    **text = format!(
        "Distance: {:.0} px    Intensity: {:.2}    Relics collected: {}\nSection: {}\n[WASD] Move    [Esc] Quit",
        game.dist, intensity.0, game.relics_collected, section
    );
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Relic Hunter — RustyMusic Integration Demo".into(),
                    resolution: (900u32, 600u32).into(),
                    ..default()
                }),
                ..default()
            }),
        )
        .add_plugins(MusicPlugin { beats: 4, note_type: 4, bpm: 88.0 })
        .insert_resource(ClearColor(Color::srgb(0.0, 0.01, 0.18)))
        .insert_resource(Intensity(0.0))
        .add_systems(Startup, setup)
        .add_systems(Update, handle_quit)
        .add_systems(
            Update,
            (move_player, update_intensity, check_collection,
             sync_ring_positions, update_visuals, update_hud).chain(),
        )
        .run();
}
