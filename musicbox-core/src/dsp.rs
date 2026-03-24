use rand::Rng;

/// A single drone oscillator that fades in and out over time.
pub struct Oscillator {
    /// Current phase of the audio oscillator (0.0 to 1.0)
    pub phase: f32,
    /// Frequency in Hz
    pub freq: f32,
    /// Phase of the slow LFO that controls amplitude envelope
    pub envelope_phase: f32,
    /// How fast the envelope LFO cycles (Hz) — very slow, e.g. 0.012-0.05 Hz
    pub envelope_rate: f32,
    /// Phase of an LFO that gently drifts the pitch
    pub drift_phase: f32,
    /// Rate of pitch drift LFO
    pub drift_rate: f32,
    /// Max pitch drift in Hz
    pub drift_amount: f32,
    /// Base amplitude for this oscillator
    pub amplitude: f32,
    /// How much of the cycle is silent (0.0 = always on, 0.8 = silent 80% of the time)
    pub sparsity: f32,
    /// Sample rate
    pub sample_rate: f32,
}

impl Oscillator {
    pub fn new(freq: f32, amplitude: f32, sparsity: f32, sample_rate: f32, rng: &mut impl Rng) -> Self {
        Self {
            phase: rng.r#gen::<f32>(),
            freq,
            envelope_phase: rng.r#gen::<f32>(),
            envelope_rate: rng.r#gen_range(0.012..0.05),
            drift_phase: rng.r#gen::<f32>(),
            drift_rate: rng.r#gen_range(0.02..0.08),
            drift_amount: freq * 0.003,
            amplitude,
            sparsity,
            sample_rate,
        }
    }

    /// Generate the next sample and advance all phases.
    pub fn next_sample(&mut self) -> f32 {
        let envelope_raw = (self.envelope_phase * std::f32::consts::TAU).sin();
        let envelope_01 = (envelope_raw + 1.0) * 0.5;
        let envelope = ((envelope_01 - self.sparsity) / (1.0 - self.sparsity)).clamp(0.0, 1.0);
        let envelope = envelope * envelope;

        let drift = (self.drift_phase * std::f32::consts::TAU).sin() * self.drift_amount;
        let current_freq = self.freq + drift;

        let sample = (self.phase * std::f32::consts::TAU).sin();

        self.phase += current_freq / self.sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        self.envelope_phase += self.envelope_rate / self.sample_rate;
        if self.envelope_phase >= 1.0 {
            self.envelope_phase -= 1.0;
        }
        self.drift_phase += self.drift_rate / self.sample_rate;
        if self.drift_phase >= 1.0 {
            self.drift_phase -= 1.0;
        }

        sample * envelope * self.amplitude
    }
}

/// A resonant low-pass filter using a state-variable filter topology.
pub struct ResonantLpf {
    pub low: f32,
    pub band: f32,
    pub cutoff_lfo_phase: f32,
    pub cutoff_lfo_rate: f32,
    pub cutoff_min: f32,
    pub cutoff_max: f32,
    pub resonance: f32,
    pub sample_rate: f32,
}

impl ResonantLpf {
    pub fn new(cutoff_min: f32, cutoff_max: f32, resonance: f32, lfo_rate: f32, sample_rate: f32, rng: &mut impl Rng) -> Self {
        Self {
            low: 0.0,
            band: 0.0,
            cutoff_lfo_phase: rng.r#gen::<f32>(),
            cutoff_lfo_rate: lfo_rate,
            cutoff_min,
            cutoff_max,
            resonance,
            sample_rate,
        }
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let lfo = (self.cutoff_lfo_phase * std::f32::consts::TAU).sin();
        let cutoff = self.cutoff_min + (self.cutoff_max - self.cutoff_min) * (lfo + 1.0) * 0.5;

        let f = (std::f32::consts::PI * cutoff / self.sample_rate).sin() * 2.0;
        let q = 1.0 - self.resonance;

        for _ in 0..2 {
            let high = input - self.low - q * self.band;
            self.band += f * high;
            self.low += f * self.band;
        }

        self.cutoff_lfo_phase += self.cutoff_lfo_rate / self.sample_rate;
        if self.cutoff_lfo_phase >= 1.0 {
            self.cutoff_lfo_phase -= 1.0;
        }

        self.low
    }
}

/// A delay line used as a building block for reverb.
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

/// Dattorro plate reverb, ported from Mutable Instruments Clouds.
/// Original: https://github.com/pichenettes/eurorack/blob/master/clouds/dsp/fx/reverb.h
/// Copyright 2014 Emilie Gillet, licensed under MIT.
pub struct DattorroReverb {
    ap1: DelayLine,
    ap2: DelayLine,
    ap3: DelayLine,
    ap4: DelayLine,
    dap1a: DelayLine,
    dap1b: DelayLine,
    del1: DelayLine,
    dap2a: DelayLine,
    dap2b: DelayLine,
    del2: DelayLine,
    pub input_gain: f32,
    pub reverb_time: f32,
    pub diffusion: f32,
    pub lp: f32,
    lp1_state: f32,
    lp2_state: f32,
    lfo1_cos: f32,
    lfo1_sin: f32,
    lfo2_cos: f32,
    lfo2_sin: f32,
    lfo_counter: u32,
    pub amount: f32,
    pub amount_lfo_phase: f32,
    pub amount_lfo_rate: f32,
    pub amount_min: f32,
    pub amount_max: f32,
    pub sample_rate: f32,
}

impl DattorroReverb {
    pub fn new(
        reverb_time: f32,
        amount_min: f32,
        amount_max: f32,
        amount_lfo_rate: f32,
        sample_rate: f32,
        rng: &mut impl Rng,
    ) -> Self {
        Self {
            ap1: DelayLine::new(156),
            ap2: DelayLine::new(223),
            ap3: DelayLine::new(332),
            ap4: DelayLine::new(550),
            dap1a: DelayLine::new(2278),
            dap1b: DelayLine::new(2808),
            del1: DelayLine::new(4700),
            dap2a: DelayLine::new(2636),
            dap2b: DelayLine::new(2291),
            del2: DelayLine::new(6590),
            input_gain: 0.2,
            reverb_time,
            diffusion: 0.625,
            lp: 0.82,
            lp1_state: 0.0,
            lp2_state: 0.0,
            lfo1_cos: 1.0,
            lfo1_sin: 0.0,
            lfo2_cos: 1.0,
            lfo2_sin: 0.0,
            lfo_counter: 0,
            amount: (amount_min + amount_max) * 0.5,
            amount_lfo_phase: rng.r#gen::<f32>(),
            amount_lfo_rate,
            amount_min,
            amount_max,
            sample_rate,
        }
    }

    #[inline]
    pub fn allpass(delay: &mut DelayLine, input: f32, g: f32) -> f32 {
        let delayed = delay.read_at(delay.len() - 1);
        let v = input + g * delayed;
        let output = delayed - g * v;
        delay.write_and_advance(v);
        output
    }

    pub fn process(&mut self, input: f32) -> (f32, f32) {
        let lfo = (self.amount_lfo_phase * std::f32::consts::TAU).sin();
        self.amount = self.amount_min + (self.amount_max - self.amount_min) * (lfo + 1.0) * 0.5;
        self.amount_lfo_phase += self.amount_lfo_rate / self.sample_rate;
        if self.amount_lfo_phase >= 1.0 {
            self.amount_lfo_phase -= 1.0;
        }

        if self.lfo_counter & 31 == 0 {
            let lfo1_freq = 0.5 / self.sample_rate;
            let w1 = std::f32::consts::TAU * lfo1_freq * 32.0;
            let new_cos1 = self.lfo1_cos * w1.cos() - self.lfo1_sin * w1.sin();
            let new_sin1 = self.lfo1_cos * w1.sin() + self.lfo1_sin * w1.cos();
            self.lfo1_cos = new_cos1;
            self.lfo1_sin = new_sin1;

            let lfo2_freq = 0.3 / self.sample_rate;
            let w2 = std::f32::consts::TAU * lfo2_freq * 32.0;
            let new_cos2 = self.lfo2_cos * w2.cos() - self.lfo2_sin * w2.sin();
            let new_sin2 = self.lfo2_cos * w2.sin() + self.lfo2_sin * w2.cos();
            self.lfo2_cos = new_cos2;
            self.lfo2_sin = new_sin2;
        }
        self.lfo_counter = self.lfo_counter.wrapping_add(1);

        let kap = self.diffusion;
        let krt = self.reverb_time;
        let klp = self.lp;

        let lfo1_uni = (self.lfo1_cos + 1.0) * 0.5;
        let ap1_mod_delay = 14.0 + lfo1_uni * 120.0;
        let ap1_mod_read = self.ap1.read_at_f(ap1_mod_delay);
        self.ap1.write_at(138, ap1_mod_read);

        let sig = input * self.input_gain;
        let ap1_out = Self::allpass(&mut self.ap1, sig, kap);
        let ap2_out = Self::allpass(&mut self.ap2, ap1_out, kap);
        let ap3_out = Self::allpass(&mut self.ap3, ap2_out, kap);
        let apout = Self::allpass(&mut self.ap4, ap3_out, kap);

        let lfo2_uni = (self.lfo2_cos + 1.0) * 0.5;
        let del2_mod_delay = 6311.0 + lfo2_uni * 276.0;
        let del2_read = self.del2.read_at_f(del2_mod_delay);
        let tank1_in = apout + del2_read * krt;

        self.lp1_state += klp * (tank1_in - self.lp1_state);
        let lp1_out = self.lp1_state;

        let dap1a_out = Self::allpass(&mut self.dap1a, lp1_out, kap);
        let dap1b_out = Self::allpass(&mut self.dap1b, dap1a_out, kap);
        self.del1.write_and_advance(dap1b_out);

        let del1_read = self.del1.read_at(self.del1.len() - 1);
        let tank2_in = apout + del1_read * krt;

        self.lp2_state += klp * (tank2_in - self.lp2_state);
        let lp2_out = self.lp2_state;

        let dap2a_out = Self::allpass(&mut self.dap2a, lp2_out, kap);
        let dap2b_out = Self::allpass(&mut self.dap2b, dap2a_out, kap);
        self.del2.write_and_advance(dap2b_out);

        let wet_l = del1_read;
        let wet_r = del2_read;

        let left = input * (1.0 - self.amount) + wet_l * self.amount;
        let right = input * (1.0 - self.amount) + wet_r * self.amount;

        (left, right)
    }
}

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

/// 4-stage all-pass phaser modelled on the Sovtek Small Stone.
/// Stereo output: L and R LFOs are 180° out of phase for a wide spatial sweep.
pub struct Phaser {
    ap_x1: [[f32; 2]; 4], // x[n-1] per stage per channel
    ap_y1: [[f32; 2]; 4], // y[n-1] per stage per channel
    fb: [f32; 2],          // feedback sample per channel
    lfo_phase: f32,
    lfo_rate: f32,
    feedback: f32,
    mix: f32,
    sample_rate: f32,
}

impl Phaser {
    pub fn new(lfo_rate: f32, feedback: f32, mix: f32, sample_rate: f32) -> Self {
        Self {
            ap_x1: [[0.0; 2]; 4],
            ap_y1: [[0.0; 2]; 4],
            fb: [0.0; 2],
            lfo_phase: 0.0,
            lfo_rate,
            feedback,
            mix,
            sample_rate,
        }
    }

    pub fn process(&mut self, input: f32) -> (f32, f32) {
        let lfo_l = (self.lfo_phase * std::f32::consts::TAU).sin();
        let lfo_r = ((self.lfo_phase + 0.5) * std::f32::consts::TAU).sin(); // 180° offset
        self.lfo_phase += self.lfo_rate / self.sample_rate;
        if self.lfo_phase >= 1.0 { self.lfo_phase -= 1.0; }

        let mut out = [0.0f32; 2];
        for (ch, lfo) in [(0, lfo_l), (1, lfo_r)] {
            // Sweep all-pass cutoff 100–4000 Hz
            let freq = 100.0_f32 + (lfo * 0.5 + 0.5) * 3900.0;
            let g = (std::f32::consts::PI * freq / self.sample_rate - 1.0)
                  / (std::f32::consts::PI * freq / self.sample_rate + 1.0);

            let mut x = input + self.fb[ch] * self.feedback;
            for stage in 0..4 {
                let y = g * x + self.ap_x1[stage][ch] - g * self.ap_y1[stage][ch];
                self.ap_x1[stage][ch] = x;
                self.ap_y1[stage][ch] = y;
                x = y;
            }
            self.fb[ch] = x;
            out[ch] = input * (1.0 - self.mix) + x * self.mix;
        }
        (out[0], out[1])
    }
}
