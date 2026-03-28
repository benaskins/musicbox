pub mod track;
pub mod effects;
pub mod instruments;
pub mod clocks;
pub mod util;
pub mod tracks;

pub use track::{State, Track};

use tracks::drone::Drone;

/// The complete musicbox engine. Wraps a track and delegates all audio generation to it.
pub struct MusicBox {
    track: Box<dyn Track>,
}

impl MusicBox {
    /// Construct with the default drone track.
    pub fn new(sample_rate: u32, seed: u64) -> Self {
        Self { track: Box::new(Drone::new(sample_rate, seed)) }
    }

    pub fn start_fade_out(&mut self) { self.track.start_fade_out(); }
    pub fn is_done(&self) -> bool { self.track.is_done() }
    pub fn state(&self) -> State { self.track.state() }

    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.track.render(left, right);
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
