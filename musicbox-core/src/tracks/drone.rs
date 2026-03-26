use rand::Rng;
use rand::SeedableRng;

use crate::effects::{DattorroReverb, ResonantLpf};
use crate::instruments::oscillator::Oscillator;
use crate::instruments::pluck::PluckEngine;
use crate::track::{State, Track};

const FADE_DURATION: f32 = 3.0;

/// A pentatonic minor scale: A, C, D, E, G
/// Base frequencies at octave 2, then generated across multiple octaves.
fn pentatonic_frequencies() -> Vec<f32> {
    let base_notes = [110.0, 130.81, 146.83, 164.81, 196.0]; // A2, C3, D3, E3, G3
    let mut freqs = Vec::new();
    for &octave_mult in &[0.5, 1.0, 2.0, 4.0] {
        for &f in &base_notes {
            freqs.push(f * octave_mult);
        }
    }
    freqs
}

/// A layer of oscillators in a frequency range, with optional effects.
pub struct Layer {
    pub oscillators: Vec<Oscillator>,
    pub filter: Option<ResonantLpf>,
    pub reverb: Option<DattorroReverb>,
}

impl Layer {
    pub fn new(freqs: &[f32], amplitude: f32, sparsity: f32, sample_rate: f32, rng: &mut impl Rng) -> Self {
        let oscillators = freqs
            .iter()
            .map(|&f| Oscillator::new(f, amplitude, sparsity, sample_rate, rng))
            .collect();
        Self { oscillators, filter: None, reverb: None }
    }

    pub fn with_filter(mut self, filter: ResonantLpf) -> Self {
        self.filter = Some(filter);
        self
    }

    pub fn with_reverb(mut self, reverb: DattorroReverb) -> Self {
        self.reverb = Some(reverb);
        self
    }

    pub fn next_sample(&mut self) -> (f32, f32) {
        let mut sample: f32 = self.oscillators.iter_mut().map(|o| o.next_sample()).sum();
        if let Some(f) = &mut self.filter {
            sample = f.process(sample);
        }
        if let Some(r) = &mut self.reverb {
            r.process(sample)
        } else {
            (sample, sample)
        }
    }
}

/// Drone generative engine: multiple layers of oscillators with effects,
/// implementing the `Track` trait.
pub struct Drone {
    bass: Layer,
    mid: Layer,
    high: Layer,
    plucks: PluckEngine,
    limiter_gain: f32,
    fade_pos: u32,
    fade_state: State,
    fade_samples: u32,
}

impl Drone {
    pub fn new(sample_rate: u32, seed: u64) -> Self {
        let sr = sample_rate as f32;
        let fade_samples = (sr * FADE_DURATION) as u32;
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);

        let all_freqs = pentatonic_frequencies();

        let bass_freqs: Vec<f32> = all_freqs.iter().copied().filter(|&f| f < 130.0).collect();
        let mid_freqs: Vec<f32> = all_freqs
            .iter()
            .copied()
            .filter(|&f| (130.0..400.0).contains(&f))
            .collect();
        let high_freqs: Vec<f32> = all_freqs.iter().copied().filter(|&f| f >= 400.0).collect();
        let pluck_freqs: Vec<f32> = all_freqs
            .iter()
            .copied()
            .filter(|&f| (330.0..800.0).contains(&f))
            .collect();

        let bass = Layer::new(&bass_freqs, 0.15, 0.7, sr, &mut rng);
        let mid_filter = ResonantLpf::new(200.0, 1200.0, 0.3, 0.065, sr, &mut rng);
        let mid = Layer::new(&mid_freqs, 0.08, 0.5, sr, &mut rng).with_filter(mid_filter);
        let high_reverb = DattorroReverb::new(0.85, 0.4, 0.7, 0.04, sr, &mut rng);
        let high = Layer::new(&high_freqs, 0.04, 0.3, sr, &mut rng).with_reverb(high_reverb);
        let plucks = PluckEngine::new(pluck_freqs, 0.3, sr, &mut rng);

        Self {
            bass,
            mid,
            high,
            plucks,
            limiter_gain: 1.0,
            fade_pos: 0,
            fade_state: State::FadingIn,
            fade_samples,
        }
    }

    /// Generate the next stereo sample pair, including fade and limiting.
    fn next_sample(&mut self) -> (f32, f32) {
        let master_gain = match self.fade_state {
            State::FadingIn => {
                self.fade_pos += 1;
                if self.fade_pos >= self.fade_samples {
                    self.fade_state = State::Playing;
                }
                let t = self.fade_pos as f32 / self.fade_samples as f32;
                t * t
            }
            State::Playing => 1.0,
            State::FadingOut => {
                if self.fade_pos == 0 {
                    self.fade_state = State::Done;
                    0.0
                } else {
                    self.fade_pos = self.fade_pos.saturating_sub(1);
                    let t = self.fade_pos as f32 / self.fade_samples as f32;
                    t * t
                }
            }
            State::Done => 0.0,
        };

        if self.fade_state == State::Done {
            return (0.0, 0.0);
        }

        let (bass_l, bass_r) = self.bass.next_sample();
        let (mid_l, mid_r) = self.mid.next_sample();
        let (high_l, high_r) = self.high.next_sample();
        let (pluck_l, pluck_r) = self.plucks.next_sample();

        let mut left = (bass_l + mid_l + high_l + pluck_l).tanh();
        let mut right = (bass_r + mid_r + high_r + pluck_r).tanh();

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

impl Track for Drone {
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let len = left.len().min(right.len());
        for i in 0..len {
            let (l, r) = self.next_sample();
            left[i] = l;
            right[i] = r;
        }
    }

    fn start_fade_out(&mut self) {
        if self.fade_state == State::FadingIn || self.fade_state == State::Playing {
            self.fade_state = State::FadingOut;
        }
    }

    fn state(&self) -> State {
        self.fade_state
    }
}
