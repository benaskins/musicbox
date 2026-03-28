/// A slow-evolving sine LFO that produces a swing (shuffle) timing offset in samples.
///
/// Phase advances by 1/32 per beat, completing one full sine cycle every 32 beats (~27s at 72 BPM).
/// Call `advance()` once per kick; call `offset_samples(beat_duration)` anywhere a swing nudge is needed.
pub struct SwingLfo {
    phase: f32, // 0..1, wraps every 32 beats
}

impl SwingLfo {
    pub fn new() -> Self {
        Self { phase: 0.0 }
    }

    /// Advance one beat. Call this on every kick trigger.
    pub fn advance(&mut self) {
        self.phase += 1.0 / 32.0;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
    }

    /// Current swing delay in samples. Off-beat positions should be delayed by this amount.
    /// Range: 0 ..= beat_duration / 12.
    pub fn offset_samples(&self, beat_duration: u32) -> u32 {
        let max_swing = beat_duration / 4 / 3;
        let t = (self.phase * std::f32::consts::TAU).sin() * 0.5 + 0.5; // 0..1
        let t = 0.75 + t * 0.25; // remap to 0.75..1.0 — never fully straight
        (t * max_swing as f32) as u32
    }
}
