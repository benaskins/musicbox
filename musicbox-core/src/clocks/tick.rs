use super::note::NoteValue;

/// A sample-rate clock source that emits a boolean trigger once per period.
pub trait ClockTick {
    /// Advance one sample. Returns `true` on the sample where a trigger fires.
    fn tick(&mut self) -> bool;

    /// Reset phase to the start of a cycle without changing tempo or swing.
    fn reset(&mut self);
}

// ── RoboticClockTick ─────────────────────────────────────────────────────────

/// Fires at perfectly regular intervals using phase accumulation.
///
/// The period is stored as a fractional phase increment per sample (f64). On each
/// sample the phase grows by that increment; when it reaches or exceeds 1.0 a
/// trigger fires and the overflow carries into the next cycle. This means any
/// rounding introduced by non-integer sample periods is corrected on the very
/// next tick rather than accumulating indefinitely.
pub struct RoboticClockTick {
    phase: f64,
    phase_inc: f64,
    period_quarter_notes: f64,
}

impl RoboticClockTick {
    /// Create a tick source for a standard note value at the given BPM.
    pub fn new(note_value: NoteValue, bpm: f32, sample_rate: f32) -> Self {
        Self::from_quarter_notes(note_value.in_quarter_notes(), bpm, sample_rate)
    }

    /// Create a tick source from an arbitrary period expressed in quarter notes.
    ///
    /// Useful for bar-length ticks derived from a [`TimeSignature`].
    ///
    /// [`TimeSignature`]: super::note::TimeSignature
    pub fn from_quarter_notes(period_qn: f64, bpm: f32, sample_rate: f32) -> Self {
        Self {
            phase: 0.0,
            phase_inc: Self::compute_inc(period_qn, bpm, sample_rate),
            period_quarter_notes: period_qn,
        }
    }

    /// Update the tempo without resetting phase (no audible discontinuity).
    pub fn set_bpm(&mut self, bpm: f32, sample_rate: f32) {
        self.phase_inc = Self::compute_inc(self.period_quarter_notes, bpm, sample_rate);
    }

    fn compute_inc(period_qn: f64, bpm: f32, sample_rate: f32) -> f64 {
        let samples_per_quarter = sample_rate as f64 * 60.0 / bpm as f64;
        1.0 / (period_qn * samples_per_quarter)
    }
}

impl ClockTick for RoboticClockTick {
    fn tick(&mut self) -> bool {
        self.phase += self.phase_inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            true
        } else {
            false
        }
    }

    fn reset(&mut self) {
        self.phase = 0.0;
    }
}

// ── SwungClockTick ────────────────────────────────────────────────────────────

/// Fires at alternating long/short intervals to produce a swing (shuffle) feel.
///
/// On-beats occupy `swing_ratio` of a parent period; off-beats fill the rest.
/// The parent period is two of the note value being swung.
///
/// | `swing_ratio` | feel                     |
/// |---------------|--------------------------|
/// | 0.50          | straight (no swing)      |
/// | 0.60          | light shuffle            |
/// | 0.67 (≈ 2/3)  | triplet swing            |
/// | 0.75          | heavy/dotted shuffle     |
///
/// Phase accumulation is used for both on-beat and off-beat halves, so rounding
/// is corrected within each half-period rather than accumulating.
pub struct SwungClockTick {
    phase: f64,
    on_beat_inc: f64,
    off_beat_inc: f64,
    is_on_beat: bool,
    // Stored for BPM / swing updates
    parent_period_qn: f64,
    swing_ratio: f64,
}

impl SwungClockTick {
    /// `note_value` is the subdivision being swung (e.g. `NoteValue::Eighth` for
    /// 8th-note swing). The parent period is two of those subdivisions.
    ///
    /// `swing_ratio` is clamped to `[0.01, 0.99]` to keep both halves non-zero.
    /// Pass `0.5` for straight (unswung) time.
    pub fn new(note_value: NoteValue, swing_ratio: f32, bpm: f32, sample_rate: f32) -> Self {
        let parent_qn = note_value.in_quarter_notes() * 2.0;
        let ratio = (swing_ratio as f64).clamp(0.01, 0.99);
        Self::from_parts(parent_qn, ratio, bpm, sample_rate)
    }

    /// Update the tempo without resetting phase.
    pub fn set_bpm(&mut self, bpm: f32, sample_rate: f32) {
        let (on, off) = Self::compute_incs(self.parent_period_qn, self.swing_ratio, bpm, sample_rate);
        self.on_beat_inc = on;
        self.off_beat_inc = off;
    }

    /// Update the swing ratio without resetting phase.
    pub fn set_swing(&mut self, swing_ratio: f32, bpm: f32, sample_rate: f32) {
        self.swing_ratio = (swing_ratio as f64).clamp(0.01, 0.99);
        self.set_bpm(bpm, sample_rate);
    }

    fn from_parts(parent_period_qn: f64, swing_ratio: f64, bpm: f32, sample_rate: f32) -> Self {
        let (on_beat_inc, off_beat_inc) =
            Self::compute_incs(parent_period_qn, swing_ratio, bpm, sample_rate);
        Self {
            phase: 0.0,
            on_beat_inc,
            off_beat_inc,
            is_on_beat: true,
            parent_period_qn,
            swing_ratio,
        }
    }

    fn compute_incs(parent_qn: f64, swing_ratio: f64, bpm: f32, sample_rate: f32) -> (f64, f64) {
        let samples_per_quarter = sample_rate as f64 * 60.0 / bpm as f64;
        let parent_samples = parent_qn * samples_per_quarter;
        let on = 1.0 / (swing_ratio * parent_samples);
        let off = 1.0 / ((1.0 - swing_ratio) * parent_samples);
        (on, off)
    }
}

impl ClockTick for SwungClockTick {
    fn tick(&mut self) -> bool {
        let inc = if self.is_on_beat { self.on_beat_inc } else { self.off_beat_inc };
        let next_inc = if self.is_on_beat { self.off_beat_inc } else { self.on_beat_inc };
        self.phase += inc;
        if self.phase >= 1.0 {
            // Convert the residual phase from the current increment's scale to the next
            // increment's scale. Without this, a late fire on one half-beat would not be
            // correctly compensated on the following half-beat.
            self.phase = (self.phase - 1.0) * (next_inc / inc);
            self.is_on_beat = !self.is_on_beat;
            true
        } else {
            false
        }
    }

    fn reset(&mut self) {
        self.phase = 0.0;
        self.is_on_beat = true;
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const BPM: f32 = 120.0;
    const SR: f32 = 44100.0;
    // At 120 BPM, 44100 SR: samples per quarter = 22050 (exact).
    const SPQ: usize = 22050;

    fn count_ticks(tick: &mut dyn ClockTick, samples: usize) -> usize {
        (0..samples).filter(|_| tick.tick()).count()
    }

    #[test]
    fn robotic_quarter_fires_at_correct_interval() {
        let mut t = RoboticClockTick::new(NoteValue::Quarter, BPM, SR);
        // First tick fires at sample SPQ (index SPQ-1 in 0-based, but we count calls).
        for _ in 0..SPQ - 1 {
            assert!(!t.tick());
        }
        assert!(t.tick(), "quarter tick should fire on sample {}", SPQ);
    }

    #[test]
    fn robotic_eighth_fires_twice_per_quarter() {
        let mut t = RoboticClockTick::new(NoteValue::Eighth, BPM, SR);
        assert_eq!(count_ticks(&mut t, SPQ * 4 + 1), 8);
    }

    #[test]
    fn robotic_sixteenth_fires_four_times_per_quarter() {
        let mut t = RoboticClockTick::new(NoteValue::Sixteenth, BPM, SR);
        assert_eq!(count_ticks(&mut t, SPQ * 4 + 1), 16);
    }

    #[test]
    fn robotic_thirty_second_count() {
        let mut t = RoboticClockTick::new(NoteValue::ThirtySecond, BPM, SR);
        assert_eq!(count_ticks(&mut t, SPQ * 4 + 1), 32);
    }

    #[test]
    fn robotic_quarter_triplet_count() {
        // 3 quarter triplets = 2 quarter notes. In 4 quarters: 4 * (3/2) = 6 triplets.
        let mut t = RoboticClockTick::new(NoteValue::QuarterTriplet, BPM, SR);
        assert_eq!(count_ticks(&mut t, SPQ * 4 + 1), 6);
    }

    #[test]
    fn robotic_eighth_triplet_count() {
        // 3 eighth triplets = 1 quarter note. In 4 quarters: 12 triplets.
        let mut t = RoboticClockTick::new(NoteValue::EighthTriplet, BPM, SR);
        assert_eq!(count_ticks(&mut t, SPQ * 4 + 1), 12);
    }

    #[test]
    fn robotic_reset_restarts_phase() {
        let mut t = RoboticClockTick::new(NoteValue::Quarter, BPM, SR);
        // Advance halfway through the period.
        for _ in 0..SPQ / 2 {
            t.tick();
        }
        t.reset();
        // Should take a full period to fire again.
        assert_eq!(count_ticks(&mut t, SPQ - 1), 0);
        assert!(t.tick());
    }

    #[test]
    fn swung_on_beat_longer_than_off_beat() {
        let swing = 0.67_f32; // ≈ triplet swing
        let mut t = SwungClockTick::new(NoteValue::Eighth, swing, BPM, SR);

        let mut intervals: Vec<usize> = Vec::new();
        let mut since_last = 0usize;
        for _ in 0..SPQ * 8 {
            since_last += 1;
            if t.tick() {
                intervals.push(since_last);
                since_last = 0;
            }
        }

        // Odd-indexed intervals (0, 2, 4 …) are on-beats; even-indexed are off-beats.
        let on_beats: Vec<usize> = intervals.iter().copied().step_by(2).collect();
        let off_beats: Vec<usize> = intervals.iter().skip(1).copied().step_by(2).collect();

        let on_avg = on_beats.iter().sum::<usize>() as f64 / on_beats.len() as f64;
        let off_avg = off_beats.iter().sum::<usize>() as f64 / off_beats.len() as f64;

        assert!(on_avg > off_avg, "on-beat ({on_avg:.1}) should be longer than off-beat ({off_avg:.1})");

        // Ratio should be approximately swing / (1 - swing) ≈ 2.03 for ratio=0.67.
        let ratio = on_avg / off_avg;
        let expected = 0.67 / (1.0 - 0.67);
        assert!((ratio - expected).abs() < 0.05, "swing ratio {ratio:.3} expected ≈{expected:.3}");
    }

    #[test]
    fn swung_straight_is_uniform() {
        let mut t = SwungClockTick::new(NoteValue::Eighth, 0.5, BPM, SR);
        let mut intervals: Vec<usize> = Vec::new();
        let mut since_last = 0usize;
        for _ in 0..SPQ * 8 {
            since_last += 1;
            if t.tick() {
                intervals.push(since_last);
                since_last = 0;
            }
        }
        let min = *intervals.iter().min().unwrap();
        let max = *intervals.iter().max().unwrap();
        // At 0.5 swing (straight), all intervals should be within ±1 sample.
        assert!(max - min <= 1, "straight swing should have uniform intervals (got {min}..{max})");
    }

    #[test]
    fn swung_fires_same_total_count_as_robotic() {
        // +1 so any fire that falls exactly on the window boundary is captured.
        let samples = SPQ * 16 + 1;
        let mut robotic = RoboticClockTick::new(NoteValue::Eighth, BPM, SR);
        let mut swung = SwungClockTick::new(NoteValue::Eighth, 0.67, BPM, SR);
        assert_eq!(count_ticks(&mut robotic, samples), count_ticks(&mut swung, samples));
    }
}
