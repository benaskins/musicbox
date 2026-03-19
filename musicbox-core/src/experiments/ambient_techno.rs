use rand::Rng;
use rand::SeedableRng;

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

/// First experiment: kick pulsing at a steady frequency.
/// Minimal engine to verify the kick + pulse oscillator pipeline.
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

impl AmbientTechno {
    pub fn new(sample_rate: u32, seed: u64) -> Self {
        let sr = sample_rate as f32;
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);

        // ~2.08 Hz = ~125 BPM
        let kick_freq = 2.0833;
        // Start at a random phase so the first kick isn't always at sample 0
        let kick_phase = rng.r#gen::<f32>();

        Self {
            kick: Kick::new(sr),
            kick_pulse: PulseOscillator::new_with_phase(kick_freq, sr, kick_phase),
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

    pub fn set_param(&mut self, name: &str, value: f32) {
        match name {
            "pulse" => self.kick_pulse.set_freq(value.clamp(1.3, 2.5)),
            _ => {}
        }
    }

    pub fn get_params(&self) -> Vec<(&str, f32, f32, f32)> {
        vec![
            ("pulse", self.kick_pulse.freq(), 1.3, 2.5),
        ]
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

        // Pulse oscillator triggers the kick
        if self.kick_pulse.tick() {
            self.kick.trigger();
        }

        let kick_sample = self.kick.next_sample();

        let mut left = kick_sample;
        let mut right = kick_sample;

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
    fn pulse_param_changes_kick_rate() {
        let mut engine = AmbientTechno::new(44100, 42);

        // Count kicks at default rate (~2.08 Hz) over 2 seconds
        let mut left = vec![0.0f32; 128];
        let mut right = vec![0.0f32; 128];
        let mut kicks_default = 0u32;
        let mut was_silent = true;
        for _ in 0..(44100 * 2 / 128) {
            engine.render(&mut left, &mut right);
            let peak = left.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            if peak > 0.1 && was_silent {
                kicks_default += 1;
                was_silent = false;
            } else if peak < 0.01 {
                was_silent = true;
            }
        }

        // Now set pulse to 1.3 Hz (slower) and count again
        let mut engine2 = AmbientTechno::new(44100, 42);
        engine2.set_param("pulse", 1.3);
        let mut kicks_slow = 0u32;
        was_silent = true;
        for _ in 0..(44100 * 2 / 128) {
            engine2.render(&mut left, &mut right);
            let peak = left.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            if peak > 0.1 && was_silent {
                kicks_slow += 1;
                was_silent = false;
            } else if peak < 0.01 {
                was_silent = true;
            }
        }

        assert!(kicks_default > kicks_slow,
            "faster pulse ({}) should produce more kicks than slower ({})",
            kicks_default, kicks_slow);
    }
}
