use rand::Rng;

/// A single drone oscillator that fades in and out over time.
pub struct Oscillator {
    /// Current phase of the audio oscillator (0.0 to 1.0)
    pub phase: f32,
    /// Frequency in Hz
    pub freq: f32,
    /// Phase of the slow LFO that controls amplitude envelope
    pub envelope_phase: f32,
    /// How fast the envelope LFO cycles (Hz) — very slow, e.g. 0.012-0.05 Hz
    pub envelope_rate: f32,
    /// Phase of an LFO that gently drifts the pitch
    pub drift_phase: f32,
    /// Rate of pitch drift LFO
    pub drift_rate: f32,
    /// Max pitch drift in Hz
    pub drift_amount: f32,
    /// Base amplitude for this oscillator
    pub amplitude: f32,
    /// How much of the cycle is silent (0.0 = always on, 0.8 = silent 80% of the time)
    pub sparsity: f32,
    /// Sample rate
    pub sample_rate: f32,
}

impl Oscillator {
    pub fn new(freq: f32, amplitude: f32, sparsity: f32, sample_rate: f32, rng: &mut impl Rng) -> Self {
        Self {
            phase: rng.r#gen::<f32>(),
            freq,
            envelope_phase: rng.r#gen::<f32>(),
            envelope_rate: rng.r#gen_range(0.012..0.05),
            drift_phase: rng.r#gen::<f32>(),
            drift_rate: rng.r#gen_range(0.02..0.08),
            drift_amount: freq * 0.003,
            amplitude,
            sparsity,
            sample_rate,
        }
    }

    /// Generate the next sample and advance all phases.
    pub fn next_sample(&mut self) -> f32 {
        let envelope_raw = (self.envelope_phase * std::f32::consts::TAU).sin();
        let envelope_01 = (envelope_raw + 1.0) * 0.5;
        let envelope = ((envelope_01 - self.sparsity) / (1.0 - self.sparsity)).clamp(0.0, 1.0);
        let envelope = envelope * envelope;

        let drift = (self.drift_phase * std::f32::consts::TAU).sin() * self.drift_amount;
        let current_freq = self.freq + drift;

        let sample = (self.phase * std::f32::consts::TAU).sin();

        self.phase += current_freq / self.sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        self.envelope_phase += self.envelope_rate / self.sample_rate;
        if self.envelope_phase >= 1.0 {
            self.envelope_phase -= 1.0;
        }
        self.drift_phase += self.drift_rate / self.sample_rate;
        if self.drift_phase >= 1.0 {
            self.drift_phase -= 1.0;
        }

        sample * envelope * self.amplitude
    }
}
