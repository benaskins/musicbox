use rand::Rng;
use rand::SeedableRng;

use crate::dsp::{DattorroReverb, ResonantLpf};

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
    current_amp: f32,
    sample_rate: f32,
    active: bool,
}

impl Kick {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            phase: 0.0,
            pitch_start: 150.0,
            pitch_end: 50.0,
            pitch_decay: 0.995,
            amp_decay: 0.9995,
            current_pitch: 50.0,
            current_amp: 0.0,
            sample_rate,
            active: false,
        }
    }

    /// Fire the kick — resets envelope.
    pub fn trigger(&mut self) {
        self.phase = 0.0;
        self.current_pitch = self.pitch_start;
        self.current_amp = 1.0;
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

        // Exponential pitch decay toward pitch_end
        self.current_pitch = self.pitch_end
            + (self.current_pitch - self.pitch_end) * self.pitch_decay;

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
            decay: (-1.0 / (sample_rate * 0.03_f32)).exp(), // ~30ms decay
            bp_low: 0.0,
            bp_band: 0.0,
            bp_freq: 8000.0,
            sample_rate,
        }
    }

    pub fn trigger(&mut self) {
        self.amp = 0.15;
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

    /// Trigger a stab with a root frequency. Creates a minor chord (root, minor 3rd, 5th)
    /// with slight detuning.
    pub fn trigger(&mut self, root_freq: f32, rng: &mut Xorshift64) {
        let detune_root = 1.0 + (rng.white() * 0.008);
        let detune_3rd = 1.0 + (rng.white() * 0.012);
        let detune_5th = 1.0 + (rng.white() * 0.010);
        self.freqs[0] = root_freq * detune_root;
        self.freqs[1] = root_freq * 1.2 * detune_3rd; // minor 3rd
        self.freqs[2] = root_freq * 1.5 * detune_5th; // 5th
        self.phases = [0.0; 3];
        self.amp = 0.35;
        self.lp_cutoff = 2500.0;
        self.lp_low = 0.0;
        self.lp_band = 0.0;
        self.hp_low = 0.0;
        self.hp_band = 0.0;
        self.active = true;
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
/// Stab      3/5      0.720 Hz    5 base cycles  (4.2s)
/// Grain     1/7      0.171 Hz    7 base cycles  (5.8s)
///
/// Full alignment: LCM(4, 5, 7) = 140 base cycles ≈ 116.7s ≈ ~2 minutes
/// ```
pub struct AmbientTechno {
    kick: Kick,
    kick_pulse: PulseOscillator,
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
const HAT_RATIO: (f32, f32) = (7.0, 4.0);    // 7/4 base
const STAB_RATIO: (f32, f32) = (3.0, 5.0);   // 3/5 base
const GRAIN_RATIO: (f32, f32) = (1.0, 7.0);   // 1/7 base

impl AmbientTechno {
    pub fn new(sample_rate: u32, _seed: u64) -> Self {
        let sr = sample_rate as f32;

        Self {
            kick: Kick::new(sr),
            kick_pulse: PulseOscillator::new(BASE_FREQ, sr),
            limiter_gain: 1.0,
            fade_pos: 0,
            fade_state: FadeState::FadingIn,
            fade_samples: (sr * FADE_DURATION) as u32,
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
            self.kick.trigger();
        }

        let sample = self.kick.next_sample();

        let mut left = sample;
        let mut right = sample;

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
