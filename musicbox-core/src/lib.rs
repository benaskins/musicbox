pub mod dsp;
pub mod experiments;

use rand::SeedableRng;

use dsp::{DattorroReverb, Layer, PluckEngine, ResonantLpf};

const FADE_DURATION: f32 = 3.0;

/// Fade/playback state for the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    FadingIn,
    Playing,
    FadingOut,
    Done,
}

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

/// The complete musicbox engine. Generates stereo samples with all layers,
/// effects, limiting, and master fade.
pub struct MusicBox {
    bass: Layer,
    mid: Layer,
    high: Layer,
    plucks: PluckEngine,
    limiter_gain: f32,
    fade_pos: u32,
    fade_state: State,
    fade_samples: u32,
}

impl MusicBox {
    /// Construct with explicit sample rate and RNG seed.
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

    /// Signal the synth to begin fading out.
    pub fn start_fade_out(&mut self) {
        if self.fade_state == State::FadingIn || self.fade_state == State::Playing {
            self.fade_state = State::FadingOut;
        }
    }

    /// True once fade-out is complete and output is silent.
    pub fn is_done(&self) -> bool {
        self.fade_state == State::Done
    }

    /// Current fade/playback state.
    pub fn state(&self) -> State {
        self.fade_state
    }

    /// Fill split stereo buffers. Host calls this per audio callback.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let len = left.len().min(right.len());
        for i in 0..len {
            let (l, r) = self.next_sample();
            left[i] = l;
            right[i] = r;
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

/// Parse a duration string like "10m", "1h30m", "90s", "5m30s" into seconds.
pub fn parse_duration(s: &str) -> Option<f32> {
    let s = s.trim();
    let mut total: f32 = 0.0;
    let mut num_buf = String::new();

    for c in s.chars() {
        if c.is_ascii_digit() || c == '.' {
            num_buf.push(c);
        } else {
            let n: f32 = num_buf.parse().ok()?;
            num_buf.clear();
            match c {
                'h' => total += n * 3600.0,
                'm' => total += n * 60.0,
                's' => total += n,
                _ => return None,
            }
        }
    }
    if !num_buf.is_empty() {
        let n: f32 = num_buf.parse().ok()?;
        total += n;
    }
    if total > 0.0 { Some(total) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn musicbox_renders_nonzero_audio() {
        let mut engine = MusicBox::new(44100, 42);
        let mut left = vec![0.0f32; 1024];
        let mut right = vec![0.0f32; 1024];
        engine.render(&mut left, &mut right);

        // After 1024 samples of fade-in, there should be some non-zero output
        let has_signal = left.iter().any(|&s| s.abs() > 1e-10)
            || right.iter().any(|&s| s.abs() > 1e-10);
        assert!(has_signal, "expected non-zero audio output during fade-in");
    }

    #[test]
    fn musicbox_fades_out_to_done() {
        let mut engine = MusicBox::new(44100, 42);

        // Render enough to get past fade-in (3s = 132300 samples)
        let mut buf_l = vec![0.0f32; 4096];
        let mut buf_r = vec![0.0f32; 4096];
        for _ in 0..40 {
            engine.render(&mut buf_l, &mut buf_r);
        }
        assert_eq!(engine.state(), State::Playing);

        // Trigger fade-out
        engine.start_fade_out();
        assert_eq!(engine.state(), State::FadingOut);

        // Render through the fade-out (3s = 132300 samples)
        for _ in 0..40 {
            engine.render(&mut buf_l, &mut buf_r);
        }
        assert!(engine.is_done());
    }

    #[test]
    fn musicbox_deterministic_with_same_seed() {
        let mut engine1 = MusicBox::new(44100, 123);
        let mut engine2 = MusicBox::new(44100, 123);

        let mut l1 = vec![0.0f32; 512];
        let mut r1 = vec![0.0f32; 512];
        let mut l2 = vec![0.0f32; 512];
        let mut r2 = vec![0.0f32; 512];

        engine1.render(&mut l1, &mut r1);
        engine2.render(&mut l2, &mut r2);

        assert_eq!(l1, l2, "same seed should produce identical left channel");
        assert_eq!(r1, r2, "same seed should produce identical right channel");
    }

    #[test]
    fn musicbox_output_within_bounds() {
        let mut engine = MusicBox::new(44100, 42);
        let mut left = vec![0.0f32; 4096];
        let mut right = vec![0.0f32; 4096];

        // Render a few blocks
        for _ in 0..10 {
            engine.render(&mut left, &mut right);
            for &s in left.iter().chain(right.iter()) {
                assert!(s.abs() <= 1.0, "sample {} exceeds [-1, 1] range", s);
            }
        }
    }

    #[test]
    fn parse_duration_works() {
        assert_eq!(parse_duration("10m"), Some(600.0));
        assert_eq!(parse_duration("1h30m"), Some(5400.0));
        assert_eq!(parse_duration("90s"), Some(90.0));
        assert_eq!(parse_duration("5m30s"), Some(330.0));
        assert_eq!(parse_duration(""), None);
    }
}
