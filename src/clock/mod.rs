use bevy::prelude::{Message, MessageWriter, Res, ResMut, Resource, Time};

#[derive(Debug, Clone, Copy, Resource)]
pub struct Clock {
    pub beats: f32,
    pub note_type: f32,
    pub tempo_bpm: f32,
    pub playing: bool,
    pub accumulator: f32,
    pub beat_length: f32,
    pub elapsed_time: f32,
    pub sixteenth: u32,
    pub beat: u32,
    pub bar_count: u32,
    pub beat_count: u32,
    pub sixteenth_count: u32,
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
            self.accumulator -= self.beat_length; // preserve overshoot to avoid drift
            self.sixteenth += 1;
            self.sixteenth_count += 1;
            if self.sixteenth >= self.beats as u32 {
                self.sixteenth = 0;
                self.beat += 1;
                self.beat_count += 1;
            }

            if self.beat >= self.beats as u32 {
                self.beat = 0;
                self.bar_count += 1;
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
            sixteenth: 0,
            bar_count: 0,
            beat: 0,
            beat_count: 0,
            sixteenth_count: 0,
        }
    }

    /// Fractional bar position: `bar_count + beat/beats + sixteenth/beats²`.
    pub fn time_bars(&self) -> f32 {
        self.bar_count as f32
            + self.beat as f32 / self.beats
            + self.sixteenth as f32 / (self.beats * self.beats)
    }
}

#[derive(Debug, Clone, Copy, Message)]
pub struct Beat {
    pub elapsed_time: f32,
    pub beat: u32,
    pub sixteenth: u32,
    pub bar_count: u32,
    pub beat_count: u32,
    pub sixteenth_count: u32,
    /// Fractional bar position at the moment this beat fired.
    pub time_bars: f32,
}

impl Beat {
    pub fn new(
        elapsed_time: f32,
        beat: u32,
        sixteenth: u32,
        bar_count: u32,
        beat_count: u32,
        sixteenth_count: u32,
        time_bars: f32,
    ) -> Self {
        Self {
            elapsed_time,
            beat,
            sixteenth,
            bar_count,
            beat_count,
            sixteenth_count,
            time_bars,
        }
    }
}

pub fn progress_clock_system(
    mut clock: ResMut<Clock>,
    time: Res<Time>,
    mut beat_sender: MessageWriter<Beat>,
) {
    if clock.progress(time.delta_secs()) {
        let time_bars = clock.time_bars();
        beat_sender.write(Beat::new(
            clock.elapsed_time,
            clock.beat,
            clock.sixteenth,
            clock.bar_count,
            clock.beat_count,
            clock.sixteenth_count,
            time_bars,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_clock() -> Clock {
        Clock::new(4, 4, 120.0) // 120 bpm, 4/4 → beat_length = 0.125s per 16th
    }

    #[test]
    fn clock_does_not_fire_before_beat_length() {
        let mut clock = make_clock();
        assert!(!clock.progress(0.1));
    }

    #[test]
    fn clock_fires_at_beat_length() {
        let mut clock = make_clock();
        assert!(clock.progress(clock.beat_length));
    }

    #[test]
    fn accumulator_preserves_overshoot() {
        let mut clock = make_clock();
        let overshoot = 0.005;
        clock.progress(clock.beat_length + overshoot);
        assert!(
            (clock.accumulator - overshoot).abs() < 1e-5,
            "expected ~{overshoot}, got {}",
            clock.accumulator
        );
    }

    #[test]
    fn beat_advances_after_four_sixteenths() {
        let mut clock = make_clock();
        let bl = clock.beat_length;
        for _ in 0..4 {
            clock.progress(bl);
        }
        assert_eq!(clock.beat, 1);
        assert_eq!(clock.sixteenth, 0);
    }

    #[test]
    fn bar_advances_after_sixteen_sixteenths() {
        let mut clock = make_clock();
        let bl = clock.beat_length;
        for _ in 0..16 {
            clock.progress(bl);
        }
        assert_eq!(clock.bar_count, 1);
        assert_eq!(clock.beat, 0);
        assert_eq!(clock.sixteenth, 0);
    }

    #[test]
    fn time_bars_at_start_is_zero() {
        let clock = make_clock();
        assert_eq!(clock.time_bars(), 0.0);
    }

    #[test]
    fn time_bars_after_one_sixteenth() {
        let mut clock = make_clock();
        clock.progress(clock.beat_length);
        // bar=0, beat=0, sixteenth=1 → 0 + 0 + 1/16 = 0.0625
        assert!((clock.time_bars() - 0.0625).abs() < 1e-5);
    }

    #[test]
    fn time_bars_after_one_beat() {
        let mut clock = make_clock();
        let bl = clock.beat_length;
        for _ in 0..4 {
            clock.progress(bl);
        }
        // bar=0, beat=1, sixteenth=0 → 0.25
        assert!((clock.time_bars() - 0.25).abs() < 1e-5);
    }

    #[test]
    fn sixteenth_count_is_monotonically_increasing() {
        let mut clock = make_clock();
        let bl = clock.beat_length;
        let mut last = 0u32;
        for _ in 0..20 {
            clock.progress(bl);
            assert!(clock.sixteenth_count > last);
            last = clock.sixteenth_count;
        }
    }
}
