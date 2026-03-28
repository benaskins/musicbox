/// A sub-Hz oscillator that emits trigger events each cycle.
/// Phase accumulates from 0.0 to 1.0; a trigger fires when it wraps.
pub struct PulseOscillator {
    phase: f32,
    freq: f32,
    sample_rate: f32,
}

impl PulseOscillator {
    pub fn new(freq: f32, sample_rate: f32) -> Self {
        Self {
            phase: 0.0,
            freq,
            sample_rate,
        }
    }

    pub fn new_with_phase(freq: f32, sample_rate: f32, phase: f32) -> Self {
        Self {
            phase,
            freq,
            sample_rate,
        }
    }

    /// Advance one sample. Returns true on the sample where phase wraps.
    pub fn tick(&mut self) -> bool {
        self.phase += self.freq / self.sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            true
        } else {
            false
        }
    }

    pub fn set_freq(&mut self, freq: f32) {
        self.freq = freq;
    }

    pub fn freq(&self) -> f32 {
        self.freq
    }
}
