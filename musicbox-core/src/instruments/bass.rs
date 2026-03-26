/// SH-101 inspired monosynth: sine oscillator with sub-oscillator one octave down,
/// through a cascaded 4-pole resonant low-pass (two SVF stages) with a decaying
/// filter envelope and portamento glide.
pub struct MonoSynth {
    phase: f32,          // main oscillator
    sub_phase: f32,      // sub-oscillator, one octave down
    freq: f32,           // current frequency (glides toward target)
    target_freq: f32,
    pub amp: f32,
    amp_peak: f32,       // 0.6 normal, 1.0 accented
    amp_attack_rate: f32,
    amp_decay: f32,
    attacking: bool,
    // Stage 1 SVF
    lp1_low: f32,
    lp1_band: f32,
    // Stage 2 SVF (cascaded for 4-pole response)
    lp2_low: f32,
    lp2_band: f32,
    filter_env: f32,     // 0..1, decays after each trigger
    filter_env_decay: f32,
    sweep_phase: f32,    // 0..1, advances by 1/64 per trigger — slow LPF sweep
    cutoff_min: f32,     // Hz — LPF base cutoff lower bound
    cutoff_sweep_range: f32, // Hz — how far the sweep opens the filter above cutoff_min
    cutoff_peak: f32,    // Hz — maximum cutoff reached at full filter_env
    resonance: f32,
    portamento: f32,     // exponential glide coefficient per sample (smaller = slower)
    sample_rate: f32,
}

impl MonoSynth {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            phase: 0.0,
            sub_phase: 0.0,
            freq: 110.0,
            target_freq: 110.0,
            amp: 0.0,
            amp_peak: 0.6,
            amp_attack_rate: 0.6 / (sample_rate * 0.008_f32), // ~8ms attack (snappier)
            amp_decay: (-1.0 / (sample_rate * 0.15_f32)).exp(), // ~150ms note decay
            attacking: false,
            lp1_low: 0.0,
            lp1_band: 0.0,
            lp2_low: 0.0,
            lp2_band: 0.0,
            filter_env: 0.0,
            filter_env_decay: (-1.0 / (sample_rate * 0.05_f32)).exp(), // ~50ms filter sweep
            sweep_phase: 0.0,
            cutoff_min: 40.0,
            cutoff_sweep_range: 210.0, // 40–250 Hz
            cutoff_peak: 400.0,
            resonance: 0.7,
            portamento: 0.005,
            sample_rate,
        }
    }

    /// Bass variant: lower cutoff, lower resonance, longer decay — thick and rounded.
    pub fn new_bass(sample_rate: f32) -> Self {
        Self {
            amp_attack_rate: 0.6 / (sample_rate * 0.025_f32), // ~25ms attack
            amp_decay: (-1.0 / (sample_rate * 0.12_f32)).exp(), // ~120ms note decay
            filter_env_decay: (-1.0 / (sample_rate * 0.08_f32)).exp(), // ~80ms filter sweep
            cutoff_min: 120.0,
            cutoff_sweep_range: 120.0, // 120–240 Hz
            cutoff_peak: 300.0,
            resonance: 0.25,
            portamento: 0.001,
            ..Self::new(sample_rate)
        }
    }

    pub fn trigger(&mut self, freq: f32, accented: bool) {
        self.target_freq = freq;
        // Don't reset amp to 0 — retrigger from current level to avoid a click.
        self.amp_peak = if accented { 1.0 } else { 0.6 };
        self.attacking = true;
        self.filter_env = if accented { 1.3 } else { 1.0 }; // accent also opens filter wider
        self.sweep_phase += 1.0 / 64.0;
        if self.sweep_phase >= 1.0 { self.sweep_phase -= 1.0; }
    }

    pub fn next_sample(&mut self) -> f32 {
        if self.amp < 0.0001 && !self.attacking {
            return 0.0;
        }

        // Portamento: exponential glide toward target frequency
        self.freq += (self.target_freq - self.freq) * self.portamento;

        // Sawtooth main oscillator (SH-101 style)
        let main = 1.0 - 2.0 * self.phase;
        self.phase += self.freq / self.sample_rate;
        if self.phase >= 1.0 { self.phase -= 1.0; }

        // Sub-oscillator: square wave one octave down
        let sub = if self.sub_phase < 0.5 { 1.0_f32 } else { -1.0_f32 };
        self.sub_phase += (self.freq * 0.5) / self.sample_rate;
        if self.sub_phase >= 1.0 { self.sub_phase -= 1.0; }

        let osc = main * 0.7 + sub * 0.3;

        // Cascaded 4-pole resonant low-pass (two SVF stages)
        let sweep = (self.sweep_phase * std::f32::consts::TAU).sin() * 0.5 + 0.5; // 0..1
        let base_cutoff = self.cutoff_min + sweep * self.cutoff_sweep_range;
        let cutoff = base_cutoff + self.filter_env * (self.cutoff_peak - base_cutoff);
        let f = (std::f32::consts::PI * cutoff / self.sample_rate).sin() * 2.0;
        let resonance = self.resonance;

        let high1 = osc - self.lp1_low - resonance * self.lp1_band;
        self.lp1_band += f * high1;
        self.lp1_low += f * self.lp1_band;

        let high2 = self.lp1_low - self.lp2_low - resonance * self.lp2_band;
        self.lp2_band += f * high2;
        self.lp2_low += f * self.lp2_band;

        self.filter_env *= self.filter_env_decay;
        if self.attacking {
            self.amp += self.amp_attack_rate;
            if self.amp >= self.amp_peak {
                self.amp = self.amp_peak;
                self.attacking = false;
            }
        } else {
            self.amp *= self.amp_decay;
        }

        self.lp2_low * self.amp
    }
}
