use bevy::audio::AudioSink;
use bevy::input::ButtonInput;
use bevy::prelude::{AudioSinkPlayback, Event, EventReader, EventWriter, KeyCode, Query, Res, ResMut, Resource, Time};
#[derive(Debug, Clone, Copy, Resource)]
pub struct Clock {
    pub bpm: f32, // quarter notes per minute
    pub playing: bool,
    pub cooldown: f32,
    pub sixty_seconds: f32,
    pub beat_count: u32,
}

trait Cooldown {
    fn cooldown(&mut self, delta: f32) -> bool;
}

impl Cooldown for Clock {
    fn cooldown(&mut self, delta: f32) -> bool {
        if !self.playing {
            return false;
        }
        self.sixty_seconds += delta;
        self.cooldown -= delta;

        if self.sixty_seconds >= 60.0 {
            self.sixty_seconds = 0.0;
        }

        if self.cooldown < 0.0 {
            self.cooldown = 60.0 / self.bpm;
            return true;
        }

        false
    }
}

impl Clock {
    pub fn new(bpm: f32) -> Self {
        Self {
            bpm,
            playing: true,
            cooldown: 60.0 / bpm,
            beat_count: 0,
            sixty_seconds: 0.0,
        }
    }

    pub fn get_beat(&self) -> u32 {
        let beat = self.sixty_seconds * self.bpm ;
        (beat / 60.0) as u32 // what beat are we on, bro?
    }
}

#[derive(Debug, Clone, Copy, Event)]
pub struct Beat {
    clock_time: f32,
    beat_count: u32,
}

pub fn beat_system(
    mut clock: ResMut<Clock>, time: Res<Time>,
    mut beat_sender: EventWriter<Beat>
) {
    if clock.cooldown(time.delta_seconds()) {
        beat_sender.send(Beat {
            clock_time: clock.sixty_seconds,
            beat_count: clock.get_beat(),
        });
    }
}

pub fn play_sound_on_the_beat(
    mut beat_reader: EventReader<Beat>,
) {
    for beat in beat_reader.read() {
        println!("Beat: {}", beat.beat_count);
        if beat.beat_count % 4 == 0 {
            println!("Bass Drum");
        } else {
            println!("Snare Drum");
        }
    }
}

// fn interactive_audio(input: Res<ButtonInput<KeyCode>>, mut query: Query<(&mut AudioSink, &Dsp)>) {
//     if input.just_pressed(KeyCode::KeyS) {
//         for (sink, _) in query.iter_mut().filter(|(_s, d)| **d == Dsp::Sine) {
//             sink.toggle();
//         }
//     }
//
//     if input.just_pressed(KeyCode::KeyT) {
//         for (sink, _) in query.iter_mut().filter(|(_s, d)| **d == Dsp::Triangle) {
//             sink.toggle();
//         }
//     }
// }
