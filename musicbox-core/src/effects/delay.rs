use rand::Rng;

/// A delay line used as a building block for reverb and other effects.
pub struct DelayLine {
    pub buffer: Vec<f32>,
    pub write_pos: usize,
}

impl DelayLine {
    pub fn new(length: usize) -> Self {
        Self {
            buffer: vec![0.0; length],
            write_pos: 0,
        }
    }

    pub fn write_and_advance(&mut self, sample: f32) {
        self.buffer[self.write_pos] = sample;
        self.write_pos = (self.write_pos + 1) % self.buffer.len();
    }

    pub fn read_at(&self, delay: usize) -> f32 {
        let len = self.buffer.len();
        let pos = (self.write_pos + len - delay) % len;
        self.buffer[pos]
    }

    pub fn read_at_f(&self, delay: f32) -> f32 {
        let delay = delay.max(1.0);
        let len = self.buffer.len();
        let d_floor = delay.floor() as usize;
        let frac = delay - delay.floor();
        let a = self.read_at(d_floor.min(len - 1));
        let b = self.read_at((d_floor + 1).min(len - 1));
        a + (b - a) * frac
    }

    pub fn write_at(&mut self, delay: usize, sample: f32) {
        let len = self.buffer.len();
        let pos = (self.write_pos + len - delay) % len;
        self.buffer[pos] = sample;
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }
}

/// BBD (Bucket Brigade Device) delay emulation.
pub struct BbdDelay {
    pub buffer: DelayLine,
    pub lp_state: f32,
    pub feedback: f32,
    pub delay_samples: f32,
    pub wobble_phase: f32,
    pub wobble_rate: f32,
    pub wobble_depth: f32,
    pub mix: f32,
    pub sample_rate: f32,
}

impl BbdDelay {
    pub fn new(
        delay_ms: f32,
        feedback: f32,
        mix: f32,
        wobble_rate: f32,
        wobble_depth_ms: f32,
        sample_rate: f32,
        rng: &mut impl Rng,
    ) -> Self {
        let delay_samples = delay_ms * sample_rate / 1000.0;
        let max_samples = (delay_samples + wobble_depth_ms * sample_rate / 1000.0 + 100.0) as usize;
        Self {
            buffer: DelayLine::new(max_samples),
            lp_state: 0.0,
            feedback,
            delay_samples,
            wobble_phase: rng.r#gen::<f32>(),
            wobble_rate,
            wobble_depth: wobble_depth_ms * sample_rate / 1000.0,
            mix,
            sample_rate,
        }
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let wobble = (self.wobble_phase * std::f32::consts::TAU).sin() * self.wobble_depth;
        let current_delay = self.delay_samples + wobble;

        let delayed = self.buffer.read_at_f(current_delay);

        self.lp_state += 0.45 * (delayed - self.lp_state);
        let filtered = self.lp_state;

        let write_sample = input + filtered * self.feedback;
        self.buffer.write_and_advance(write_sample);

        self.wobble_phase += self.wobble_rate / self.sample_rate;
        if self.wobble_phase >= 1.0 {
            self.wobble_phase -= 1.0;
        }

        input * (1.0 - self.mix) + filtered * self.mix
    }
}

/// Dub delay: long feedback delay with filtering in the feedback path.
/// Classic dub style — repeats that darken and smear over time.
pub struct DubDelay {
    buffer: DelayLine,
    feedback: f32,
    lp_state: f32,
    lp_coeff: f32,
    hp_state: f32,
    hp_coeff: f32,
    delay_samples: usize,
    mix: f32,
}

impl DubDelay {
    pub fn new(delay_ms: f32, feedback: f32, mix: f32, sample_rate: f32) -> Self {
        let delay_samples = (delay_ms * sample_rate / 1000.0) as usize;
        Self {
            buffer: DelayLine::new(delay_samples + 1),
            feedback,
            lp_state: 0.0,
            lp_coeff: 0.35, // darkening LP in feedback
            hp_state: 0.0,
            hp_coeff: 0.05, // removes DC/sub buildup in feedback
            delay_samples,
            mix,
        }
    }

    pub fn process(&mut self, input: f32) -> (f32, f32) {
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
