use bevy::prelude::{Message, MessageWriter, Res, ResMut, Resource, Time};
use bevy_seedling::prelude::{Audio, AudioTime};

/// Seconds after the clock starts (or resyncs) before its first step sounds.
const START_DELAY: f64 = 0.05;
/// A step this far behind the audio clock is skipped rather than played late.
const MAX_LATENESS: f64 = 0.05;
/// Seconds for the remembered worst frame gap to halve once frames are quick again.
const GAP_HALF_LIFE: f64 = 2.0;

/// The musical clock. It runs on the *audio* clock (`Time<Audio>`) and emits
/// each step as a [`Beat`] `lookahead` seconds before it should sound, stamped
/// with its exact audio time, so instruments can schedule sample-accurately.
///
/// Steps are only emitted while `Update` runs. When frames slow down (a
/// hidden or minimised window can drop to ~1 frame per second), the clock
/// schedules further ahead, up to `max_lookahead`, so the music keeps playing
/// through the gaps; it tightens back to `lookahead` once frames are quick again.
#[derive(Debug, Clone, Copy, Resource)]
pub struct Clock {
    pub beats: f32,
    pub note_type: f32,
    pub tempo_bpm: f32,
    pub playing: bool,
    /// Length of one step (a 16th in 4/4) in seconds. Change it to change tempo.
    pub beat_length: f32,
    /// How far ahead of the audio clock steps are emitted, in seconds, at
    /// normal frame rates. Lower means game changes are heard sooner.
    pub lookahead: f64,
    /// The furthest ahead the clock will schedule to bridge slow frames.
    pub max_lookahead: f64,
    /// The longest recent gap between updates (decays over time), in seconds.
    pub frame_gap: f64,
    /// Audio time of the previous update.
    pub last_update: Option<f64>,
    /// Audio time of the next step to emit; `None` until the clock starts.
    pub next_time: Option<f64>,
    pub sixteenth: u32,
    pub beat: u32,
    pub bar_count: u32,
    pub beat_count: u32,
    pub sixteenth_count: u32,
}

impl Clock {
    pub fn new(beats: u32, note_type: u32, bpm: f32) -> Self {
        Self {
            beats: beats as f32,
            note_type: note_type as f32,
            tempo_bpm: bpm,
            playing: true,
            beat_length: (60.0 / bpm / beats as f32) / (beats as f32 / note_type as f32),
            lookahead: 0.1,
            max_lookahead: 1.5,
            frame_gap: 0.0,
            last_update: None,
            next_time: None,
            sixteenth: 0,
            bar_count: 0,
            beat: 0,
            beat_count: 0,
            sixteenth_count: 0,
        }
    }

    /// Record that a frame happened at audio time `now`; call once per update,
    /// before draining [`Clock::next_step`].
    pub fn observe_frame(&mut self, now: f64) {
        if let Some(last) = self.last_update {
            let gap = (now - last).max(0.0);
            let decayed = self.frame_gap * 0.5f64.powf(gap / GAP_HALF_LIFE);
            self.frame_gap = gap.max(decayed);
        }
        self.last_update = Some(now);
    }

    /// How far ahead steps are emitted right now: enough to bridge the
    /// slowest recent frame gap, within `lookahead..=max_lookahead`.
    pub fn effective_lookahead(&self) -> f64 {
        (self.frame_gap * 1.5 + 0.02).clamp(self.lookahead, self.max_lookahead.max(self.lookahead))
    }

    /// Change tempo; takes effect from the next step.
    pub fn set_bpm(&mut self, bpm: f32) {
        let bpm = bpm.max(1.0);
        self.tempo_bpm = bpm;
        self.beat_length = (60.0 / bpm / self.beats) / (self.beats / self.note_type);
    }

    /// Fractional bar position of the next step: `bar_count + beat/beats + sixteenth/beats²`.
    pub fn time_bars(&self) -> f32 {
        self.bar_count as f32
            + self.beat as f32 / self.beats
            + self.sixteenth as f32 / (self.beats * self.beats)
    }

    /// Emit the next step if it starts before `now + lookahead`, then advance.
    pub fn next_step(&mut self, now: f64) -> Option<Beat> {
        if !self.playing {
            self.next_time = None;
            return None;
        }
        let mut time = self.next_time.unwrap_or(now + START_DELAY);
        if time >= now + self.effective_lookahead() {
            return None;
        }
        if time < now - MAX_LATENESS {
            // We stalled (hitch, breakpoint): resync instead of firing a burst of late notes.
            time = now + START_DELAY;
        }
        let beat = Beat {
            beat: self.beat,
            sixteenth: self.sixteenth,
            bar_count: self.bar_count,
            beat_count: self.beat_count,
            sixteenth_count: self.sixteenth_count,
            time_bars: self.time_bars(),
            time,
            step_secs: self.beat_length as f64,
        };
        self.next_time = Some(time + self.beat_length as f64);
        self.advance();
        Some(beat)
    }

    fn advance(&mut self) {
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
    }
}

#[derive(Debug, Clone, Copy, Default, Message)]
pub struct Beat {
    pub beat: u32,
    pub sixteenth: u32,
    pub bar_count: u32,
    pub beat_count: u32,
    pub sixteenth_count: u32,
    /// Fractional bar position of this step.
    pub time_bars: f32,
    /// Audio-clock time (seconds) at which this step sounds.
    pub time: f64,
    /// Length of one step in seconds, for turning step offsets into time.
    pub step_secs: f64,
}

pub fn progress_clock_system(
    mut clock: ResMut<Clock>,
    time: Res<Time<Audio>>,
    mut beat_sender: MessageWriter<Beat>,
) {
    let now = time.now().0;
    clock.observe_frame(now);
    while let Some(beat) = clock.next_step(now) {
        beat_sender.write(beat);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_clock() -> Clock {
        Clock::new(4, 4, 120.0) // 120 bpm, 4/4 → beat_length = 0.125s per 16th
    }

    /// Drain every step emitted up to audio time `now`.
    fn steps(clock: &mut Clock, now: f64) -> Vec<Beat> {
        std::iter::from_fn(|| clock.next_step(now)).collect()
    }

    #[test]
    fn first_step_is_the_downbeat_shortly_after_start() {
        let mut clock = make_clock();
        let beats = steps(&mut clock, 10.0);
        assert_eq!(beats.len(), 1);
        assert_eq!((beats[0].bar_count, beats[0].beat, beats[0].sixteenth), (0, 0, 0));
        assert!((beats[0].time - (10.0 + START_DELAY)).abs() < 1e-9);
    }

    #[test]
    fn emits_only_within_the_lookahead_window() {
        let mut clock = make_clock();
        steps(&mut clock, 0.0);
        // 0.05 + k*0.125 < now + 0.1 → by now = 1.0, steps up to t = 1.05 are out.
        let all: Vec<Beat> = (1..=100).flat_map(|k| steps(&mut clock, k as f64 * 0.01)).collect();
        let last = all.last().unwrap();
        assert!(last.time < 1.0 + clock.lookahead);
        assert!(clock.next_time.unwrap() >= 1.0 + clock.lookahead);
    }

    #[test]
    fn step_times_are_exact_regardless_of_frame_timing() {
        let mut clock = make_clock();
        let mut beats = steps(&mut clock, 0.0);
        let mut now = 0.0;
        for jitter in [0.016, 0.003, 0.041, 0.0, 0.033, 0.007].iter().cycle().take(300) {
            now += jitter;
            beats.extend(steps(&mut clock, now));
        }
        for (k, b) in beats.iter().enumerate() {
            assert!((b.time - (START_DELAY + k as f64 * 0.125)).abs() < 1e-9, "step {k}");
            assert_eq!(b.sixteenth_count, k as u32);
        }
    }

    #[test]
    fn bar_advances_after_sixteen_steps() {
        let mut clock = make_clock();
        let beats = steps(&mut clock, 3.0);
        let beats: Vec<Beat> = beats.into_iter().chain((1..40).flat_map(|k| steps(&mut clock, 3.0 + k as f64 * 0.1))).collect();
        assert_eq!((beats[4].beat, beats[4].sixteenth), (1, 0));
        assert_eq!((beats[16].bar_count, beats[16].beat, beats[16].sixteenth), (1, 0, 0));
        assert!((beats[16].time_bars - 1.0).abs() < 1e-5);
        assert!((beats[1].time_bars - 0.0625).abs() < 1e-5);
    }

    #[test]
    fn tempo_changes_apply_from_the_next_step() {
        let mut clock = make_clock();
        let a = steps(&mut clock, 0.0)[0];
        clock.beat_length = 0.25;
        let b = steps(&mut clock, 0.2)[0];
        let c = steps(&mut clock, 0.45)[0];
        assert!((b.time - a.time - 0.125).abs() < 1e-9);
        assert!((c.time - b.time - 0.25).abs() < 1e-9);
    }

    #[test]
    fn resyncs_after_a_stall_instead_of_bursting() {
        let mut clock = make_clock();
        steps(&mut clock, 0.0);
        let late = steps(&mut clock, 5.0);
        assert_eq!(late.len(), 1, "a stall must not flush a backlog of steps");
        assert!(late[0].time >= 5.0);
    }

    #[test]
    fn slow_frames_stretch_the_lookahead_so_music_keeps_going() {
        let mut clock = make_clock();
        let mut beats = Vec::new();
        let frame = |clock: &mut Clock, now: f64, beats: &mut Vec<Beat>| {
            clock.observe_frame(now);
            beats.extend(steps(clock, now));
        };
        // 2 s at 60 fps, then the window is hidden: one frame per second for 10 s.
        let mut now = 0.0;
        while now < 2.0 {
            frame(&mut clock, now, &mut beats);
            now += 1.0 / 60.0;
        }
        assert!(clock.effective_lookahead() < 0.11, "tight at 60 fps");
        for _ in 0..10 {
            now += 1.0;
            frame(&mut clock, now, &mut beats);
        }
        // The first unexpected 1 s stall forces one resync; after that the
        // grid must be unbroken: every step exactly one step after the last.
        let after = beats.iter().position(|b| b.time > 3.5).unwrap();
        for pair in beats[after..].windows(2) {
            assert!((pair[1].time - pair[0].time - 0.125).abs() < 1e-9, "gap at {}", pair[0].time);
        }
        assert!(beats.last().unwrap().time > now, "scheduled past the latest frame");

        // Back at 60 fps, the lookahead shrinks again within seconds.
        for _ in 0..(8 * 60) {
            now += 1.0 / 60.0;
            frame(&mut clock, now, &mut beats);
        }
        assert!(clock.effective_lookahead() < 0.2, "still {}", clock.effective_lookahead());
    }

    #[test]
    fn stopped_clock_emits_nothing_and_restarts_cleanly() {
        let mut clock = make_clock();
        clock.playing = false;
        assert!(steps(&mut clock, 1.0).is_empty());
        clock.playing = true;
        let b = steps(&mut clock, 2.0);
        assert!((b[0].time - (2.0 + START_DELAY)).abs() < 1e-9);
    }
}
