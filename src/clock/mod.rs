use bevy::asset::Handle;
use bevy::prelude::{Component, Event, EventWriter, Res, ResMut, Resource, Time};
use bevy::utils::HashMap;
use bevy_kira_audio::{Audio, AudioControl, AudioSource};

#[derive(Debug, Clone, Copy, Resource)]
pub struct Clock {
    pub beats: f32,
    pub note_type: f32,
    // beats per measure
    pub tempo_bpm: f32,
    // beats per minute, aka tempo
    pub playing: bool,
    pub accumulator: f32,
    pub beat_length: f32,
    pub elapsed_time: f32,
    pub beat: u32,
    pub bar: u32,
}

trait ProgressClock {
    fn progress(&mut self, delta: f32) -> bool;
}

impl ProgressClock for Clock {
    fn progress(&mut self, delta: f32) -> bool {
        if !self.playing {
            return false;
        }
        self.accumulator += delta;
        
        if self.accumulator >= self.beat_length {
            self.accumulator -= self.beat_length;
            self.beat += 1;
            if self.beat >= self.beats as u32 {
                self.beat = 0;
                self.bar += 1;
            }

            return true;
        }

        false
    }
    //
    // fn get_exact_notes(&self, factor: f32) -> u32 {
    //     let beat = self.elapsed_time * self.tempo_bpm * factor;
    //     (beat / 60.0).floor() as u32 // what beat are we on, bro?
    // }
}

impl Clock {
    pub fn new(beats: u32, note_type: u32, bpm: f32) -> Self {
        Self {
            beats: beats as f32,
            note_type: note_type as f32,
            tempo_bpm: bpm,
            playing: true,
            accumulator: 0.0,
            elapsed_time: 0.0,
            beat_length: (60.0 / bpm / beats as f32) / (beats as f32 / note_type as f32),
            beat: 0,
            bar: 0,
        }
    }


}

#[derive(Debug, Clone, Copy, Event)]
pub struct Beat {
    pub elapsed_time: f32,
    pub beat: u32,
    pub bar: u32,
}

// signal(beat: Int, thisNoteIndex: Int, timeBars: Float, hitTime: Float, baseIntensity: Float)
pub trait MusicPlayer: Send + Sync {
    fn signal(&self, audio: &Res<Audio>, beat: Beat, global_intensity: f32);
    fn set_chord(&self, chord: Chord);
    fn play(&self, audio: &Res<Audio>, beat: Beat, midi_note_diff: i32);
}

fn midi_diff_to_pitch(midi_diff: i32) -> f64 {
    let min_pitch = -12;
    let max_pitch = 12;
    if midi_diff < 0 {
        if midi_diff < min_pitch {
            0.5
        } else {
            midi_diff_to_pitch_what(midi_diff)
        }
    } else if midi_diff > 0 {
        if midi_diff > max_pitch {
            2.0
        } else {
            midi_diff_to_pitch_what(midi_diff)
        }
    } else {
        1.0
    }
}

fn midi_diff_to_pitch_what(midi_diff: i32) -> f64 {
    let f = 2.0f64.powf(midi_diff as f64 / 12.0);
    f
}

// fn to_pitch(midi_diff:i32)-> f32 {
// let minPitch = -12;
// let  maxPitch = 12;
// /**
//  * Hmm. So, -12 is 0.5f in pitch,
//  * + 12 is 2.0f
//  *
//  * 0 is 1f
//  *
//  *
//  */
// if (midi_diff < 0) {
// if (midi_diff < minPitch) {
//     0.5f
// } else
// this.fromMidiToPitch()
// //1f - (1f / (maxPitch * 2 / this.absoluteValue.toFloat()))
// } else if (this > 0) {
// if (this > maxPitch)
// 2f
// else {
// this.fromMidiToPitch()
// //            1f + norm(0f, 12f, this.toFloat())
// }
// } else {
// 1f
// }
// }

// fromMidiToPitch(): Float {
// /**
//  * The midi reference note is apparently 69, not 60...
//  */
//
// val f = 2f.pow(this.toFloat() / 12f) //This should give us a factor, right?
// return f
// }


pub struct Sampler {
    pub handle: Handle<AudioSource>
}

pub struct Drummer {
    pub name: String,
    pub sampler: Sampler,
    pub notes: HashMap<u32, Note>
}

impl MusicPlayer for Drummer {
    fn signal(&self, audio: &Res<Audio>, beat: Beat, base_intensity: f32) {
        if let Some(note) = self.notes.get(&beat.beat) {
            let minIntensity = 1.0 - base_intensity;
            if note.strength >= minIntensity {
                self.play(audio, beat, note.midi_note_diff);
            }
        }
    }

    fn set_chord(&self, chord: Chord) {
        //We don't do diddly if we are a drummer here.
    }

    fn play(&self, audio: &Res<Audio>, beat: Beat, midi_note_diff: i32) {
        audio.play(self.sampler.handle.clone_weak())
            .with_playback_rate(midi_diff_to_pitch(midi_note_diff))
        ;
    }
}


#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    pub midi_note_diff: i32,
    pub strength: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chord {
    pub bar: u32,
    pub chord_notes: Vec<Note>,
    pub scale_notes: Vec<Note>
}

#[derive(Resource)]
pub struct Conductor {
    pub musicians: Vec<Box<dyn MusicPlayer>>

}

pub fn progress_clock_system(
    mut clock: ResMut<Clock>, time: Res<Time>,
    mut beat_sender: EventWriter<Beat>,
) {
    if clock.progress(time.delta_seconds()) {
        beat_sender.send(Beat {
            elapsed_time: clock.elapsed_time,
            beat: clock.beat,
            bar: clock.bar,
        });
    }
}
