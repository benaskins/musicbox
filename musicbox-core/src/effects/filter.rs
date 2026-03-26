use rand::Rng;

/// A resonant low-pass filter using a state-variable filter topology.
pub struct ResonantLpf {
    pub low: f32,
    pub band: f32,
    pub cutoff_lfo_phase: f32,
    pub cutoff_lfo_rate: f32,
    pub cutoff_min: f32,
    pub cutoff_max: f32,
    pub resonance: f32,
    pub sample_rate: f32,
}

impl ResonantLpf {
    pub fn new(cutoff_min: f32, cutoff_max: f32, resonance: f32, lfo_rate: f32, sample_rate: f32, rng: &mut impl Rng) -> Self {
        Self {
            low: 0.0,
            band: 0.0,
            cutoff_lfo_phase: rng.r#gen::<f32>(),
            cutoff_lfo_rate: lfo_rate,
            cutoff_min,
            cutoff_max,
            resonance,
            sample_rate,
        }
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let lfo = (self.cutoff_lfo_phase * std::f32::consts::TAU).sin();
        let cutoff = self.cutoff_min + (self.cutoff_max - self.cutoff_min) * (lfo + 1.0) * 0.5;

        let f = (std::f32::consts::PI * cutoff / self.sample_rate).sin() * 2.0;
        let q = 1.0 - self.resonance;

        for _ in 0..2 {
            let high = input - self.low - q * self.band;
            self.band += f * high;
            self.low += f * self.band;
        }

        self.cutoff_lfo_phase += self.cutoff_lfo_rate / self.sample_rate;
        if self.cutoff_lfo_phase >= 1.0 {
            self.cutoff_lfo_phase -= 1.0;
        }

        self.low
    }
}
