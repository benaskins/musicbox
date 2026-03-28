/// High synth pad: four detuned sawtooth oscillators with vibrato, slow attack,
/// LFO-swept LPF, and plate reverb. Detuning spread emulates a string ensemble.
pub struct SynthPad {
    phases: [f32; 4],
    base_freqs: [f32; 4],
    vibrato_phase: f32,
    pub amp: f32,
    attack_rate: f32,
    release_rate: f32,
    sustaining: bool,
    sample_rate: f32,
}

impl SynthPad {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            phases: [0.0; 4],
            base_freqs: [0.0; 4],
            vibrato_phase: 0.0,
            amp: 0.0,
            attack_rate: 1.0 / (sample_rate * 2.0),  // 2s attack
            release_rate: 1.0 / (sample_rate * 3.0), // 3s release
            sustaining: false,
            sample_rate,
        }
    }

    /// Start a new note. Four voices spread at 0, +12, -12, +24 cents
    /// to emulate a string section. Phase is preserved to avoid clicks.
    pub fn trigger(&mut self, freq: f32) {
        self.base_freqs[0] = freq;
        self.base_freqs[1] = freq * 2_f32.powf( 12.0 / 1200.0);
        self.base_freqs[2] = freq * 2_f32.powf(-12.0 / 1200.0);
        self.base_freqs[3] = freq * 2_f32.powf( 24.0 / 1200.0);
        self.sustaining = true;
    }

    /// Start a minor triad. Voices: root, minor third, perfect fifth, root+12 cents (for width).
    pub fn trigger_minor_chord(&mut self, root: f32) {
        let minor_third = root * 2_f32.powf(3.0 / 12.0);  // +3 semitones
        let fifth       = root * 2_f32.powf(7.0 / 12.0);  // +7 semitones
        self.base_freqs[0] = root;
        self.base_freqs[1] = minor_third;
        self.base_freqs[2] = fifth;
        self.base_freqs[3] = root * 2_f32.powf(12.0 / 1200.0); // root +12 cents for width
        self.sustaining = true;
    }

    pub fn release(&mut self) {
        self.sustaining = false;
    }

    pub fn next_sample(&mut self) -> f32 {
        if self.sustaining {
            self.amp = (self.amp + self.attack_rate).min(0.028);
        } else {
            self.amp = (self.amp - self.release_rate).max(0.0);
        }

        if self.amp == 0.0 {
            return 0.0;
        }

        // Vibrato: 5 Hz, ~4 cents depth
        let vibrato = (self.vibrato_phase * std::f32::consts::TAU).sin() * 0.0023;
        self.vibrato_phase += 5.0 / self.sample_rate;
        if self.vibrato_phase >= 1.0 { self.vibrato_phase -= 1.0; }

        let mut sig = 0.0f32;
        for i in 0..4 {
            let freq = self.base_freqs[i] * (1.0 + vibrato);
            // Sawtooth wave: rich harmonic content like bowed strings
            let saw = 2.0 * self.phases[i] - 1.0;
            // Small triangle blend softens the harshest overtones
            let tri = 4.0 * (self.phases[i] - (self.phases[i] + 0.5).floor()).abs() - 1.0;
            sig += saw * 0.8 + tri * 0.2;
            self.phases[i] += freq / self.sample_rate;
            if self.phases[i] >= 1.0 { self.phases[i] -= 1.0; }
        }

        sig / 4.0 * self.amp
    }
}
