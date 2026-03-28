use super::note::{NoteValue, TimeSignature};
use super::tick::{ClockTick, RoboticClockTick, SwungClockTick};

/// Trigger flags produced by the clock on a given sample.
///
/// Each field is `true` on exactly the sample where that subdivision fires.
/// Instruments receive a `ClockOutput` each sample and act on whichever flags
/// they subscribe to — no callbacks or channels are needed.
#[derive(Default, Debug, Clone, Copy)]
pub struct ClockOutput {
    /// One trigger per bar, derived from the [`TimeSignature`].
    pub bar: bool,
    pub half: bool,
    pub quarter: bool,
    pub eighth: bool,
    pub sixteenth: bool,
    pub thirty_second: bool,
    pub quarter_triplet: bool,
    pub eighth_triplet: bool,
    pub sixteenth_triplet: bool,
    /// Eighth note with swing applied.
    pub swung_eighth: bool,
    /// Sixteenth note with swing applied.
    pub swung_sixteenth: bool,
}

/// Centralized clock service. Owns all subdivision tick sources and vends a
/// [`ClockOutput`] on every sample.
///
/// # Usage
///
/// ```no_run
/// use musicbox_core::clocks::{Clock, TimeSignature};
///
/// let mut clock = Clock::new(120.0, TimeSignature::four_four(), 0.67, 44100.0);
///
/// // In the render loop:
/// let ticks = clock.tick();
/// if ticks.quarter { /* fire drum hit */ }
/// if ticks.swung_eighth { /* fire swung hi-hat */ }
/// ```
pub struct Clock {
    bpm: f32,
    sample_rate: f32,
    bar: RoboticClockTick,
    half: RoboticClockTick,
    quarter: RoboticClockTick,
    eighth: RoboticClockTick,
    sixteenth: RoboticClockTick,
    thirty_second: RoboticClockTick,
    quarter_triplet: RoboticClockTick,
    eighth_triplet: RoboticClockTick,
    sixteenth_triplet: RoboticClockTick,
    swung_eighth: SwungClockTick,
    swung_sixteenth: SwungClockTick,
}

impl Clock {
    /// Construct a clock.
    ///
    /// - `bpm` — quarter notes per minute
    /// - `time_sig` — determines bar length only (individual note values are BPM-relative)
    /// - `swing` — on-beat fraction: 0.5 = straight, ~0.667 = triplet swing
    /// - `sample_rate` — samples per second
    pub fn new(bpm: f32, time_sig: TimeSignature, swing: f32, sample_rate: f32) -> Self {
        Self {
            bpm,
            sample_rate,
            bar: RoboticClockTick::from_quarter_notes(
                time_sig.bar_in_quarter_notes(),
                bpm,
                sample_rate,
            ),
            half: RoboticClockTick::new(NoteValue::Half, bpm, sample_rate),
            quarter: RoboticClockTick::new(NoteValue::Quarter, bpm, sample_rate),
            eighth: RoboticClockTick::new(NoteValue::Eighth, bpm, sample_rate),
            sixteenth: RoboticClockTick::new(NoteValue::Sixteenth, bpm, sample_rate),
            thirty_second: RoboticClockTick::new(NoteValue::ThirtySecond, bpm, sample_rate),
            quarter_triplet: RoboticClockTick::new(NoteValue::QuarterTriplet, bpm, sample_rate),
            eighth_triplet: RoboticClockTick::new(NoteValue::EighthTriplet, bpm, sample_rate),
            sixteenth_triplet: RoboticClockTick::new(NoteValue::SixteenthTriplet, bpm, sample_rate),
            swung_eighth: SwungClockTick::new(NoteValue::Eighth, swing, bpm, sample_rate),
            swung_sixteenth: SwungClockTick::new(NoteValue::Sixteenth, swing, bpm, sample_rate),
        }
    }

    /// Advance one sample and return all trigger flags for this sample.
    pub fn tick(&mut self) -> ClockOutput {
        ClockOutput {
            bar: self.bar.tick(),
            half: self.half.tick(),
            quarter: self.quarter.tick(),
            eighth: self.eighth.tick(),
            sixteenth: self.sixteenth.tick(),
            thirty_second: self.thirty_second.tick(),
            quarter_triplet: self.quarter_triplet.tick(),
            eighth_triplet: self.eighth_triplet.tick(),
            sixteenth_triplet: self.sixteenth_triplet.tick(),
            swung_eighth: self.swung_eighth.tick(),
            swung_sixteenth: self.swung_sixteenth.tick(),
        }
    }

    /// Change tempo without resetting any phase (seamless mid-stream tempo changes).
    pub fn set_bpm(&mut self, bpm: f32) {
        self.bpm = bpm;
        let sr = self.sample_rate;
        self.bar.set_bpm(bpm, sr);
        self.half.set_bpm(bpm, sr);
        self.quarter.set_bpm(bpm, sr);
        self.eighth.set_bpm(bpm, sr);
        self.sixteenth.set_bpm(bpm, sr);
        self.thirty_second.set_bpm(bpm, sr);
        self.quarter_triplet.set_bpm(bpm, sr);
        self.eighth_triplet.set_bpm(bpm, sr);
        self.sixteenth_triplet.set_bpm(bpm, sr);
        self.swung_eighth.set_bpm(bpm, sr);
        self.swung_sixteenth.set_bpm(bpm, sr);
    }

    /// Change swing amount without resetting phase.
    pub fn set_swing(&mut self, swing: f32) {
        let (bpm, sr) = (self.bpm, self.sample_rate);
        self.swung_eighth.set_swing(swing, bpm, sr);
        self.swung_sixteenth.set_swing(swing, bpm, sr);
    }

    pub fn bpm(&self) -> f32 { self.bpm }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const BPM: f32 = 120.0;
    const SR: f32 = 44100.0;
    const SPQ: usize = 22050; // samples per quarter at 120 BPM, 44100 SR

    fn count_field(clock: &mut Clock, samples: usize, field: fn(&ClockOutput) -> bool) -> usize {
        (0..samples).filter(|_| field(&clock.tick())).count()
    }

    #[test]
    fn four_four_bar_fires_every_four_quarters() {
        let mut clock = Clock::new(BPM, TimeSignature::four_four(), 0.5, SR);
        let bars = count_field(&mut clock, SPQ * 16, |t| t.bar);
        assert_eq!(bars, 4);
    }

    #[test]
    fn three_four_bar_fires_every_three_quarters() {
        let mut clock = Clock::new(BPM, TimeSignature::three_four(), 0.5, SR);
        let bars = count_field(&mut clock, SPQ * 12, |t| t.bar);
        assert_eq!(bars, 4);
    }

    #[test]
    fn six_eight_bar_fires_every_three_quarter_notes() {
        // 6/8 bar = 3 quarter notes = 3 * SPQ samples
        let mut clock = Clock::new(BPM, TimeSignature::six_eight(), 0.5, SR);
        let bars = count_field(&mut clock, SPQ * 12, |t| t.bar);
        assert_eq!(bars, 4);
    }

    #[test]
    fn all_subdivisions_correct_count() {
        // Add 1 extra sample so any tick that falls exactly on the boundary is captured.
        let samples = SPQ * 12 + 1;
        let mut clock = Clock::new(BPM, TimeSignature::four_four(), 0.5, SR);
        assert_eq!(count_field(&mut clock, samples, |t| t.quarter), 12);

        let mut clock = Clock::new(BPM, TimeSignature::four_four(), 0.5, SR);
        assert_eq!(count_field(&mut clock, samples, |t| t.eighth), 24);

        let mut clock = Clock::new(BPM, TimeSignature::four_four(), 0.5, SR);
        assert_eq!(count_field(&mut clock, samples, |t| t.sixteenth), 48);

        let mut clock = Clock::new(BPM, TimeSignature::four_four(), 0.5, SR);
        assert_eq!(count_field(&mut clock, samples, |t| t.thirty_second), 96);
    }

    #[test]
    fn triplet_counts() {
        // Add 1 extra sample so any tick that falls exactly on the boundary is captured.
        let samples = SPQ * 12 + 1;
        let mut clock = Clock::new(BPM, TimeSignature::four_four(), 0.5, SR);
        // 12 quarters → 12 * (3/2) = 18 quarter triplets
        assert_eq!(count_field(&mut clock, samples, |t| t.quarter_triplet), 18);

        let mut clock = Clock::new(BPM, TimeSignature::four_four(), 0.5, SR);
        // 12 quarters → 12 * 3 = 36 eighth triplets
        assert_eq!(count_field(&mut clock, samples, |t| t.eighth_triplet), 36);
    }

    #[test]
    fn swung_eighth_count_matches_straight() {
        // +1 so any fire that falls exactly on the window boundary is captured.
        let samples = SPQ * 16 + 1;
        let mut straight = Clock::new(BPM, TimeSignature::four_four(), 0.5, SR);
        let mut swung = Clock::new(BPM, TimeSignature::four_four(), 0.67, SR);
        let n_straight = count_field(&mut straight, samples, |t| t.eighth);
        let n_swung = count_field(&mut swung, samples, |t| t.swung_eighth);
        assert_eq!(n_straight, n_swung, "swung eighth count should equal straight eighth count");
    }

    #[test]
    fn set_bpm_changes_tick_rate() {
        let samples = SPQ * 4; // 4 quarters at 120 BPM
        let mut clock = Clock::new(BPM, TimeSignature::four_four(), 0.5, SR);
        clock.set_bpm(240.0); // double tempo
        // At 240 BPM, SPQ is halved → should fire 8 times in the same sample window.
        let quarters = count_field(&mut clock, samples, |t| t.quarter);
        assert_eq!(quarters, 8);
    }
}
