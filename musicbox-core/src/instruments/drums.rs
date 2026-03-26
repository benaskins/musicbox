use crate::util::prng::Xorshift64;

/// Synthesized kick drum.
/// Sine body with exponential pitch envelope: starts at `pitch_start` Hz,
/// decays to `pitch_end` Hz. Amplitude decays exponentially.
pub struct Kick {
    pub phase: f32,
    pub pitch_start: f32,
    pub pitch_end: f32,
    pub pitch_decay: f32,
    pub amp_decay: f32,
    pub current_pitch: f32,
    pub current_pitch_end: f32,
    pub current_amp: f32,
    pub sample_rate: f32,
    pub active: bool,
}

impl Kick {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            phase: 0.0,
            pitch_start: 110.0,
            pitch_end: 50.0,
            pitch_decay: 0.995,
            amp_decay: 0.9995,
            current_pitch: 50.0,
            current_pitch_end: 50.0,
            current_amp: 0.0,
            sample_rate,
            active: false,
        }
    }

    /// Ghost kick: same pitch shape but half the decay rate (double length).
    pub fn new_ghost(sample_rate: f32) -> Self {
        Self {
            amp_decay: 0.9995_f32.powf(0.25), // decays to silence in ~4× the time of a normal kick
            ..Self::new(sample_rate)
        }
    }

    /// Fire the kick — resets envelope.
    pub fn trigger(&mut self) {
        self.trigger_with_amp(1.0);
    }

    /// Fire with a specific peak amplitude.
    pub fn trigger_with_amp(&mut self, amp: f32) {
        self.trigger_with_amp_and_pitch(amp, 1.0);
    }

    /// Fire with a specific amplitude and pitch multiplier (both start and end pitch scale together).
    pub fn trigger_with_amp_and_pitch(&mut self, amp: f32, pitch_mul: f32) {
        self.phase = 0.0;
        self.current_pitch = self.pitch_start * pitch_mul;
        self.current_pitch_end = self.pitch_end * pitch_mul;
        self.current_amp = amp;
        self.active = true;
    }

    /// Generate next sample.
    pub fn next_sample(&mut self) -> f32 {
        if !self.active {
            return 0.0;
        }

        let sample = (self.phase * std::f32::consts::TAU).sin();

        self.phase += self.current_pitch / self.sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        // Exponential pitch decay toward current_pitch_end
        self.current_pitch = self.current_pitch_end
            + (self.current_pitch - self.current_pitch_end) * self.pitch_decay;

        // Exponential amplitude decay
        self.current_amp *= self.amp_decay;
        if self.current_amp < 0.001 {
            self.active = false;
            self.current_amp = 0.0;
        }

        // Soft clip for punch
        (sample * self.current_amp * 1.5).tanh()
    }
}

/// Roland TR-808 style snare: two detuned sine bodies with fast pitch drop,
/// plus bright band-passed noise. Both components have independent amplitude envelopes.
pub struct Snare808 {
    // Dual sine body (detuned pair — characteristic of the 808 circuit)
    tone1_phase: f32,
    tone2_phase: f32,
    tone1_pitch: f32,      // current pitch of oscillator 1
    tone2_pitch: f32,      // current pitch of oscillator 2
    tone1_pitch_end: f32,
    tone2_pitch_end: f32,
    tone_pitch_decay: f32, // shared per-sample pitch decay factor
    tone_amp: f32,
    tone_amp_decay: f32,
    // Noise
    noise: Xorshift64,
    noise_amp: f32,
    noise_amp_decay: f32,
    noise_bp_low: f32,     // SVF band-pass state
    noise_bp_band: f32,
    // Transient crack (very short broadband burst for snappiness)
    crack_amp: f32,
    crack_amp_decay: f32,
    // Shared
    active: bool,
    sample_rate: f32,
}

impl Snare808 {
    pub fn new(sample_rate: f32, seed: u64) -> Self {
        Self {
            tone1_phase: 0.0,
            tone2_phase: 0.0,
            tone1_pitch: 80.0,
            tone2_pitch: 70.0,
            tone1_pitch_end: 80.0,
            tone2_pitch_end: 70.0,
            tone_pitch_decay: 0.993, // fast sweep — mostly settles in ~15ms
            tone_amp: 0.0,
            tone_amp_decay: (-1.0 / (sample_rate * 0.025_f32)).exp(), // ~25ms body
            noise: Xorshift64::new(seed),
            noise_amp: 0.0,
            noise_amp_decay: (-1.0 / (sample_rate * 0.03_f32)).exp(), // ~30ms noise tail
            noise_bp_low: 0.0,
            noise_bp_band: 0.0,
            crack_amp: 0.0,
            crack_amp_decay: (-1.0 / (sample_rate * 0.005_f32)).exp(), // ~5ms
            active: false,
            sample_rate,
        }
    }

    fn reset_phases(&mut self) {
        self.tone1_phase = 0.0;
        self.tone2_phase = 0.0;
        self.tone1_pitch = 200.0; // sweeps 200 → 80 Hz
        self.tone2_pitch = 175.0; // sweeps 175 → 70 Hz (detuned pair)
        self.noise_bp_low = 0.0;
        self.noise_bp_band = 0.0;
        self.active = true;
    }

    pub fn trigger(&mut self) {
        self.reset_phases();
        self.tone_amp = 0.15;
        self.noise_amp = 0.50;
        self.crack_amp = 0.3;
    }

    /// Ghost hit: half the amplitude of a full trigger.
    pub fn trigger_ghost(&mut self) {
        self.reset_phases();
        self.tone_amp = 0.12;
        self.noise_amp = 0.25;
        self.crack_amp = 0.15;
    }

    pub fn next_sample(&mut self) -> f32 {
        if !self.active {
            return 0.0;
        }

        // Dual sine body
        let t1 = (self.tone1_phase * std::f32::consts::TAU).sin();
        self.tone1_phase += self.tone1_pitch / self.sample_rate;
        if self.tone1_phase >= 1.0 { self.tone1_phase -= 1.0; }
        self.tone1_pitch = self.tone1_pitch_end
            + (self.tone1_pitch - self.tone1_pitch_end) * self.tone_pitch_decay;

        let t2 = (self.tone2_phase * std::f32::consts::TAU).sin();
        self.tone2_phase += self.tone2_pitch / self.sample_rate;
        if self.tone2_phase >= 1.0 { self.tone2_phase -= 1.0; }
        self.tone2_pitch = self.tone2_pitch_end
            + (self.tone2_pitch - self.tone2_pitch_end) * self.tone_pitch_decay;

        let tone = (t1 + t2) * 0.5 * self.tone_amp;
        self.tone_amp *= self.tone_amp_decay;

        // Bright band-passed noise (SVF, ~2 kHz centre)
        let white = self.noise.white();
        let f = (std::f32::consts::PI * 2000.0 / self.sample_rate).sin() * 2.0;
        let q = 0.4;
        let high = white - self.noise_bp_low - q * self.noise_bp_band;
        self.noise_bp_band += f * high;
        self.noise_bp_low += f * self.noise_bp_band;
        let snap = self.noise_bp_band * self.noise_amp;
        self.noise_amp *= self.noise_amp_decay;

        // Transient crack: broadband white noise burst, decays in ~5ms
        let crack = self.noise.white() * self.crack_amp;
        self.crack_amp *= self.crack_amp_decay;

        if self.tone_amp < 0.001 && self.noise_amp < 0.001 && self.crack_amp < 0.001 {
            self.active = false;
        }

        (tone + snap + crack).tanh()
    }
}

/// Hi-hat: band-passed noise burst with fast exponential decay.
/// Triggered by a pulse oscillator.
pub struct HiHat {
    noise: Xorshift64,
    pub amp: f32,
    peak_amp: f32,
    decay: f32,
    /// Band-pass state (simple 2-pole)
    bp_low: f32,
    bp_band: f32,
    bp_freq: f32,
    sample_rate: f32,
}

impl HiHat {
    pub fn new(sample_rate: f32, seed: u64) -> Self {
        Self {
            noise: Xorshift64::new(seed),
            amp: 0.0,
            peak_amp: 0.15,
            decay: (-1.0 / (sample_rate * 0.03_f32)).exp(), // ~30ms decay
            bp_low: 0.0,
            bp_band: 0.0,
            bp_freq: 8000.0,
            sample_rate,
        }
    }

    /// Closed hi-hat: very short decay, high BP frequency.
    pub fn new_closed(sample_rate: f32, seed: u64) -> Self {
        Self {
            noise: Xorshift64::new(seed),
            amp: 0.0,
            peak_amp: 0.009375, // 1/16 of open hat's 0.15
            decay: (-1.0 / (sample_rate * 0.008_f32)).exp(), // ~8ms decay
            bp_low: 0.0,
            bp_band: 0.0,
            bp_freq: 10000.0,
            sample_rate,
        }
    }

    /// Rim-shot variant: lower, clickier, shorter decay than a hat.
    pub fn new_rim(sample_rate: f32, seed: u64) -> Self {
        Self {
            noise: Xorshift64::new(seed),
            amp: 0.0,
            peak_amp: 0.09,
            decay: (-1.0 / (sample_rate * 0.015_f32)).exp(), // ~15ms decay
            bp_low: 0.0,
            bp_band: 0.0,
            bp_freq: 3000.0,
            sample_rate,
        }
    }

    pub fn trigger(&mut self) {
        self.amp = self.peak_amp;
    }

    pub fn next_sample(&mut self) -> f32 {
        if self.amp < 0.0001 {
            self.amp = 0.0;
            return 0.0;
        }

        let noise = self.noise.white();

        // Simple SVF band-pass for metallic character
        let f = (std::f32::consts::PI * self.bp_freq / self.sample_rate).sin() * 2.0;
        let q = 0.4;
        let high = noise - self.bp_low - q * self.bp_band;
        self.bp_band += f * high;
        self.bp_low += f * self.bp_band;
        let bp_out = self.bp_band;

        self.amp *= self.decay;

        bp_out * self.amp
    }
}

/// TR-808-style clave. Default frequency is 2500 Hz; use `trigger_with_note` to tune it.
pub struct ClaveVoice {
    phase: f32,
    amp: f32,
    freq: f32,
    sample_rate: f32,
}

impl ClaveVoice {
    pub fn new(sample_rate: f32) -> Self {
        Self { phase: 0.0, amp: 0.0, freq: 2500.0, sample_rate }
    }

    /// Trigger at a specific musical note, e.g. `"A6"`, `"C#5"`, `"Bb4"`.
    pub fn trigger_with_note(&mut self, note: &str) {
        self.freq = Self::note_to_freq(note);
        self.amp = 1.0;
        self.phase = 0.0;
    }

    /// Convert a note name ("A6", "C#5", "Bb4") to Hz via equal temperament (A4 = 440 Hz).
    pub fn note_to_freq(note: &str) -> f32 {
        let bytes = note.as_bytes();
        let semitone: i32 = match bytes[0] {
            b'C' => 0, b'D' => 2, b'E' => 4, b'F' => 5,
            b'G' => 7, b'A' => 9, b'B' => 11, _ => 9,
        };
        let (accidental, octave_idx) = match bytes.get(1) {
            Some(b'#') => (1i32, 2),
            Some(b'b') => (-1i32, 2),
            _ => (0i32, 1),
        };
        let octave = (bytes[octave_idx] - b'0') as i32;
        let midi = (octave + 1) * 12 + semitone + accidental;
        440.0 * 2.0_f32.powf((midi - 69) as f32 / 12.0)
    }

    pub fn next_sample(&mut self) -> f32 {
        let out = (self.phase * std::f32::consts::TAU).sin() * self.amp;
        self.phase += self.freq / self.sample_rate;
        if self.phase >= 1.0 { self.phase -= 1.0; }
        self.amp *= 0.993; // decays to ~5 % in 10 ms at 44100 Hz
        out
    }
}
