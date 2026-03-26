use rand::Rng;
use crate::effects::delay::DelayLine;
use crate::effects::BbdDelay;

/// Karplus-Strong pluck synthesis.
pub struct PluckVoice {
    pub delay: DelayLine,
    pub period: usize,
    pub lp_state: f32,
    pub feedback: f32,
    pub active: bool,
}

impl PluckVoice {
    pub fn new(max_delay: usize) -> Self {
        Self {
            delay: DelayLine::new(max_delay),
            period: max_delay,
            lp_state: 0.0,
            feedback: 0.996,
            active: false,
        }
    }

    pub fn next_sample(&mut self) -> f32 {
        if !self.active {
            return 0.0;
        }

        let out = self.delay.read_at(self.period);
        let prev = self.delay.read_at(self.period - 1);
        let averaged = (out + prev) * 0.5;

        self.lp_state += 0.7 * (averaged - self.lp_state);
        let fed_back = self.lp_state * self.feedback;

        self.delay.write_and_advance(fed_back);

        if out.abs() < 0.0001 {
            self.active = false;
        }

        out
    }
}

/// Engine that stochastically triggers pluck voices and feeds them through
/// a BBD delay. Produces stereo output via two slightly different BBD delays.
pub struct PluckEngine {
    pub voices: Vec<PluckVoice>,
    pub bbd_left: BbdDelay,
    pub bbd_right: BbdDelay,
    pub freqs: Vec<f32>,
    pub next_pluck_in: u32,
    pub min_interval: u32,
    pub max_interval: u32,
    pub amplitude: f32,
    pub rng_state: u64,
}

impl PluckEngine {
    pub fn new(freqs: Vec<f32>, amplitude: f32, sample_rate: f32, rng: &mut impl Rng) -> Self {
        let num_voices = 6;
        let voices = (0..num_voices)
            .map(|_| PluckVoice::new(512))
            .collect();

        let bbd_left = BbdDelay::new(340.0, 0.35, 0.5, 0.3, 1.5, sample_rate, rng);
        let bbd_right = BbdDelay::new(370.0, 0.35, 0.5, 0.25, 1.8, sample_rate, rng);

        let min_interval = (sample_rate * 1.0) as u32;
        let max_interval = (sample_rate * 4.0) as u32;

        let rng_state = rng.r#gen::<u64>() | 1;

        Self {
            voices,
            bbd_left,
            bbd_right,
            freqs,
            next_pluck_in: min_interval,
            min_interval,
            max_interval,
            amplitude,
            rng_state,
        }
    }

    pub fn xorshift(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    pub fn next_sample(&mut self) -> (f32, f32) {
        if self.next_pluck_in == 0 {
            let freq_idx = (Self::xorshift(&mut self.rng_state) as usize) % self.freqs.len();
            let freq = self.freqs[freq_idx];
            let period = (self.bbd_left.sample_rate / freq) as usize;

            let voice_idx = self
                .voices
                .iter()
                .position(|v| !v.active)
                .unwrap_or(0);

            let voice = &mut self.voices[voice_idx];
            voice.period = period.min(voice.delay.len() - 1).max(2);
            for s in voice.delay.buffer.iter_mut() {
                *s = 0.0;
            }
            for i in 0..voice.period {
                let r = Self::xorshift(&mut self.rng_state);
                voice.delay.buffer[i] = (r as f32) / (u64::MAX as f32) - 0.5;
            }
            voice.delay.write_pos = voice.period;
            voice.lp_state = 0.0;
            voice.active = true;

            let range = self.max_interval - self.min_interval;
            self.next_pluck_in =
                self.min_interval + (Self::xorshift(&mut self.rng_state) as u32) % range;
        } else {
            self.next_pluck_in -= 1;
        }

        let dry: f32 = self.voices.iter_mut().map(|v| v.next_sample()).sum::<f32>() * self.amplitude;

        let left = self.bbd_left.process(dry);
        let right = self.bbd_right.process(dry);

        (left, right)
    }
}
