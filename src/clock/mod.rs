use bevy::prelude::{Event, EventWriter, Res, ResMut, Resource, Time};

#[derive(Debug, Clone, Copy, Resource)]
pub struct Clock {
    pub beats: f32,
    // beats per measure
    pub tempo_bpm: f32,
    // beats per minute, aka tempo
    pub playing: bool,
    pub accumulator: f32,
    pub next_beat: f32,
    pub elapsed_time: f32,
    pub beat_count: u32,
    pub beat_length: f32
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
        
        if self.accumulator + self.elapsed_time > self.next_beat {
            self.elapsed_time += self.beat_length;
            self.accumulator = 0.0;
            self.next_beat = self.elapsed_time + self.beat_length;
            return true;
        }

        false
    }
}

impl Clock {
    pub fn new(beats: f32, bpm: f32) -> Self {
        Self {
            beats,
            tempo_bpm: bpm,
            playing: true,
            accumulator: 0.0,
            beat_count: 0,
            elapsed_time: 0.0,
            next_beat: 60.0 / bpm / beats,
            beat_length: 60.0 / bpm / beats
        }
    }

    pub fn get_beat(&self) -> u32 {
        let beat = self.elapsed_time * self.tempo_bpm;
        (beat / 60.0) as u32 // what beat are we on, bro?
    }

    pub fn get_exact_notes(&self, factor: f32) -> u32 {
        let beat = self.elapsed_time * self.tempo_bpm * factor;
        (beat / 60.0).floor() as u32 // what beat are we on, bro?
    }
}

#[derive(Debug, Clone, Copy, Event)]
pub struct Beat {
    pub elapsed_time: f32,
    pub quarter: u32,
    pub eigth: u32,
    pub sixteenth: u32,
}

pub fn progress_clock_system(
    mut clock: ResMut<Clock>, time: Res<Time>,
    mut beat_sender: EventWriter<Beat>,
) {
    if clock.progress(time.delta_seconds()) {
        beat_sender.send(Beat {
            elapsed_time: clock.elapsed_time,
            quarter: clock.get_beat(),
            eigth: clock.get_exact_notes(2.0),
            sixteenth: clock.get_exact_notes(4.0),
        });
    }
}
