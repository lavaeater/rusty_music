//! Situational awareness: the game describes *how things are*; the music follows.
//!
//! - [`MusicState`]: the game writes a target [`Mood`] (intensity, tension,
//!   valence, energy) whenever it likes; the music glides towards it.
//! - [`Layer`]: a musician that enters and leaves with a mood dimension, at bar lines.
//! - [`TempoFollows`]: tempo driven by a dimension.
//! - [`Harmony`](crate::harmony::Harmony): mode follows valence, chord colour
//!   follows tension, rebuilt at each phrase start.
//! - [`Stinger`]: a musical gesture for a game event, quantised to the next
//!   step, beat or bar.
//!
//! Add [`SituationPlugin`] after [`MusicPlugin`](crate::MusicPlugin). It
//! drives the [`Intensity`] resource from `MusicState`, so existing musicians
//! respond without changes.

use std::sync::Arc;

use bevy::prelude::*;

use crate::clock::{Beat, Clock, progress_clock_system};
use crate::harmony::{Harmony, Mode};
use crate::instrument::NoteEvent;
use crate::musicians::conductor::{Conductor, UpcomingChords};
use crate::musicians::{Chord, Muted};
use crate::player::{play_sound_on_the_beat, Intensity};

/// One axis of the mood.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dim {
    /// How much is going on: density, layers. Drives the classic `Intensity`.
    Intensity,
    /// Danger, suspense: brighter filters, more dissonant chord colours.
    Tension,
    /// Dark (0) … bright (1): picks the musical mode.
    Valence,
    /// Calm (0) … frantic (1): typically drives tempo.
    Energy,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mood {
    pub intensity: f32,
    pub tension: f32,
    pub valence: f32,
    pub energy: f32,
}

impl Default for Mood {
    fn default() -> Self {
        Self { intensity: 0.3, tension: 0.2, valence: 0.4, energy: 0.4 }
    }
}

impl Mood {
    pub fn get(&self, dim: Dim) -> f32 {
        match dim {
            Dim::Intensity => self.intensity,
            Dim::Tension => self.tension,
            Dim::Valence => self.valence,
            Dim::Energy => self.energy,
        }
    }

    pub fn set(&mut self, dim: Dim, value: f32) {
        let value = value.clamp(0.0, 1.0);
        match dim {
            Dim::Intensity => self.intensity = value,
            Dim::Tension => self.tension = value,
            Dim::Valence => self.valence = value,
            Dim::Energy => self.energy = value,
        }
    }

    /// Move every dimension towards `target` by fraction `k` (0–1).
    fn approach(&mut self, target: &Mood, k: f32) {
        for dim in [Dim::Intensity, Dim::Tension, Dim::Valence, Dim::Energy] {
            let now = self.get(dim);
            self.set(dim, now + (target.get(dim) - now) * k);
        }
    }
}

/// The game's view of the situation. Write `target`; read `current`.
#[derive(Debug, Clone, Resource)]
pub struct MusicState {
    pub target: Mood,
    current: Mood,
    /// Seconds for the music to (mostly) catch up with a change in `target`.
    pub response: f32,
}

impl Default for MusicState {
    fn default() -> Self {
        Self { target: Mood::default(), current: Mood::default(), response: 2.0 }
    }
}

impl MusicState {
    /// The smoothed mood the music is playing right now.
    pub fn current(&self) -> Mood {
        self.current
    }

    pub fn with_response(self, seconds: f32) -> Self {
        Self { response: seconds, ..self }
    }

    /// Set a dimension instantly, skipping the glide: a hit, an explosion.
    pub fn jolt(&mut self, dim: Dim, value: f32) {
        self.target.set(dim, value);
        self.current.set(dim, value);
    }

    /// Jump straight to the target (scene changes, loading a save).
    pub fn snap(&mut self) {
        self.current = self.target;
    }

    fn update(&mut self, dt: f32) {
        let k = if self.response <= 0.0 { 1.0 } else { 1.0 - (-3.0 * dt / self.response).exp() };
        let target = self.target;
        self.current.approach(&target, k);
    }
}

/// A musician that plays only while `dim` is high enough: vertical layering.
/// It enters when `dim` rises to `enter` and leaves when it falls below
/// `exit` (keep `exit` < `enter`, so it doesn't flicker), always at a bar line.
#[derive(Debug, Clone, Copy, Component)]
pub struct Layer {
    pub dim: Dim,
    pub enter: f32,
    pub exit: f32,
}

impl Layer {
    /// Enter at `threshold`, leave a little below it.
    pub fn above(dim: Dim, threshold: f32) -> Self {
        Self { dim, enter: threshold, exit: threshold - 0.08 }
    }

    /// Whether the layer should be playing, given whether it is now.
    pub fn wants_to_play(&self, value: f32, playing: bool) -> bool {
        if playing { value >= self.exit } else { value >= self.enter }
    }
}

/// Tempo follows a mood dimension: `slow` bpm at 0, `fast` bpm at 1.
#[derive(Debug, Clone, Copy, Resource)]
pub struct TempoFollows {
    pub dim: Dim,
    pub slow: f32,
    pub fast: f32,
}

// ── Stingers ─────────────────────────────────────────────────────────────────

/// When a stinger may start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quantize {
    /// The next 16th step: as soon as possible, still in time.
    Step,
    /// The next beat.
    Beat,
    /// The next downbeat.
    Bar,
}

impl Quantize {
    pub fn matches(self, beat: &Beat) -> bool {
        match self {
            Quantize::Step => true,
            Quantize::Beat => beat.sixteenth == 0,
            Quantize::Bar => beat.sixteenth == 0 && beat.beat == 0,
        }
    }
}

/// Builds a stinger's notes from the chord playing when it starts.
pub type GestureFn = Arc<dyn Fn(&Chord) -> Vec<NoteEvent> + Send + Sync>;

/// What a stinger plays. Offsets/lengths are in steps from its start.
#[derive(Clone)]
pub enum Gesture {
    Notes(Vec<NoteEvent>),
    FromChord(GestureFn),
}

impl Gesture {
    pub fn custom(f: impl Fn(&Chord) -> Vec<NoteEvent> + Send + Sync + 'static) -> Self {
        Gesture::FromChord(Arc::new(f))
    }

    /// The chord's main tones, all at once, held for `length` steps.
    pub fn chord_stab(length: f32, velocity: f32) -> Self {
        Self::custom(move |chord| {
            chord.chord_notes.iter().filter(|n| n.strength >= 0.6)
                .map(|n| NoteEvent::from_diff(n.midi_note_diff).velocity(velocity).length(length))
                .collect()
        })
    }

    /// The chord's triad climbing for `count` notes, one every `every` steps.
    pub fn arpeggio_up(count: usize, every: f32, velocity: f32) -> Self {
        Self::custom(move |chord| {
            let triad: Vec<i32> = chord.chord_notes.iter().take(3).map(|n| n.midi_note_diff).collect();
            (0..count)
                .map(|k| {
                    let pitch = triad[k % triad.len()] + 12 * (k / triad.len()) as i32;
                    NoteEvent::from_diff(pitch).offset(k as f32 * every).length(every * 1.5).velocity(velocity)
                })
                .collect()
        })
    }

    /// A semitone cluster on the chord root: an alarm, a hit, something wrong.
    pub fn dissonant_hit(length: f32) -> Self {
        Self::custom(move |chord| {
            let root = chord.chord_notes[0].midi_note_diff;
            [0, 1, 6].iter().map(|d| NoteEvent::from_diff(root + d).length(length)).collect()
        })
    }

    pub fn notes_for(&self, chord: &Chord) -> Vec<NoteEvent> {
        match self {
            Gesture::Notes(notes) => notes.clone(),
            Gesture::FromChord(f) => f(chord),
        }
    }
}

/// Send this to play a gesture on `musician`'s instrument at the next
/// `quantize` point. The musician may be [`Muted`] (a dedicated stinger
/// instrument usually is): stingers play anyway.
#[derive(Clone, Message)]
pub struct Stinger {
    pub musician: Entity,
    pub gesture: Gesture,
    pub quantize: Quantize,
}

// ── Plugin ───────────────────────────────────────────────────────────────────

pub struct SituationPlugin;

impl Plugin for SituationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MusicState>().init_resource::<Harmony>();
        if !app.world().contains_resource::<Conductor>() {
            let harmony = app.world().resource::<Harmony>();
            let conductor = Conductor { chords: harmony.chords(0.0), chord_length_bars: harmony.phrase_bars() };
            app.insert_resource(conductor);
        }
        app
            .add_systems(Update, (smooth_mood, follow_tempo).chain().before(progress_clock_system))
            .add_systems(
                Update,
                (conduct, update_layers).after(progress_clock_system).before(play_sound_on_the_beat),
            );
    }
}

fn smooth_mood(time: Res<Time>, mut state: ResMut<MusicState>, mut intensity: ResMut<Intensity>) {
    state.update(time.delta_secs());
    intensity.0 = state.current().intensity;
}

fn follow_tempo(tempo: Option<Res<TempoFollows>>, state: Res<MusicState>, mut clock: ResMut<Clock>) {
    let Some(tempo) = tempo else { return };
    let bpm = tempo.slow + (tempo.fast - tempo.slow) * state.current().get(tempo.dim);
    if (clock.tempo_bpm - bpm).abs() > 0.05 {
        clock.set_bpm(bpm);
    }
}

/// At each phrase start, re-pick the mode from valence and rebuild the chords.
/// At each phrase start, re-pick the mode from valence and schedule new
/// chords from exactly that bar.
fn conduct(
    mut commands: Commands,
    mut beats: MessageReader<Beat>,
    mut harmony: ResMut<Harmony>,
    state: Res<MusicState>,
    mut conductor: ResMut<Conductor>,
    mut built: Local<bool>,
) {
    let mood = state.current();
    let phrase = harmony.phrase_bars().max(1.0) as u32;
    let phrase_start = beats
        .read()
        .find(|b| b.beat == 0 && b.sixteenth == 0 && b.bar_count.is_multiple_of(phrase))
        .map(|b| b.time_bars);
    if *built && phrase_start.is_none() {
        return;
    }
    if harmony.mode_follows_valence {
        harmony.mode = Mode::from_valence(mood.valence);
    }
    let next = Conductor { chords: harmony.chords(mood.tension), chord_length_bars: harmony.phrase_bars() };
    match phrase_start {
        Some(from_bars) if *built => commands.insert_resource(UpcomingChords { from_bars, conductor: next }),
        _ => *conductor = next,
    }
    *built = true;
}

fn update_layers(
    mut commands: Commands,
    mut beats: MessageReader<Beat>,
    state: Res<MusicState>,
    layers: Query<(Entity, &Layer, Has<Muted>)>,
) {
    if !beats.read().any(|b| b.beat == 0 && b.sixteenth == 0) {
        return;
    }
    let mood = state.current();
    for (entity, layer, muted) in &layers {
        let wants = layer.wants_to_play(mood.get(layer.dim), !muted);
        if wants && muted {
            commands.entity(entity).remove::<Muted>();
        } else if !wants && !muted {
            commands.entity(entity).insert(Muted);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mood_glides_towards_target() {
        let mut state = MusicState { response: 1.0, ..default() };
        state.target.tension = 1.0;
        let start = state.current().tension;
        state.update(0.1);
        let early = state.current().tension;
        assert!(early > start && early < 0.5, "moves a little at first: {early}");
        for _ in 0..30 {
            state.update(0.1);
        }
        assert!(state.current().tension > 0.95, "arrives after ~response");
    }

    #[test]
    fn layer_hysteresis() {
        let layer = Layer::above(Dim::Intensity, 0.6);
        assert!(!layer.wants_to_play(0.58, false), "not yet in");
        assert!(layer.wants_to_play(0.6, false), "enters at threshold");
        assert!(layer.wants_to_play(0.55, true), "stays while just below");
        assert!(!layer.wants_to_play(0.5, true), "leaves well below");
    }

    #[test]
    fn quantize_points() {
        let beat = |beat, sixteenth| Beat { beat, sixteenth, ..default() };
        assert!(Quantize::Step.matches(&beat(1, 3)));
        assert!(Quantize::Beat.matches(&beat(2, 0)) && !Quantize::Beat.matches(&beat(2, 1)));
        assert!(Quantize::Bar.matches(&beat(0, 0)) && !Quantize::Bar.matches(&beat(1, 0)));
    }

    #[test]
    fn gestures_follow_the_chord() {
        let chord = Harmony::default().chords(0.0).remove(0); // A minor
        let arp = Gesture::arpeggio_up(4, 1.0, 1.0).notes_for(&chord);
        let pitches: Vec<f32> = arp.iter().map(|n| n.pitch - crate::instrument::SAMPLE_ROOT).collect();
        assert_eq!(pitches, vec![-3.0, 0.0, 4.0, 9.0]);
        assert_eq!(arp[3].offset, 3.0);
        assert!(Gesture::chord_stab(4.0, 1.0).notes_for(&chord).len() >= 3);
    }
}
