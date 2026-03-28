use crate::util::prng::Xorshift64;

/// Dub stab: 2-3 detuned saw/triangle oscillators forming a chord,
/// with fast attack, band-pass filtered (decaying LPF + fixed HPF),
/// and fed through dub delay.
pub struct DubStab {
    phases: [f32; 3],
    freqs: [f32; 3],
    /// 0.0 = saw, 1.0 = triangle (blends between them)
    wave_blend: f32,
    pub amp: f32,
    decay: f32,
    /// LPF state (decaying cutoff — each stab opens and closes)
    lp_low: f32,
    lp_band: f32,
    lp_cutoff: f32,
    lp_decay: f32,
    /// HPF state (fixed cutoff — removes mud)
    hp_low: f32,
    hp_band: f32,
    hp_cutoff: f32,
    sample_rate: f32,
    pub active: bool,
}

impl DubStab {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            phases: [0.0; 3],
            freqs: [0.0; 3],
            wave_blend: 0.4, // mostly saw with some triangle character
            amp: 0.0,
            decay: (-1.0 / (sample_rate * 0.35_f32)).exp(), // ~350ms decay
            lp_low: 0.0,
            lp_band: 0.0,
            lp_cutoff: 800.0,
            lp_decay: (-1.0 / (sample_rate * 0.25_f32)).exp(),
            hp_low: 0.0,
            hp_band: 0.0,
            hp_cutoff: 250.0,
            sample_rate,
            active: false,
        }
    }

    /// Long-decay variant: amplitude holds for ~4 beats (3.3s at 72 BPM), filter closes slowly.
    pub fn new_long(sample_rate: f32) -> Self {
        Self {
            decay: (-1.0 / (sample_rate * 3.0_f32)).exp(),
            lp_decay: (-1.0 / (sample_rate * 2.0_f32)).exp(),
            ..Self::new(sample_rate)
        }
    }

    /// Trigger with explicit chord tones, applying slight random detuning to each voice.
    pub fn trigger_with_chord(&mut self, notes: [f32; 3], rng: &mut Xorshift64) {
        self.trigger_with_chord_and_cutoff(notes, 2500.0, rng);
    }

    pub fn trigger_with_chord_and_cutoff(&mut self, notes: [f32; 3], initial_cutoff: f32, rng: &mut Xorshift64) {
        self.freqs[0] = notes[0] * (1.0 + rng.white() * 0.008);
        self.freqs[1] = notes[1] * (1.0 + rng.white() * 0.012);
        self.freqs[2] = notes[2] * (1.0 + rng.white() * 0.010);
        self.phases = [0.0; 3];
        self.amp = 0.35;
        self.lp_cutoff = initial_cutoff;
        self.lp_low = 0.0;
        self.lp_band = 0.0;
        self.hp_low = 0.0;
        self.hp_band = 0.0;
        self.active = true;
    }

    /// Trigger a stab with a root frequency. Creates a minor chord (root, minor 3rd, 5th)
    /// with slight detuning.
    pub fn trigger(&mut self, root_freq: f32, rng: &mut Xorshift64) {
        self.trigger_with_chord([root_freq, root_freq * 1.2, root_freq * 1.5], rng);
    }

    #[inline]
    fn saw(phase: f32) -> f32 {
        2.0 * phase - 1.0
    }

    #[inline]
    fn triangle(phase: f32) -> f32 {
        4.0 * (phase - (phase + 0.5).floor()).abs() - 1.0
    }

    pub fn next_sample(&mut self) -> f32 {
        if !self.active {
            return 0.0;
        }

        // Sum detuned saw/triangle oscillators
        let mut sig = 0.0;
        for i in 0..3 {
            let s = Self::saw(self.phases[i]);
            let t = Self::triangle(self.phases[i]);
            sig += s + (t - s) * self.wave_blend;
            self.phases[i] += self.freqs[i] / self.sample_rate;
            if self.phases[i] >= 1.0 {
                self.phases[i] -= 1.0;
            }
        }
        sig *= self.amp / 3.0;

        // LPF with decaying cutoff — stab opens bright then closes
        let lp_f = (std::f32::consts::PI * self.lp_cutoff / self.sample_rate).sin() * 2.0;
        let lp_q = 0.5;
        let lp_high = sig - self.lp_low - lp_q * self.lp_band;
        self.lp_band += lp_f * lp_high;
        self.lp_low += lp_f * self.lp_band;

        // HPF — fixed cutoff, removes mud
        let hp_f = (std::f32::consts::PI * self.hp_cutoff / self.sample_rate).sin() * 2.0;
        let hp_q = 0.5;
        let hp_high = self.lp_low - self.hp_low - hp_q * self.hp_band;
        self.hp_band += hp_f * hp_high;
        self.hp_low += hp_f * self.hp_band;

        self.amp *= self.decay;
        self.lp_cutoff = 250.0 + (self.lp_cutoff - 250.0) * self.lp_decay;

        if self.amp < 0.001 {
            self.active = false;
            self.amp = 0.0;
        }

        hp_high
    }
}
