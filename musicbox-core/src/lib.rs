use rand::Rng;
use rand::SeedableRng;

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

/// A single drone oscillator that fades in and out over time.
struct Oscillator {
    /// Current phase of the audio oscillator (0.0 to 1.0)
    phase: f32,
    /// Frequency in Hz
    freq: f32,
    /// Phase of the slow LFO that controls amplitude envelope
    envelope_phase: f32,
    /// How fast the envelope LFO cycles (Hz) — very slow, e.g. 0.012-0.05 Hz
    envelope_rate: f32,
    /// Phase of an LFO that gently drifts the pitch
    drift_phase: f32,
    /// Rate of pitch drift LFO
    drift_rate: f32,
    /// Max pitch drift in Hz
    drift_amount: f32,
    /// Base amplitude for this oscillator
    amplitude: f32,
    /// How much of the cycle is silent (0.0 = always on, 0.8 = silent 80% of the time)
    sparsity: f32,
    /// Sample rate
    sample_rate: f32,
}

impl Oscillator {
    fn new(freq: f32, amplitude: f32, sparsity: f32, sample_rate: f32, rng: &mut impl Rng) -> Self {
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
    fn next_sample(&mut self) -> f32 {
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
struct ResonantLpf {
    low: f32,
    band: f32,
    cutoff_lfo_phase: f32,
    cutoff_lfo_rate: f32,
    cutoff_min: f32,
    cutoff_max: f32,
    resonance: f32,
    sample_rate: f32,
}

impl ResonantLpf {
    fn new(cutoff_min: f32, cutoff_max: f32, resonance: f32, lfo_rate: f32, sample_rate: f32, rng: &mut impl Rng) -> Self {
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

    fn process(&mut self, input: f32) -> f32 {
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
struct DelayLine {
    buffer: Vec<f32>,
    write_pos: usize,
}

impl DelayLine {
    fn new(length: usize) -> Self {
        Self {
            buffer: vec![0.0; length],
            write_pos: 0,
        }
    }

    fn write_and_advance(&mut self, sample: f32) {
        self.buffer[self.write_pos] = sample;
        self.write_pos = (self.write_pos + 1) % self.buffer.len();
    }

    fn read_at(&self, delay: usize) -> f32 {
        let len = self.buffer.len();
        let pos = (self.write_pos + len - delay) % len;
        self.buffer[pos]
    }

    fn read_at_f(&self, delay: f32) -> f32 {
        let delay = delay.max(1.0);
        let len = self.buffer.len();
        let d_floor = delay.floor() as usize;
        let frac = delay - delay.floor();
        let a = self.read_at(d_floor.min(len - 1));
        let b = self.read_at((d_floor + 1).min(len - 1));
        a + (b - a) * frac
    }

    fn write_at(&mut self, delay: usize, sample: f32) {
        let len = self.buffer.len();
        let pos = (self.write_pos + len - delay) % len;
        self.buffer[pos] = sample;
    }

    fn len(&self) -> usize {
        self.buffer.len()
    }
}

/// Dattorro plate reverb, ported from Mutable Instruments Clouds.
/// Original: https://github.com/pichenettes/eurorack/blob/master/clouds/dsp/fx/reverb.h
/// Copyright 2014 Emilie Gillet, licensed under MIT.
struct DattorroReverb {
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
    input_gain: f32,
    reverb_time: f32,
    diffusion: f32,
    lp: f32,
    lp1_state: f32,
    lp2_state: f32,
    lfo1_cos: f32,
    lfo1_sin: f32,
    lfo2_cos: f32,
    lfo2_sin: f32,
    lfo_counter: u32,
    amount: f32,
    amount_lfo_phase: f32,
    amount_lfo_rate: f32,
    amount_min: f32,
    amount_max: f32,
    sample_rate: f32,
}

impl DattorroReverb {
    fn new(
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
    fn allpass(delay: &mut DelayLine, input: f32, g: f32) -> f32 {
        let delayed = delay.read_at(delay.len() - 1);
        let v = input + g * delayed;
        let output = delayed - g * v;
        delay.write_and_advance(v);
        output
    }

    fn process(&mut self, input: f32) -> (f32, f32) {
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
struct PluckVoice {
    delay: DelayLine,
    period: usize,
    lp_state: f32,
    feedback: f32,
    active: bool,
}

impl PluckVoice {
    fn new(max_delay: usize) -> Self {
        Self {
            delay: DelayLine::new(max_delay),
            period: max_delay,
            lp_state: 0.0,
            feedback: 0.996,
            active: false,
        }
    }

    fn next_sample(&mut self) -> f32 {
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
struct BbdDelay {
    buffer: DelayLine,
    lp_state: f32,
    feedback: f32,
    delay_samples: f32,
    wobble_phase: f32,
    wobble_rate: f32,
    wobble_depth: f32,
    mix: f32,
    sample_rate: f32,
}

impl BbdDelay {
    fn new(
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

    fn process(&mut self, input: f32) -> f32 {
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
struct PluckEngine {
    voices: Vec<PluckVoice>,
    bbd_left: BbdDelay,
    bbd_right: BbdDelay,
    freqs: Vec<f32>,
    next_pluck_in: u32,
    min_interval: u32,
    max_interval: u32,
    amplitude: f32,
    rng_state: u64,
}

impl PluckEngine {
    fn new(freqs: Vec<f32>, amplitude: f32, sample_rate: f32, rng: &mut impl Rng) -> Self {
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

    fn xorshift(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    fn next_sample(&mut self) -> (f32, f32) {
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
struct Layer {
    oscillators: Vec<Oscillator>,
    filter: Option<ResonantLpf>,
    reverb: Option<DattorroReverb>,
}

impl Layer {
    fn new(freqs: &[f32], amplitude: f32, sparsity: f32, sample_rate: f32, rng: &mut impl Rng) -> Self {
        let oscillators = freqs
            .iter()
            .map(|&f| Oscillator::new(f, amplitude, sparsity, sample_rate, rng))
            .collect();
        Self { oscillators, filter: None, reverb: None }
    }

    fn with_filter(mut self, filter: ResonantLpf) -> Self {
        self.filter = Some(filter);
        self
    }

    fn with_reverb(mut self, reverb: DattorroReverb) -> Self {
        self.reverb = Some(reverb);
        self
    }

    fn next_sample(&mut self) -> (f32, f32) {
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
