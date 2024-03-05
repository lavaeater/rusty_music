use bevy::prelude::{Event, EventWriter, Res, ResMut, Resource, Time};

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
            // self.accumulator -= self.beat_length;
            self.accumulator = 0.0;
            self.beat += 1;
            if self.beat >= self.beats as u32 * 4 {
                self.beat = 0;
                self.bar += 1;
            }

            return true;
        }

        false
    }
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
