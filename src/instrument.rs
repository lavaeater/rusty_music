//! Notes and instruments: musicians decide *what* to play as [`NoteEvent`]s;
//! an [`Instrument`] decides *how it sounds* and schedules it on the audio clock.
//!
//! Swapping a musician's instrument (sample → multisample → synth) never
//! touches the musician's logic.

use bevy::prelude::{Commands, Entity, Time};
use bevy_seedling::node::events::AudioEvents;
use bevy_seedling::prelude::{
    Audio, AudioContext, FirewheelNode, InstantSeconds, PlaybackSettings, SamplePlayer, Volume,
};

use crate::musicians::{midi_diff_to_pitch, Sampler};
use crate::sampler::{Dynamic, MultiSampler};

/// The MIDI note a single [`Sampler`] sounds at when played unshifted.
/// Musicians working in `midi_note_diff` offsets emit `SAMPLE_ROOT + diff`.
pub const SAMPLE_ROOT: f32 = 60.0;

/// One note, as a musician asks for it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteEvent {
    /// Absolute MIDI pitch (fractional allowed for synths).
    pub pitch: f32,
    /// 0–1. Synths scale amplitude by it; multisamplers pick a dynamic layer.
    pub velocity: f32,
    /// Length in steps (16ths in 4/4). One-shot samples ignore it.
    pub length: f32,
    /// Start offset in steps from the beat that produced the note.
    pub offset: f32,
    /// Extra gain in decibels on top of the instrument's own level.
    pub gain_db: f32,
    /// Playing style for instruments that have several (e.g. `"pizz"`, `"sustain"`).
    pub articulation: &'static str,
    /// Which sub-instrument of a [`Channels`] instrument plays it (drum kits, doublings).
    pub channel: usize,
}

impl NoteEvent {
    pub fn new(pitch: f32) -> Self {
        Self { pitch, velocity: 1.0, length: 1.0, offset: 0.0, gain_db: 0.0, articulation: "", channel: 0 }
    }

    /// A note `midi_note_diff` semitones from [`SAMPLE_ROOT`].
    pub fn from_diff(midi_note_diff: i32) -> Self {
        Self::new(SAMPLE_ROOT + midi_note_diff as f32)
    }

    pub fn velocity(self, velocity: f32) -> Self {
        Self { velocity, ..self }
    }

    pub fn length(self, length: f32) -> Self {
        Self { length, ..self }
    }

    pub fn offset(self, offset: f32) -> Self {
        Self { offset, ..self }
    }

    pub fn gain_db(self, gain_db: f32) -> Self {
        Self { gain_db, ..self }
    }

    pub fn articulation(self, articulation: &'static str) -> Self {
        Self { articulation, ..self }
    }

    pub fn channel(self, channel: usize) -> Self {
        Self { channel, ..self }
    }
}

/// Everything an instrument may need to make a sound.
pub struct PlayContext<'a, 'w, 's> {
    pub commands: &'a mut Commands<'w, 's>,
    pub audio: &'a mut AudioContext,
    pub time: &'a Time<Audio>,
    /// The Firewheel node on the musician's entity, if it has one (synths).
    pub node: Option<&'a FirewheelNode>,
    /// Look up the Firewheel node of any entity, for instruments that play a
    /// node elsewhere (e.g. one synth per drum in a kit).
    pub find_node: &'a dyn Fn(Entity) -> Option<FirewheelNode>,
    /// The musician's mix section, if any: samples play through its pool.
    pub section: Option<crate::mix::Section>,
}

pub trait Instrument: Send + Sync {
    /// Sound `note` starting at audio time `at`, lasting `length` seconds.
    fn play(&mut self, note: &NoteEvent, at: f64, length: f64, cx: &mut PlayContext);
}

/// Spawn a one-shot sample that starts exactly at `at`.
pub fn schedule_sample(cx: &mut PlayContext, player: SamplePlayer, speed: f64, at: f64) {
    let mut events = AudioEvents::new(cx.time);
    let settings = PlaybackSettings::default().with_playback(false).with_speed(speed);
    settings.play_at(None, InstantSeconds(at), &mut events);
    let mut sample = cx.commands.spawn((events, settings, player));
    if let Some(section) = cx.section {
        sample.insert(crate::mix::SectionPool(section));
    }
}

impl Instrument for Sampler {
    fn play(&mut self, note: &NoteEvent, at: f64, _length: f64, cx: &mut PlayContext) {
        let player = SamplePlayer::new(self.handle.clone())
            .with_volume(Volume::Decibels(self.volume as f32 + note.gain_db));
        let speed = midi_diff_to_pitch((note.pitch - SAMPLE_ROOT).round() as i32);
        schedule_sample(cx, player, speed, at);
    }
}

impl Instrument for MultiSampler {
    fn play(&mut self, note: &NoteEvent, at: f64, _length: f64, cx: &mut PlayContext) {
        let dynamic = Dynamic::for_intensity(note.velocity);
        if let Some((handle, residual)) = self.pick(note.pitch.round() as i32, note.articulation, dynamic) {
            let player = SamplePlayer::new(handle).with_volume(Volume::Decibels(self.volume_db + note.gain_db));
            schedule_sample(cx, player, midi_diff_to_pitch(residual), at);
        }
    }
}

/// Several instruments addressed by [`NoteEvent::channel`]: a drum kit, or a
/// melody doubled by violins (channel 0) and flute (channel 1).
#[derive(Default)]
pub struct Channels(pub Vec<Box<dyn Instrument>>);

impl Channels {
    pub fn with(mut self, instrument: impl Instrument + 'static) -> Self {
        self.0.push(Box::new(instrument));
        self
    }
}

impl Instrument for Channels {
    fn play(&mut self, note: &NoteEvent, at: f64, length: f64, cx: &mut PlayContext) {
        if let Some(instrument) = self.0.get_mut(note.channel) {
            instrument.play(note, at, length, cx);
        }
    }
}

/// Plays nothing: the default for musicians that bring no instrument.
pub struct Silent;

impl Instrument for Silent {
    fn play(&mut self, _note: &NoteEvent, _at: f64, _length: f64, _cx: &mut PlayContext) {}
}
