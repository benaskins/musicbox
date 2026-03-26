use rand::Rng;
use crate::util::prng::Xorshift64;
use crate::effects::DattorroReverb;

/// Frequency pools for space grains — very high shimmer and deep sub rumble
const GRAIN_FREQS_HIGH: [f32; 6] = [1800.0, 2400.0, 3200.0, 4200.0, 5600.0, 7000.0];
const GRAIN_FREQS_LOW: [f32; 4] = [40.0, 55.0, 65.0, 80.0];

pub struct Grain {
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
    pub fn new() -> Self {
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

    pub fn trigger(&mut self, freq: f32, drift: f32, duration_samples: f32, pan: f32) {
        self.phase = 0.0;
        self.freq = freq;
        self.drift = drift;
        self.window_phase = 0.0;
        self.window_rate = 1.0 / duration_samples;
        self.pan = pan;
        self.active = true;
    }

    pub fn next_sample(&mut self, sample_rate: f32) -> (f32, f32) {
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

impl GranularEngine {
    pub fn new(sample_rate: f32, seed: u64, rng: &mut impl Rng) -> Self {
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
