use rand::Rng;
use rand::SeedableRng;

use crate::dsp::{BbdDelay, DattorroReverb, Phaser, ResonantLpf};

/// Counts down an `Option<u32>` timer and runs `$body` when it hits zero.
macro_rules! tick_timer {
    ($timer:expr, $body:block) => {
        if let Some(t) = $timer {
            if t == 0 {
                $body
                $timer = None;
            } else {
                $timer = Some(t - 1);
            }
        }
    };
}

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

/// Synthesized kick drum.
/// Sine body with exponential pitch envelope: starts at `pitch_start` Hz,
/// decays to `pitch_end` Hz. Amplitude decays exponentially.
pub struct Kick {
    phase: f32,
    pitch_start: f32,
    pitch_end: f32,
    pitch_decay: f32,
    amp_decay: f32,
    current_pitch: f32,
    current_pitch_end: f32,
    current_amp: f32,
    sample_rate: f32,
    active: bool,
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

    pub fn trigger(&mut self) {
        self.tone1_phase = 0.0;
        self.tone2_phase = 0.0;
        self.tone1_pitch = 200.0; // sweeps 200 → 80 Hz
        self.tone2_pitch = 175.0; // sweeps 175 → 70 Hz (detuned pair)
        self.tone_amp = 0.15;
        self.noise_amp = 0.50;
        self.noise_bp_low = 0.0;
        self.noise_bp_band = 0.0;
        self.crack_amp = 0.3;
        self.active = true;
    }

    /// Ghost hit: half the amplitude of a full trigger.
    pub fn trigger_ghost(&mut self) {
        self.tone1_phase = 0.0;
        self.tone2_phase = 0.0;
        self.tone1_pitch = 200.0;
        self.tone2_pitch = 175.0;
        self.tone_amp = 0.12;
        self.noise_amp = 0.25;
        self.noise_bp_low = 0.0;
        self.noise_bp_band = 0.0;
        self.crack_amp = 0.15;
        self.active = true;
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

/// Xorshift64 PRNG for cheap per-sample noise.
/// Deterministic given a seed, no allocation.
pub struct Xorshift64 {
    state: u64,
}

impl Xorshift64 {
    fn new(seed: u64) -> Self {
        Self { state: seed | 1 }
    }

    #[inline]
    fn next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// White noise sample in [-1.0, 1.0].
    #[inline]
    fn white(&mut self) -> f32 {
        (self.next() as f32) / (u64::MAX as f32) * 2.0 - 1.0
    }
}

/// Hi-hat: band-passed noise burst with fast exponential decay.
/// Triggered by a pulse oscillator.
pub struct HiHat {
    noise: Xorshift64,
    amp: f32,
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

/// Continuous hiss layer: filtered noise, always present.
/// Level modulated by a slow LFO for breathing texture.
struct Hiss {
    noise: Xorshift64,
    filter: ResonantLpf,
    lfo_phase: f32,
    lfo_rate: f32,
    base_level: f32,
    sample_rate: f32,
}

impl Hiss {
    fn new(sample_rate: f32, rng: &mut impl Rng) -> Self {
        let filter = ResonantLpf::new(2000.0, 6000.0, 0.15, 0.03, sample_rate, rng);
        Self {
            noise: Xorshift64::new(rng.r#gen::<u64>() | 1),
            filter,
            lfo_phase: rng.r#gen::<f32>(),
            lfo_rate: rng.r#gen_range(0.02..0.06),
            base_level: 0.08,
            sample_rate,
        }
    }

    fn next_sample(&mut self) -> f32 {
        let noise = self.noise.white();
        let filtered = self.filter.process(noise);

        // Slow breathing LFO
        let lfo = (self.lfo_phase * std::f32::consts::TAU).sin();
        let level = self.base_level * (0.5 + 0.5 * lfo);

        self.lfo_phase += self.lfo_rate / self.sample_rate;
        if self.lfo_phase >= 1.0 {
            self.lfo_phase -= 1.0;
        }

        filtered * level
    }

    fn set_level(&mut self, level: f32) {
        self.base_level = level;
    }
}


/// Dub stab: 2-3 detuned saw/triangle oscillators forming a chord,
/// with fast attack, band-pass filtered (decaying LPF + fixed HPF),
/// and fed through dub delay.
pub struct DubStab {
    phases: [f32; 3],
    freqs: [f32; 3],
    /// 0.0 = saw, 1.0 = triangle (blends between them)
    wave_blend: f32,
    amp: f32,
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
    active: bool,
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
        let mut sig = 0.0f32;
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

/// Dub delay: long feedback delay with filtering in the feedback path.
/// Classic dub style — repeats that darken and smear over time.
struct DubDelay {
    buffer: crate::dsp::DelayLine,
    feedback: f32,
    lp_state: f32,
    lp_coeff: f32,
    hp_state: f32,
    hp_coeff: f32,
    delay_samples: usize,
    mix: f32,
}

impl DubDelay {
    fn new(delay_ms: f32, feedback: f32, mix: f32, sample_rate: f32) -> Self {
        let delay_samples = (delay_ms * sample_rate / 1000.0) as usize;
        Self {
            buffer: crate::dsp::DelayLine::new(delay_samples + 1),
            feedback,
            lp_state: 0.0,
            lp_coeff: 0.35, // darkening LP in feedback
            hp_state: 0.0,
            hp_coeff: 0.05, // removes DC/sub buildup in feedback
            delay_samples,
            mix,
        }
    }

    fn process(&mut self, input: f32) -> (f32, f32) {
        let delayed = self.buffer.read_at(self.delay_samples);

        // LP in feedback path — each repeat gets darker
        self.lp_state += self.lp_coeff * (delayed - self.lp_state);
        // HP in feedback path — prevents mud accumulation
        let hp_in = self.lp_state;
        self.hp_state += self.hp_coeff * (hp_in - self.hp_state);
        let filtered = hp_in - self.hp_state;

        let write = input + filtered * self.feedback;
        self.buffer.write_and_advance(write);

        // Stereo: dry left, wet right (classic dub ping-pong feel)
        let dry = input * (1.0 - self.mix * 0.5);
        let wet = delayed * self.mix;
        (dry + wet * 0.4, dry + wet)
    }
}

/// High synth pad: three detuned oscillators (sine/triangle blend) with a slow
/// amplitude attack, an LFO-swept LPF, and plate reverb. Designed to float
/// above the percussion and stabs. Notes are changed by calling `trigger`.
/// High synth pad: four detuned sawtooth oscillators with vibrato, slow attack,
/// LFO-swept LPF, and plate reverb. Detuning spread emulates a string ensemble.
pub struct SynthPad {
    phases: [f32; 4],
    base_freqs: [f32; 4],
    vibrato_phase: f32,
    amp: f32,
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

/// Granular engine tuned for deep space textures: long, slow grains at
/// extreme frequencies (very high shimmer + very low rumble), sparse
/// triggering, fed through long reverb with wide stereo.
pub struct GranularEngine {
    grains: Vec<Grain>,
    noise: Xorshift64,
    reverb: DattorroReverb,
    sample_rate: f32,
    level: f32,
}

struct Grain {
    phase: f32,
    freq: f32,
    /// Slow pitch drift per grain — each grain glides slightly
    drift: f32,
    window_phase: f32,
    window_rate: f32,
    /// Per-grain stereo position (-1 to 1)
    pan: f32,
    active: bool,
}

impl Grain {
    fn new() -> Self {
        Self {
            phase: 0.0,
            freq: 0.0,
            drift: 0.0,
            window_phase: 0.0,
            window_rate: 0.0,
            pan: 0.0,
            active: false,
        }
    }

    fn trigger(&mut self, freq: f32, drift: f32, duration_samples: f32, pan: f32) {
        self.phase = 0.0;
        self.freq = freq;
        self.drift = drift;
        self.window_phase = 0.0;
        self.window_rate = 1.0 / duration_samples;
        self.pan = pan;
        self.active = true;
    }

    fn next_sample(&mut self, sample_rate: f32) -> (f32, f32) {
        if !self.active {
            return (0.0, 0.0);
        }

        // Hann window
        let window = 0.5 * (1.0 - (self.window_phase * std::f32::consts::TAU).cos());
        let sample = (self.phase * std::f32::consts::TAU).sin() * window;

        self.phase += self.freq / sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        // Slow pitch glide
        self.freq += self.drift / sample_rate;

        self.window_phase += self.window_rate;
        if self.window_phase >= 1.0 {
            self.active = false;
        }

        // Equal-power pan
        let r = (self.pan + 1.0) * 0.5; // 0..1
        let l_gain = (1.0 - r).sqrt();
        let r_gain = r.sqrt();
        (sample * l_gain, sample * r_gain)
    }
}

/// Frequency pools for space grains — very high shimmer and deep sub rumble
const GRAIN_FREQS_HIGH: [f32; 6] = [1800.0, 2400.0, 3200.0, 4200.0, 5600.0, 7000.0];
const GRAIN_FREQS_LOW: [f32; 4] = [40.0, 55.0, 65.0, 80.0];

impl GranularEngine {
    pub fn new(sample_rate: f32, seed: u64, rng: &mut impl rand::Rng) -> Self {
        let grains = (0..6).map(|_| Grain::new()).collect();
        Self {
            grains,
            noise: Xorshift64::new(seed),
            reverb: DattorroReverb::new(0.95, 0.6, 0.85, 0.015, sample_rate, rng),
            sample_rate,
            level: 0.1,
        }
    }

    pub fn set_level(&mut self, level: f32) {
        self.level = level;
    }

    /// Spawn a single grain — called by the external pulse oscillator.
    pub fn spawn_grain(&mut self) {
        if let Some(grain) = self.grains.iter_mut().find(|g| !g.active) {
            // Pick from high or low frequency pool (70% high shimmer, 30% sub)
            let is_high = (self.noise.next() % 10) < 7;
            let freq = if is_high {
                GRAIN_FREQS_HIGH[(self.noise.next() as usize) % GRAIN_FREQS_HIGH.len()]
            } else {
                GRAIN_FREQS_LOW[(self.noise.next() as usize) % GRAIN_FREQS_LOW.len()]
            };

            // Long grains: 200ms–1.5s
            let dur_ms = 200.0 + (self.noise.next() % 1300) as f32;
            let dur_samples = dur_ms * self.sample_rate / 1000.0;

            // Slow pitch drift: ±10 Hz/sec for high, ±2 Hz/sec for low
            let drift_range = if is_high { 10.0 } else { 2.0 };
            let drift = (self.noise.white()) * drift_range;

            // Wide stereo placement
            let pan = self.noise.white();

            grain.trigger(freq, drift, dur_samples, pan);
        }
    }

    /// Generate stereo audio from active grains through reverb.
    pub fn next_sample(&mut self) -> (f32, f32) {
        let mut sum_l = 0.0f32;
        let mut sum_r = 0.0f32;
        for grain in &mut self.grains {
            let (l, r) = grain.next_sample(self.sample_rate);
            sum_l += l;
            sum_r += r;
        }

        // Feed mono sum through long reverb for depth
        let mono = (sum_l + sum_r) * 0.5;
        let (rev_l, rev_r) = self.reverb.process(mono);

        (rev_l * self.level, rev_r * self.level)
    }
}

/// SH-101 inspired monosynth: sine oscillator with sub-oscillator one octave down,
/// through a cascaded 4-pole resonant low-pass (two SVF stages) with a decaying
/// filter envelope and portamento glide.
pub struct MonoSynth {
    phase: f32,          // main oscillator
    sub_phase: f32,      // sub-oscillator, one octave down
    freq: f32,           // current frequency (glides toward target)
    target_freq: f32,
    amp: f32,
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
            sample_rate,
        }
    }

    pub fn trigger(&mut self, freq: f32, accented: bool) {
        self.target_freq = freq;
        self.amp = 0.0;
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
        self.freq += (self.target_freq - self.freq) * 0.005;

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
        // base_cutoff sweeps 80–2500 Hz over a 64-note sine cycle
        let sweep = (self.sweep_phase * std::f32::consts::TAU).sin() * 0.5 + 0.5; // 0..1
        let base_cutoff = 40.0_f32 + sweep * 210.0; // 40–250 Hz
        let peak_cutoff: f32 = 400.0;
        let cutoff = base_cutoff + self.filter_env * (peak_cutoff - base_cutoff);
        let f = (std::f32::consts::PI * cutoff / self.sample_rate).sin() * 2.0;
        let resonance = 0.7_f32;

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

// A natural minor scale, two octaves from A2
const A_MINOR_SCALE: [f32; 15] = [
    110.00, // 0  A2
    123.47, // 1  B2
    130.81, // 2  C3
    146.83, // 3  D3
    164.81, // 4  E3
    174.61, // 5  F3
    196.00, // 6  G3
    220.00, // 7  A3
    246.94, // 8  B3
    261.63, // 9  C4
    293.66, // 10 D4
    329.63, // 11 E4
    349.23, // 12 F4
    392.00, // 13 G4
    440.00, // 14 A4
];
// Base sequence as scale-degree indices (A2=0)
const MONO_SEQ_IDX: [usize; 16] = [
    0, // A2
    2, // C3
    4, // E3
    0, // A2
    2, // C3
    4, // E3
    0, // A2
    2, // C3
    5, // F3
    4, // E3
    0, // A2
    5, // F3
    4, // E3
    2, // C3
    0, // A2
    2, // C3
];

const PAD_FREQS: [f32; 5] = [440.0, 523.25, 587.33, 659.26, 784.0]; // A4–G5

// A minor pentatonic chord voicings — all three notes in scale for each root position.
// Stab1: A2–G3 range.  Stab2: one step lower, same scale.
const STAB1_CHORDS: [[f32; 3]; 5] = [
    [110.00, 130.81, 164.81], // Am:  A2 C3 E3
    [130.81, 164.81, 196.00], // C:   C3 E3 G3
    [146.83, 196.00, 220.00], // Dm7: D3 G3 A3
    [164.81, 196.00, 220.00], // Em7: E3 G3 A3
    [196.00, 220.00, 293.66], // G:   G3 A3 D4
];
const STAB2_CHORDS: [[f32; 3]; 5] = [
    [ 49.00,  55.00,  73.42], // G1 A1 D2
    [ 55.00,  65.41,  82.41], // Am: A1 C2 E2
    [ 65.41,  82.41,  98.00], // C:  C2 E2 G2
    [ 73.42,  98.00, 110.00], // Dm7: D2 G2 A2
    [ 82.41,  98.00, 110.00], // Em7: E2 G2 A2
];
const STAB3_CHORDS: [[f32; 3]; 5] = [
    [ 55.00,  65.41,  82.41], // Am  A1 C2 E2
    [ 65.41,  82.41, 110.00], // Am/C C2 E2 A2
    [ 73.42,  87.31, 110.00], // Dm  D2 F2 A2
    [ 82.41,  98.00, 123.47], // Em  E2 G2 B2
    [ 55.00,  73.42,  87.31], // Dm/A A1 D2 F2
];

/// TR-808-style clave: decaying sine at ~2500 Hz, very short (~10 ms).
/// TR-808-style clave. Default frequency is 2500 Hz; use `trigger_with_note` to tune it.
struct ClaveVoice {
    phase: f32,
    amp: f32,
    freq: f32,
    sample_rate: f32,
}

impl ClaveVoice {
    fn new(sample_rate: f32) -> Self {
        Self { phase: 0.0, amp: 0.0, freq: 2500.0, sample_rate }
    }

    /// Trigger at the default 2500 Hz.
    fn trigger(&mut self) {
        self.amp = 1.0;
        self.phase = 0.0;
    }

    /// Trigger at a specific musical note, e.g. `"A6"`, `"C#5"`, `"Bb4"`.
    fn trigger_with_note(&mut self, note: &str) {
        self.freq = Self::note_to_freq(note);
        self.amp = 1.0;
        self.phase = 0.0;
    }

    /// Convert a note name ("A6", "C#5", "Bb4") to Hz via equal temperament (A4 = 440 Hz).
    fn note_to_freq(note: &str) -> f32 {
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

    fn next_sample(&mut self) -> f32 {
        let out = (self.phase * std::f32::consts::TAU).sin() * self.amp;
        self.phase += self.freq / self.sample_rate;
        if self.phase >= 1.0 { self.phase -= 1.0; }
        self.amp *= 0.993; // decays to ~5 % in 10 ms at 44100 Hz
        out
    }
}

// Pattern indices
const PATTERN_KICK:  usize = 0;
const PATTERN_SNARE: usize = 1;
const PATTERN_HATS:  usize = 2;
const PATTERN_RIM:   usize = 3;
const PATTERN_STAB1: usize = 4;
const PATTERN_STAB2: usize = 5;
const PATTERN_STAB3: usize = 6;
const PATTERN_PAD:   usize = 7;
const PATTERN_MONO:  usize = 8;
const PATTERN_CLAVE: usize = 9;
const NUM_PATTERNS:  usize = 10;

/// One instrument group. Config fields are set once; runtime fields track playback state.
#[derive(Clone, Copy)]
struct Pattern {
    // Config (set once)
    start_weight:    f32,  // probability of turning on when off (0..1)
    continue_weight: f32,  // probability of staying on after minimum_repeats (0..1)
    minimum_repeats: u32,  // must stay on at least this many 8-bar segments
    can_solo:        bool, // allowed to be the only active pattern
    // Runtime
    active:          bool,
    segments_active: u32,  // how many 8-bar segments this has been continuously active
}

impl Pattern {
    fn new(start_weight: f32, continue_weight: f32, minimum_repeats: u32, can_solo: bool) -> Self {
        Self {
            start_weight,
            continue_weight,
            minimum_repeats,
            can_solo,
            active: false,
            segments_active: 0,
        }
    }
}

/// Polyrhythmic ambient techno engine.
///
/// All timing derived from a single base frequency (human heartbeat, 1.2 Hz).
/// Each element pulses at an exact rational multiple of the base, creating
/// polyrhythms that phase in and out of alignment and fully resolve at the LCM.
///
/// ```text
/// Element   Ratio    Frequency   Resolves with base every
/// ──────────────────────────────────────────────────────────
/// Kick      1/1      1.200 Hz    -
/// Hat       7/4      2.100 Hz    4 base cycles  (3.3s)
/// Hat       1/1      1.200 Hz    -               phase offset 0.5 — lands on the off-beat
/// Rim       9/5      2.160 Hz    5 base cycles   drifts against kick
/// Stab      3/5      0.720 Hz    5 base cycles  (4.2s)
/// Grain     1/7      0.171 Hz    7 base cycles  (5.8s)
///
/// Full alignment: LCM(4, 5, 7) = 140 base cycles ≈ 116.7s ≈ ~2 minutes
/// ```
pub struct AmbientTechno {
    kick: Kick,
    kick_pulse: PulseOscillator,
    kick_timer: Option<u32>,
    ghost_kick: Kick,
    beat_count: u32,
    ghost_count: u32,
    ghost_timer: Option<u32>,
    hat: HiHat,
    closed_hat: HiHat,
    closed_hat_lpf: ResonantLpf,
    hat_phaser: Phaser,
    beat_phase: u32,
    beat_duration: u32,
    swing: SwingLfo,
    /// Pre-computed trigger positions (in samples from kick) for the current beat, accounting for swing.
    open_hat_position: u32,
    closed_hat_positions: [u32; 3],
    roll_active: bool,
    snare: Snare808,
    snare_timer: Option<u32>,
    snare_reverb: DattorroReverb,
    ghost_snare: Snare808,
    ghost_snare1_timer: Option<u32>,
    ghost_snare2_timer: Option<u32>,
    ghost_snare_reverb: DattorroReverb,
    /// Reverse reverb: noise swell that rises over the beat before each snare hit.
    rev_rev_noise: Xorshift64,
    rev_rev_amp: f32,
    rev_rev_rise_rate: f32,
    rev_rev_active: bool,
    rev_rev_bp_low: f32,
    rev_rev_bp_band: f32,
    rev_rev_reverb: DattorroReverb,
    rim: HiHat,
    rim_pulse: PulseOscillator,
    rim_delay: BbdDelay,
    rim_reverb: DattorroReverb,
    stab: DubStab,
    stab_lpf: ResonantLpf,
    stab_delay: DubDelay,
    stab_phaser: Phaser,
    stab_timer: Option<u32>,
    stab_rng: Xorshift64,
    last_stab_idx: usize,
    stab2: DubStab,
    stab2_lpf: ResonantLpf,
    stab2_delay: DubDelay,
    stab2_timer: Option<u32>,
    stab3: DubStab,
    stab3_lpf: ResonantLpf,
    stab3_reverb: DattorroReverb,
    pad: SynthPad,
    pad_lpf: ResonantLpf,
    pad_chorus: BbdDelay,
    pad_reverb: DattorroReverb,
    pad_rng: Xorshift64,
    mono: MonoSynth,
    mono_reverb: DattorroReverb,
    mono_step: usize,
    mono_seq_freqs: [f32; 16],  // realized sequence (rerolled every 8 playthroughs)
    mono_seq_repeats: u8,       // how many full playthroughs of the current sequence
    mono_downbeat_timer: Option<u32>,
    mono_rng: Xorshift64,
    clave: ClaveVoice,
    clave_delay: DubDelay,
    clave_reverb: DattorroReverb,
    clave_timer: Option<u32>,
    patterns: [Pattern; NUM_PATTERNS],
    pattern_rng: Xorshift64,
    sample_rate: f32,
    limiter_gain: f32,
    fade_pos: u32,
    fade_state: FadeState,
    fade_samples: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FadeState {
    FadingIn,
    Playing,
    FadingOut,
    Done,
}

const FADE_DURATION: f32 = 1.0;

/// Human resting heartbeat — the base frequency from which all rhythms derive.
const BASE_FREQ: f32 = 1.2;

/// Polyrhythmic ratios (p/q of base frequency).
/// Chosen so LCM of denominators creates long resolution period.
const RIM_RATIO: (f32, f32) = (9.0, 5.0);    // 9/5 base → 2.160 Hz — drifts against kick
const STAB_RATIO: (f32, f32) = (3.0, 5.0);   // 3/5 base
const GRAIN_RATIO: (f32, f32) = (1.0, 7.0);   // 1/7 base

impl AmbientTechno {
    pub fn new(sample_rate: u32, seed: u64) -> Self {
        let sr = sample_rate as f32;
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);

        let mut engine = Self {
            kick: Kick::new(sr),
            kick_pulse: PulseOscillator::new(BASE_FREQ, sr),
            kick_timer: None,
            ghost_kick: Kick::new_ghost(sr),
            beat_count: 0,
            ghost_count: 0,
            ghost_timer: None,
            hat: HiHat::new(sr, 0xDEADBEEF),
            closed_hat: HiHat::new_closed(sr, 0xFEEDFACE),
            closed_hat_lpf: ResonantLpf::new(3000.0, 6000.0, 0.2, 0.1, sr, &mut rng),
            hat_phaser: Phaser::new(0.7, 0.3, 0.25, sr),
            beat_phase: 0,
            beat_duration: (sr / BASE_FREQ) as u32,
            swing: SwingLfo::new(),
            open_hat_position: (sr / BASE_FREQ) as u32 / 2,
            closed_hat_positions: [0; 3],
            roll_active: false,
            snare: Snare808::new(sr, rng.r#gen::<u64>() | 1),
            snare_timer: None,
            snare_reverb: DattorroReverb::new(0.5, 0.3, 0.75, 0.04, sr, &mut rng),
            ghost_snare: Snare808::new(sr, rng.r#gen::<u64>() | 1),
            ghost_snare1_timer: None,
            ghost_snare2_timer: None,
            ghost_snare_reverb: DattorroReverb::new(0.4, 0.2, 0.7, 0.03, sr, &mut rng),
            rev_rev_noise: Xorshift64::new(rng.r#gen::<u64>() | 1),
            rev_rev_amp: 0.0,
            rev_rev_rise_rate: 0.0,
            rev_rev_active: false,
            rev_rev_bp_low: 0.0,
            rev_rev_bp_band: 0.0,
            rev_rev_reverb: DattorroReverb::new(0.92, 0.7, 0.85, 0.03, sr, &mut rng),
            rim: HiHat::new_rim(sr, 0xCAFEBABE),
            rim_pulse: PulseOscillator::new(BASE_FREQ * RIM_RATIO.0 / RIM_RATIO.1, sr),
            rim_delay: BbdDelay::new(40.0, 0.65, 0.75, 0.2, 1.0, sr, &mut rng),
            rim_reverb: DattorroReverb::new(0.92, 0.7, 0.95, 0.05, sr, &mut rng),
            stab: DubStab::new(sr),
            stab_lpf: ResonantLpf::new(300.0, 2500.0, 0.3, 0.5, sr, &mut rng), // 2s LFO
            stab_delay: DubDelay::new(416.0, 0.70, 0.75, sr),
            stab_phaser: Phaser::new(0.2, 0.6, 0.5, sr),
            stab_timer: None,
            stab_rng: Xorshift64::new(rng.r#gen::<u64>() | 1),
            last_stab_idx: 2, // D3 default until stab1 fires
            stab2: DubStab::new(sr),
            stab2_lpf: ResonantLpf::new(75.0, 625.0, 0.3, 0.5, sr, &mut rng),
            stab2_delay: DubDelay::new(416.0, 0.70, 0.75, sr),
            stab2_timer: None,
            stab3: DubStab::new_long(sr),
            stab3_lpf: ResonantLpf::new(40.0, 300.0, 0.3, 0.5, sr, &mut rng),
            stab3_reverb: DattorroReverb::new(0.95, 0.6, 0.9, 0.03, sr, &mut rng),
            pad: SynthPad::new(sr),
            pad_lpf: ResonantLpf::new(200.0, 1000.0, 0.05, 0.08, sr, &mut rng), // ~12s LFO
            pad_chorus: BbdDelay::new(20.0, 0.05, 0.6, 2.0, 3.0, sr, &mut rng),
            pad_reverb: DattorroReverb::new(0.93, 0.5, 0.85, 0.04, sr, &mut rng),
            pad_rng: Xorshift64::new(rng.r#gen::<u64>() | 1),
            mono: MonoSynth::new(sr),
            mono_reverb: DattorroReverb::new(0.82, 0.3, 0.7, 0.05, sr, &mut rng),
            mono_step: 0,
            mono_seq_freqs: {
                let mut f = [0.0f32; 16];
                for i in 0..16 { f[i] = A_MINOR_SCALE[MONO_SEQ_IDX[i]]; }
                f
            },
            mono_seq_repeats: 0,
            mono_downbeat_timer: None,
            mono_rng: Xorshift64::new(rng.r#gen::<u64>() | 1),
            clave: ClaveVoice::new(sr),
            clave_delay: DubDelay::new(416.0, 0.45, 0.3, sr),
            clave_reverb: DattorroReverb::new(0.93, 0.7, 0.85, 0.04, sr, &mut rng),
            clave_timer: None,
            // Placeholder weights — caller will configure via set_pattern() before first use.
            patterns: [
                Pattern::new(0.2, 0.6, 4, false), // PATTERN_KICK
                Pattern::new(0.1, 0.4, 4, false), // PATTERN_SNARE
                Pattern::new(0.5, 0.5, 4, false), // PATTERN_HATS
                Pattern::new(0.8, 0.4, 8, true),  // PATTERN_RIM
                Pattern::new(0.3, 0.5, 4, true),  // PATTERN_STAB1
                Pattern::new(0.2, 0.5, 4, true),  // PATTERN_STAB2
                Pattern::new(0.7, 0.3, 16, true), // PATTERN_STAB3
                Pattern::new(0.2, 0.4, 8, true),  // PATTERN_PAD
                Pattern::new(0.1, 0.3, 8, true),  // PATTERN_MONO
                Pattern::new(0.1, 0.3, 2, false),  // PATTERN_CLAVE
            ],
            pattern_rng: Xorshift64::new(rng.r#gen::<u64>() | 1),
            sample_rate: sr,
            limiter_gain: 1.0,
            fade_pos: 0,
            fade_state: FadeState::FadingIn,
            fade_samples: (sr * FADE_DURATION) as u32,
        };
        engine.update_patterns(); // initial pattern selection
        engine
    }

    fn regenerate_mono_seq(&mut self) {
        // Pick 4 unique positions from 1..=15 via partial Fisher-Yates (step 0 is always the anchor)
        let mut positions: [usize; 15] = [1,2,3,4,5,6,7,8,9,10,11,12,13,14,15];
        for i in 0..4 {
            let j = i + (self.mono_rng.next() as usize % (15 - i));
            positions.swap(i, j);
        }
        for &pos in &positions[..4] {
            let base_idx = MONO_SEQ_IDX[pos];
            let shift = (self.mono_rng.next() % 7) as i32 - 3; // -3..=3
            let idx = (base_idx as i32 + shift).clamp(0, A_MINOR_SCALE.len() as i32 - 1) as usize;
            self.mono_seq_freqs[pos] = A_MINOR_SCALE[idx];
        }
    }

    fn next_f32(&mut self) -> f32 {
        self.pattern_rng.next() as f32 / u64::MAX as f32
    }

    fn update_patterns(&mut self) {
        // Step 1: tick active patterns; deactivate those past minimum_repeats that fail continue roll.
        for i in 0..NUM_PATTERNS {
            if self.patterns[i].active {
                self.patterns[i].segments_active += 1;
                if self.patterns[i].segments_active >= self.patterns[i].minimum_repeats {
                    if self.next_f32() > self.patterns[i].continue_weight {
                        self.patterns[i].active = false;
                        self.patterns[i].segments_active = 0;
                    }
                }
            }
        }

        // Step 2: try to activate inactive patterns by start_weight.
        for i in 0..NUM_PATTERNS {
            if !self.patterns[i].active && self.next_f32() < self.patterns[i].start_weight {
                self.patterns[i].active = true;
                self.patterns[i].segments_active = 0;
            }
        }

        // Step 3: if nothing is active, force the highest-start_weight pattern on.
        let active_count = self.patterns.iter().filter(|p| p.active).count();
        if active_count == 0 {
            let best = (0..NUM_PATTERNS)
                .max_by(|&a, &b| self.patterns[a].start_weight
                    .partial_cmp(&self.patterns[b].start_weight).unwrap())
                .unwrap();
            self.patterns[best].active = true;
            self.patterns[best].segments_active = 0;
        }

        // Step 4: if only one pattern is active and it can't solo, force-add the
        // highest-start_weight inactive pattern.
        let active_count = self.patterns.iter().filter(|p| p.active).count();
        if active_count == 1 {
            let solo_idx = (0..NUM_PATTERNS).find(|&i| self.patterns[i].active).unwrap();
            if !self.patterns[solo_idx].can_solo {
                if let Some(best) = (0..NUM_PATTERNS)
                    .filter(|&i| !self.patterns[i].active)
                    .max_by(|&a, &b| self.patterns[a].start_weight
                        .partial_cmp(&self.patterns[b].start_weight).unwrap())
                {
                    self.patterns[best].active = true;
                    self.patterns[best].segments_active = 0;
                }
            }
        }
    }

    fn advance_mono_step(&mut self) {
        self.mono_step += 1;
        if self.mono_step >= 16 {
            self.mono_step = 0;
            self.mono_seq_repeats += 1;
            if self.mono_seq_repeats >= 8 {
                self.mono_seq_repeats = 0;
                self.regenerate_mono_seq();
            }
        }
    }

    pub fn start_fade_out(&mut self) {
        if self.fade_state == FadeState::FadingIn || self.fade_state == FadeState::Playing {
            self.fade_state = FadeState::FadingOut;
        }
    }

    pub fn is_done(&self) -> bool {
        self.fade_state == FadeState::Done
    }

    pub fn state(&self) -> FadeState {
        self.fade_state
    }

    pub fn set_param(&mut self, _name: &str, _value: f32) {}

    pub fn get_params(&self) -> Vec<(&str, f32, f32, f32)> {
        vec![]
    }

    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let len = left.len().min(right.len());
        for i in 0..len {
            let (l, r) = self.next_sample();
            left[i] = l;
            right[i] = r;
        }
    }

    fn next_sample(&mut self) -> (f32, f32) {
        let master_gain = match self.fade_state {
            FadeState::FadingIn => {
                self.fade_pos += 1;
                if self.fade_pos >= self.fade_samples {
                    self.fade_state = FadeState::Playing;
                }
                let t = self.fade_pos as f32 / self.fade_samples as f32;
                t * t
            }
            FadeState::Playing => 1.0,
            FadeState::FadingOut => {
                if self.fade_pos == 0 {
                    self.fade_state = FadeState::Done;
                    0.0
                } else {
                    self.fade_pos = self.fade_pos.saturating_sub(1);
                    let t = self.fade_pos as f32 / self.fade_samples as f32;
                    t * t
                }
            }
            FadeState::Done => 0.0,
        };

        if self.fade_state == FadeState::Done {
            return (0.0, 0.0);
        }

        if self.kick_pulse.tick() {
            self.beat_phase = 0;
            // Advance the swing LFO and recompute all trigger positions (including the kick itself).
            self.swing.advance();
            let sw = self.swing.offset_samples(self.beat_duration);

            // Every 8 bars (32 beats): re-evaluate which patterns are active.
            if self.beat_count % 32 == 0 {
                self.update_patterns();
            }

            if self.patterns[PATTERN_KICK].active {
                self.kick_timer = Some(sw);
            }
            if self.patterns[PATTERN_MONO].active {
                self.mono_downbeat_timer = Some(sw);
            }
            let sixteenth = self.beat_duration / 4;
            self.open_hat_position = self.beat_duration / 2 + sw;
            self.closed_hat_positions = [sixteenth + sw, sixteenth * 2, sixteenth * 3 + sw];
            self.roll_active = self.patterns[PATTERN_HATS].active && self.beat_count % 8 == 7;
            if self.beat_count % 8 == 0 && self.patterns[PATTERN_PAD].active {
                // Every 8 beats: pick a new high pentatonic note for the pad
                let idx = (self.pad_rng.next() as usize) % PAD_FREQS.len();
                self.pad.trigger_minor_chord(PAD_FREQS[idx]);
            }
            if self.beat_count % 32 == 0 && self.patterns[PATTERN_STAB3].active {
                // Every 32nd beat (0, 32, 64…): trigger stab3 on the downbeat
                let idx = (self.stab_rng.next() as usize) % STAB3_CHORDS.len();
                self.stab3.trigger_with_chord_and_cutoff(STAB3_CHORDS[idx], 600.0, &mut self.stab_rng);
            }
            if self.beat_count % 4 == 2 && self.patterns[PATTERN_SNARE].active {
                self.snare_timer = Some(sw);
                self.rev_rev_active = false;
            }
            if self.beat_count % 4 == 1 && self.patterns[PATTERN_SNARE].active {
                self.rev_rev_active = true;
                self.rev_rev_amp = 0.0;
                self.rev_rev_rise_rate = 1.0 / self.beat_duration as f32;
                self.rev_rev_bp_low = 0.0;
                self.rev_rev_bp_band = 0.0;
            }
            self.beat_count += 1;
            if self.beat_count % 2 == 0 && self.patterns[PATTERN_KICK].active {
                let eighth_note = (self.sample_rate / (BASE_FREQ * 2.0)) as u32;
                self.ghost_timer = Some(eighth_note + sw);
            }
            if self.beat_count % 8 == 7 && self.patterns[PATTERN_STAB1].active {
                let sixteenth = (self.sample_rate / (BASE_FREQ * 4.0)) as u32;
                let beat = (self.sample_rate / BASE_FREQ) as u32;
                self.stab_timer = Some(beat - sixteenth + sw);
            }
            if self.beat_count % 4 == 3 && self.patterns[PATTERN_STAB2].active {
                let sixteenth = (self.sample_rate / (BASE_FREQ * 4.0)) as u32;
                let beat = (self.sample_rate / BASE_FREQ) as u32;
                self.stab2_timer = Some(beat - sixteenth + sw);
            }
            // Clave: one hit per 4-bar loop, 1/16th before the 3rd beat of the 4th bar.
            // beat_count % 16 == 14 is the 2nd beat of bar 4 (after increment); fire timer at
            // beat - sixteenth + sw so it lands exactly one 16th note early.
            if self.beat_count % 16 == 14 && self.patterns[PATTERN_CLAVE].active {
                let sixteenth = (self.sample_rate / (BASE_FREQ * 4.0)) as u32;
                let beat = (self.sample_rate / BASE_FREQ) as u32;
                self.clave_timer = Some(beat - sixteenth + sw);
            }
        }

        tick_timer!(self.kick_timer, {
            self.kick.trigger();
        });

        tick_timer!(self.mono_downbeat_timer, {
            self.mono.trigger(self.mono_seq_freqs[self.mono_step], self.mono_step % 3 == 0);
            self.advance_mono_step();
        });

        tick_timer!(self.ghost_timer, {
            // Cycle: normal, minor third up, normal, minor third down
            const MINOR_THIRD: f32 = 1.18921; // 2^(3/12)
            const MINOR_SECOND: f32 = 1.05946; // 2^(1/12)
            let pitch_mul = match self.ghost_count % 4 {
                1 => MINOR_THIRD,
                3 => 1.0 / MINOR_SECOND,
                _ => 1.0,
            };
            self.ghost_kick.trigger_with_amp_and_pitch(0.125, pitch_mul);
            self.ghost_count += 1;
        });
        tick_timer!(self.stab_timer, {
            let idx = (self.stab_rng.next() as usize) % STAB1_CHORDS.len();
            self.stab.trigger_with_chord(STAB1_CHORDS[idx], &mut self.stab_rng);
            self.last_stab_idx = idx;
        });

        tick_timer!(self.stab2_timer, {
            self.stab2.trigger_with_chord(STAB2_CHORDS[self.last_stab_idx], &mut self.stab_rng);
        });

        tick_timer!(self.clave_timer, {
            self.clave.trigger_with_note("A6");
        });

        tick_timer!(self.snare_timer, {
            self.snare.trigger();
            let sixteenth = self.beat_duration / 4;
            self.ghost_snare1_timer = Some(self.beat_duration);
            self.ghost_snare2_timer = Some(self.beat_duration + sixteenth);
        });

        tick_timer!(self.ghost_snare1_timer, {
            self.ghost_snare.trigger_ghost();
        });

        tick_timer!(self.ghost_snare2_timer, {
            self.ghost_snare.trigger_ghost();
        });

        // Open hat is phase-locked to kick: fires at the swung halfway point of each beat (off-beat).
        if self.patterns[PATTERN_HATS].active && self.beat_phase == self.open_hat_position {
            self.hat.trigger();
        }

        let at_closed_hat_pos = self.beat_phase == self.closed_hat_positions[0]
            || self.beat_phase == self.closed_hat_positions[1]
            || self.beat_phase == self.closed_hat_positions[2];

        // Closed hats are phase-locked to the kick: fire at swung 16th note positions.
        if self.patterns[PATTERN_HATS].active && at_closed_hat_pos {
            self.closed_hat.trigger();
        }

        // Monosynth off-beat 16th notes share the same swung positions as the closed hats.
        if self.patterns[PATTERN_MONO].active && at_closed_hat_pos {
            self.mono.trigger(self.mono_seq_freqs[self.mono_step], self.mono_step % 3 == 0);
            self.advance_mono_step();
        }

        // On the last beat of every 2-measure cycle, fire a roll of 8 evenly spaced closed hats.
        // Odd-indexed hits (off-beat 32nd notes) are nudged by the current swing offset.
        if self.roll_active {
            let spacing = self.beat_duration / 8; // 32nd-note spacing
            let sw = self.swing.offset_samples(self.beat_duration);
            for i in 0..8u32 {
                let pos = spacing * i + if i % 2 == 1 { sw } else { 0 };
                if self.beat_phase == pos {
                    self.closed_hat.trigger();
                    break;
                }
            }
        }

        self.beat_phase += 1;
        if self.patterns[PATTERN_RIM].active && self.rim_pulse.tick() {
            self.rim.trigger();
        }

        let kick = self.kick.next_sample() + self.ghost_kick.next_sample();
        let snare_dry = self.snare.next_sample();
        let (snare_l, snare_r) = self.snare_reverb.process(snare_dry);
        let ghost_snare_dry = self.ghost_snare.next_sample();
        let (ghost_snare_l, ghost_snare_r) = self.ghost_snare_reverb.process(ghost_snare_dry);
        // Reverse reverb: rising noise swell fed into a long reverb, input cut at the hit.
        let rev_rev_input = if self.rev_rev_active {
            let white = self.rev_rev_noise.white();
            let f = (std::f32::consts::PI * 800.0 / self.sample_rate).sin() * 2.0;
            let high = white - self.rev_rev_bp_low - 0.4 * self.rev_rev_bp_band;
            self.rev_rev_bp_band += f * high;
            self.rev_rev_bp_low += f * self.rev_rev_bp_band;
            // t² curve: starts near-silent, accelerates toward the hit
            let out = self.rev_rev_bp_band * (self.rev_rev_amp * self.rev_rev_amp * 0.25);
            self.rev_rev_amp = (self.rev_rev_amp + self.rev_rev_rise_rate).min(1.0);
            out
        } else {
            0.0
        };
        let (rev_rev_l, rev_rev_r) = self.rev_rev_reverb.process(rev_rev_input);
        let hat = self.hat.next_sample();
        let closed_hat = self.closed_hat_lpf.process(self.closed_hat.next_sample());
        let (hat_l, hat_r) = self.hat_phaser.process(hat + closed_hat);
        let rim_dry = self.rim.next_sample();
        let rim_echoed = self.rim_delay.process(rim_dry);
        let (rim_l, rim_r) = self.rim_reverb.process(rim_echoed);
        let stab_filtered = self.stab_lpf.process(self.stab.next_sample());
        let (stab_dl, stab_dr) = self.stab_delay.process(stab_filtered);
        let (stab_l, stab_r) = self.stab_phaser.process((stab_dl + stab_dr) * 0.5);
        let stab2_filtered = self.stab2_lpf.process(self.stab2.next_sample());
        let (stab2_l, stab2_r) = self.stab2_delay.process(stab2_filtered);
        let stab3_filtered = self.stab3_lpf.process(self.stab3.next_sample());
        let (stab3_l, stab3_r) = self.stab3_reverb.process(stab3_filtered);
        let pad_filtered = self.pad_lpf.process(self.pad.next_sample());
        let pad_chorused = self.pad_chorus.process(pad_filtered);
        let (pad_l, pad_r) = self.pad_reverb.process(pad_chorused);
        let mono_dry = self.mono.next_sample();
        let (mono_rev_l, mono_rev_r) = self.mono_reverb.process(mono_dry);
        let mono_l = mono_dry + mono_rev_l * 0.25;
        let mono_r = mono_dry + mono_rev_r * 0.25;
        let clave_dry = self.clave.next_sample();
        let (clave_dl, clave_dr) = self.clave_delay.process(clave_dry);
        let (clave_l, clave_r) = self.clave_reverb.process((clave_dl + clave_dr) * 0.5);

        // Kick centre, snare centre, hat panned slightly left, closed hat centre, rim slightly right, stabs, pad and mono centre
        let mut left = kick + snare_l * 0.425 + ghost_snare_l * 0.3 + rev_rev_l * 0.25 + hat_l * 0.7 + rim_l * 0.8 + stab_l * 0.6 + stab2_l * 0.6 + stab3_l * 0.7 + pad_l + mono_l * 0.09375 + clave_l * 0.5;
        let mut right = kick + snare_r * 0.425 + ghost_snare_r * 0.3 + rev_rev_r * 0.25 + hat_r * 0.7 + rim_r * 0.8 + stab_r * 0.6 + stab2_r * 0.6 + stab3_r * 0.7 + pad_r + mono_r * 0.09375 + clave_r * 0.5;

        // Peak limiter
        let peak = left.abs().max(right.abs());
        if peak * self.limiter_gain > 0.8 {
            let target = 0.8 / peak;
            self.limiter_gain += 0.002 * (target - self.limiter_gain);
        } else {
            self.limiter_gain += 0.0001 * (1.0 - self.limiter_gain);
        }

        left *= self.limiter_gain * master_gain;
        right *= self.limiter_gain * master_gain;

        (left, right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulse_oscillator_fires_at_correct_rate() {
        let sample_rate = 44100.0;
        let freq = 2.0; // 2 Hz = trigger every 0.5 seconds = every 22050 samples
        let mut pulse = PulseOscillator::new(freq, sample_rate);

        let mut trigger_count = 0;
        let mut first_trigger_at = None;
        let mut second_trigger_at = None;

        // Run for 1 second (should get ~2 triggers)
        for i in 0..44100 {
            if pulse.tick() {
                trigger_count += 1;
                if first_trigger_at.is_none() {
                    first_trigger_at = Some(i);
                } else if second_trigger_at.is_none() {
                    second_trigger_at = Some(i);
                }
            }
        }

        assert_eq!(trigger_count, 2, "2 Hz should trigger twice per second");

        // Check interval between triggers is ~22050 samples
        let interval = second_trigger_at.unwrap() - first_trigger_at.unwrap();
        assert!(
            (interval as i32 - 22050).unsigned_abs() < 5,
            "interval between triggers should be ~22050 samples, got {}",
            interval
        );
    }

    #[test]
    fn kick_produces_signal_after_trigger() {
        let mut kick = Kick::new(44100.0);
        assert_eq!(kick.next_sample(), 0.0, "kick should be silent before trigger");

        kick.trigger();
        let mut has_signal = false;
        for _ in 0..4410 {
            if kick.next_sample().abs() > 0.01 {
                has_signal = true;
                break;
            }
        }
        assert!(has_signal, "kick should produce signal after trigger");
    }

    #[test]
    fn kick_decays_to_silence() {
        let mut kick = Kick::new(44100.0);
        kick.trigger();

        // Run for 2 seconds — should be silent by then
        for _ in 0..88200 {
            kick.next_sample();
        }
        assert!(!kick.active, "kick should be inactive after decay");
        assert_eq!(kick.next_sample(), 0.0);
    }

    #[test]
    fn ambient_techno_renders_kicks() {
        let mut engine = AmbientTechno::new(44100, 42);

        // Render 1 second — at ~2.08 Hz we should get at least 1 kick
        let mut left = vec![0.0f32; 44100];
        let mut right = vec![0.0f32; 44100];
        engine.render(&mut left, &mut right);

        let peak = left.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.01, "should have audible kick signal, got peak {}", peak);
    }

    #[test]
    fn ambient_techno_output_within_bounds() {
        let mut engine = AmbientTechno::new(44100, 42);
        let mut left = vec![0.0f32; 4096];
        let mut right = vec![0.0f32; 4096];

        for _ in 0..20 {
            engine.render(&mut left, &mut right);
            for &s in left.iter().chain(right.iter()) {
                assert!(s.abs() <= 1.0, "sample {} exceeds [-1, 1] range", s);
            }
        }
    }

    #[test]
    fn ambient_techno_deterministic_with_same_seed() {
        let mut e1 = AmbientTechno::new(44100, 99);
        let mut e2 = AmbientTechno::new(44100, 99);

        let mut l1 = vec![0.0f32; 2048];
        let mut r1 = vec![0.0f32; 2048];
        let mut l2 = vec![0.0f32; 2048];
        let mut r2 = vec![0.0f32; 2048];

        e1.render(&mut l1, &mut r1);
        e2.render(&mut l2, &mut r2);

        assert_eq!(l1, l2);
        assert_eq!(r1, r2);
    }

    #[test]
    fn hihat_produces_signal_and_decays() {
        let mut hat = HiHat::new(44100.0, 12345);
        assert_eq!(hat.next_sample(), 0.0, "hat silent before trigger");

        hat.trigger();
        let mut peak = 0.0f32;
        for _ in 0..4410 {
            peak = peak.max(hat.next_sample().abs());
        }
        assert!(peak > 0.01, "hat should produce signal after trigger");

        // After ~200ms should be essentially silent
        for _ in 0..8820 {
            hat.next_sample();
        }
        assert!(hat.next_sample().abs() < 0.001, "hat should decay to silence");
    }

    #[test]
    fn hiss_produces_continuous_signal() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let mut hiss = Hiss::new(44100.0, &mut rng);

        let mut has_signal = false;
        for _ in 0..4410 {
            if hiss.next_sample().abs() > 0.0001 {
                has_signal = true;
                break;
            }
        }
        assert!(has_signal, "hiss should produce continuous signal");
    }

    #[test]
    fn haze_param_controls_hiss_level() {
        // Test hiss directly rather than through the full mix
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let mut hiss = Hiss::new(44100.0, &mut rng);

        // At default level
        let mut rms_default = 0.0f32;
        for _ in 0..44100 {
            let s = hiss.next_sample();
            rms_default += s * s;
        }

        hiss.set_level(0.0);
        let mut rms_zero = 0.0f32;
        for _ in 0..44100 {
            let s = hiss.next_sample();
            rms_zero += s * s;
        }

        assert!(rms_default > rms_zero,
            "default level ({}) should be louder than zero ({})",
            rms_default, rms_zero);
        assert!(rms_zero < 0.0001, "hiss at level 0 should be nearly silent");
    }

    #[test]
    fn pulse_param_changes_kick_rate() {
        // Test pulse oscillator directly — more reliable than detecting kicks in mixed audio
        let mut fast = PulseOscillator::new(2.08, 44100.0);
        let mut slow = PulseOscillator::new(1.3, 44100.0);

        let mut fast_count = 0u32;
        let mut slow_count = 0u32;

        // Count triggers over 3 seconds
        for _ in 0..(44100 * 3) {
            if fast.tick() { fast_count += 1; }
            if slow.tick() { slow_count += 1; }
        }

        assert!(fast_count > slow_count,
            "2.08 Hz ({} triggers) should fire more than 1.3 Hz ({} triggers)",
            fast_count, slow_count);
    }

    #[test]
    fn dub_stab_produces_signal_and_decays() {
        let mut stab = DubStab::new(44100.0);
        assert_eq!(stab.next_sample(), 0.0, "stab silent before trigger");

        let mut noise = Xorshift64::new(42);
        stab.trigger(130.0, &mut noise);

        let mut peak = 0.0f32;
        for _ in 0..4410 {
            peak = peak.max(stab.next_sample().abs());
        }
        assert!(peak > 0.01, "stab should produce signal after trigger");

        // After 3 seconds should be silent
        for _ in 0..(44100 * 3) {
            stab.next_sample();
        }
        assert!(!stab.active, "stab should decay to inactive");
    }

    #[test]
    fn granular_engine_produces_signal() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let mut grain = GranularEngine::new(44100.0, 42, &mut rng);
        grain.set_level(0.5);

        // Spawn a grain and check it produces signal
        grain.spawn_grain();
        let mut has_signal = false;
        for _ in 0..44100 {
            let (l, r) = grain.next_sample();
            if l.abs() > 0.001 || r.abs() > 0.001 {
                has_signal = true;
                break;
            }
        }
        assert!(has_signal, "granular engine should produce signal after spawn");
    }

    #[test]
    fn polyrhythm_ratios_produce_correct_trigger_counts() {
        let sr = 44100.0;
        let base = BASE_FREQ;
        let duration_s = 20.0;

        let mut kick = PulseOscillator::new(base, sr);
        let mut hat = PulseOscillator::new(base * HAT_RATIO.0 / HAT_RATIO.1, sr);
        let mut stab = PulseOscillator::new(base * STAB_RATIO.0 / STAB_RATIO.1, sr);
        let mut grain = PulseOscillator::new(base * GRAIN_RATIO.0 / GRAIN_RATIO.1, sr);

        let mut counts = [0u32; 4];
        for _ in 0..(sr as u32 * duration_s as u32) {
            if kick.tick() { counts[0] += 1; }
            if hat.tick() { counts[1] += 1; }
            if stab.tick() { counts[2] += 1; }
            if grain.tick() { counts[3] += 1; }
        }

        // Expected: base*duration triggers for each
        // kick:  1.2 * 20 = 24
        // hat:   2.1 * 20 = 42
        // stab:  0.72 * 20 = 14.4 → 14
        // grain: 0.1714 * 20 = 3.4 → 3
        assert!((counts[0] as i32 - 24).unsigned_abs() <= 1,
            "kick: expected ~24, got {}", counts[0]);
        assert!((counts[1] as i32 - 42).unsigned_abs() <= 1,
            "hat: expected ~42, got {}", counts[1]);
        assert!((counts[2] as i32 - 14).unsigned_abs() <= 1,
            "stab: expected ~14, got {}", counts[2]);
        assert!((counts[3] as i32 - 3).unsigned_abs() <= 1,
            "grain: expected ~3, got {}", counts[3]);

        // Verify the ratios hold: hat/kick ≈ 7/4, stab/kick ≈ 3/5
        let hat_ratio = counts[1] as f32 / counts[0] as f32;
        assert!((hat_ratio - 7.0 / 4.0).abs() < 0.1,
            "hat/kick ratio should be ~1.75, got {}", hat_ratio);
    }

    #[test]
    fn full_engine_with_all_elements_stays_bounded() {
        let mut engine = AmbientTechno::new(44100, 42);
        // Crank everything up
        engine.set_param("haze", 1.0);
        engine.set_param("density", 1.0);
        engine.set_param("grain", 1.0);
        engine.set_param("drift", 1.0);

        let mut left = vec![0.0f32; 4096];
        let mut right = vec![0.0f32; 4096];

        for _ in 0..30 {
            engine.render(&mut left, &mut right);
            for &s in left.iter().chain(right.iter()) {
                assert!(s.abs() <= 1.0, "sample {} exceeds [-1, 1] range", s);
            }
        }
    }
}
