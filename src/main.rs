use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rand::Rng;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

const SAMPLE_RATE: f32 = 44100.0;

// Fade duration in seconds
const FADE_DURATION: f32 = 3.0;
const FADE_SAMPLES: u32 = (SAMPLE_RATE * FADE_DURATION) as u32;

// Master states
const STATE_FADING_IN: u8 = 0;
const STATE_PLAYING: u8 = 1;
const STATE_FADING_OUT: u8 = 2;
const STATE_DONE: u8 = 3;

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
}

impl Oscillator {
    fn new(freq: f32, amplitude: f32, sparsity: f32, rng: &mut impl Rng) -> Self {
        Self {
            phase: rng.r#gen::<f32>(),
            freq,
            // Random starting phase so oscillators don't all fade in together
            envelope_phase: rng.r#gen::<f32>(),
            // Each envelope cycles over ~20-80 seconds
            envelope_rate: rng.r#gen_range(0.012..0.05),
            drift_phase: rng.r#gen::<f32>(),
            drift_rate: rng.r#gen_range(0.02..0.08),
            drift_amount: freq * 0.003,
            amplitude,
            sparsity,
        }
    }

    /// Generate the next sample and advance all phases.
    fn next_sample(&mut self) -> f32 {
        // Envelope: sine LFO mapped through sparsity threshold.
        // Higher sparsity = oscillator must reach a higher LFO value to be heard,
        // so it spends more of its cycle silent.
        let envelope_raw = (self.envelope_phase * std::f32::consts::TAU).sin();
        // Map [-1, 1] to [0, 1]
        let envelope_01 = (envelope_raw + 1.0) * 0.5;
        // Apply sparsity: only the portion above the threshold is audible
        let envelope = ((envelope_01 - self.sparsity) / (1.0 - self.sparsity)).clamp(0.0, 1.0);
        // Squared curve for smoother fade in/out
        let envelope = envelope * envelope;

        // Pitch drift
        let drift = (self.drift_phase * std::f32::consts::TAU).sin() * self.drift_amount;
        let current_freq = self.freq + drift;

        // Sine wave oscillator
        let sample = (self.phase * std::f32::consts::TAU).sin();

        // Advance phases
        self.phase += current_freq / SAMPLE_RATE;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        self.envelope_phase += self.envelope_rate / SAMPLE_RATE;
        if self.envelope_phase >= 1.0 {
            self.envelope_phase -= 1.0;
        }
        self.drift_phase += self.drift_rate / SAMPLE_RATE;
        if self.drift_phase >= 1.0 {
            self.drift_phase -= 1.0;
        }

        sample * envelope * self.amplitude
    }
}

/// A resonant low-pass filter using a state-variable filter topology.
/// Simple, stable, and sounds musical with moderate resonance.
struct ResonantLpf {
    /// Filter state: low-pass output
    low: f32,
    /// Filter state: band-pass output
    band: f32,
    /// LFO phase for cutoff modulation
    cutoff_lfo_phase: f32,
    /// LFO rate in Hz (how fast the cutoff sweeps)
    cutoff_lfo_rate: f32,
    /// Minimum cutoff frequency in Hz
    cutoff_min: f32,
    /// Maximum cutoff frequency in Hz
    cutoff_max: f32,
    /// Resonance amount (0.0 = none, ~0.8 = strong, >1.0 = self-oscillation)
    resonance: f32,
}

impl ResonantLpf {
    fn new(cutoff_min: f32, cutoff_max: f32, resonance: f32, lfo_rate: f32, rng: &mut impl Rng) -> Self {
        Self {
            low: 0.0,
            band: 0.0,
            cutoff_lfo_phase: rng.r#gen::<f32>(),
            cutoff_lfo_rate: lfo_rate,
            cutoff_min,
            cutoff_max,
            resonance,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        // Sweep cutoff via LFO
        let lfo = (self.cutoff_lfo_phase * std::f32::consts::TAU).sin();
        let cutoff = self.cutoff_min + (self.cutoff_max - self.cutoff_min) * (lfo + 1.0) * 0.5;

        // SVF coefficient: f = 2 * sin(pi * cutoff / sample_rate)
        let f = (std::f32::consts::PI * cutoff / SAMPLE_RATE).sin() * 2.0;
        // Q from resonance: lower q = more resonance
        let q = 1.0 - self.resonance;

        // Two-pass for better stability at higher cutoffs
        for _ in 0..2 {
            let high = input - self.low - q * self.band;
            self.band += f * high;
            self.low += f * self.band;
        }

        // Advance LFO
        self.cutoff_lfo_phase += self.cutoff_lfo_rate / SAMPLE_RATE;
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

    /// Read at a delay offset (in samples) behind the write head.
    fn read_at(&self, delay: usize) -> f32 {
        let len = self.buffer.len();
        let pos = (self.write_pos + len - delay) % len;
        self.buffer[pos]
    }

    /// Read with fractional delay using linear interpolation.
    fn read_at_f(&self, delay: f32) -> f32 {
        let delay = delay.max(1.0); // safety: never read at delay < 1
        let len = self.buffer.len();
        let d_floor = delay.floor() as usize;
        let frac = delay - delay.floor();
        let a = self.read_at(d_floor.min(len - 1));
        let b = self.read_at((d_floor + 1).min(len - 1));
        a + (b - a) * frac
    }

    /// Write at a specific offset behind the write head (for the ap1 modulation trick).
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
/// Topology: 4 input allpass diffusers → 2 cross-coupled tank branches
/// with allpass filters, delay lines, LP damping, and LFO modulation.
struct DattorroReverb {
    // Delay lengths scaled from 32kHz to 44.1kHz (multiply by 44100/32000 ≈ 1.378)
    // Input diffuser allpasses
    ap1: DelayLine, // 156 (was 113)
    ap2: DelayLine, // 223 (was 162)
    ap3: DelayLine, // 332 (was 241)
    ap4: DelayLine, // 550 (was 399)

    // Tank branch 1
    dap1a: DelayLine, // 2278 (was 1653)
    dap1b: DelayLine, // 2808 (was 2038)
    del1: DelayLine,  // 4700 (was 3411)

    // Tank branch 2
    dap2a: DelayLine, // 2636 (was 1913)
    dap2b: DelayLine, // 2291 (was 1663)
    del2: DelayLine,  // 6590 (was 4782)

    // Parameters
    input_gain: f32,
    reverb_time: f32, // krt: feedback amount in tank (0.35 to ~0.98)
    diffusion: f32,   // kap: allpass coefficient
    lp: f32,          // damping coefficient (higher = less damping)

    // LP filter states (one per tank branch)
    lp1_state: f32,
    lp2_state: f32,

    // LFO for modulation (cosine oscillator)
    lfo1_cos: f32,
    lfo1_sin: f32,
    lfo2_cos: f32,
    lfo2_sin: f32,
    lfo_counter: u32,

    // Wet/dry
    amount: f32,
    amount_lfo_phase: f32,
    amount_lfo_rate: f32,
    amount_min: f32,
    amount_max: f32,
}

impl DattorroReverb {
    fn new(
        reverb_time: f32,
        amount_min: f32,
        amount_max: f32,
        amount_lfo_rate: f32,
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
            lp: 0.82, // higher = less damping, more highs preserved
            lp1_state: 0.0,
            lp2_state: 0.0,
            // Initialize LFO cosine oscillators
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
        }
    }

    /// Correct Schroeder allpass: v = input + g * delayed; output = delayed - g * v
    #[inline]
    fn allpass(delay: &mut DelayLine, input: f32, g: f32) -> f32 {
        let delayed = delay.read_at(delay.len() - 1);
        let v = input + g * delayed;
        let output = delayed - g * v;
        delay.write_and_advance(v);
        output
    }

    /// Process a mono input sample. Returns (left, right) reverb output.
    fn process(&mut self, input: f32) -> (f32, f32) {
        // Modulate wet/dry amount
        let lfo = (self.amount_lfo_phase * std::f32::consts::TAU).sin();
        self.amount = self.amount_min + (self.amount_max - self.amount_min) * (lfo + 1.0) * 0.5;
        self.amount_lfo_phase += self.amount_lfo_rate / SAMPLE_RATE;
        if self.amount_lfo_phase >= 1.0 {
            self.amount_lfo_phase -= 1.0;
        }

        // Update LFOs every 32 samples
        if self.lfo_counter & 31 == 0 {
            let lfo1_freq = 0.5 / SAMPLE_RATE;
            let w1 = std::f32::consts::TAU * lfo1_freq * 32.0;
            let new_cos1 = self.lfo1_cos * w1.cos() - self.lfo1_sin * w1.sin();
            let new_sin1 = self.lfo1_cos * w1.sin() + self.lfo1_sin * w1.cos();
            self.lfo1_cos = new_cos1;
            self.lfo1_sin = new_sin1;

            let lfo2_freq = 0.3 / SAMPLE_RATE;
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

        // --- AP1 modulation trick from Clouds ---
        let lfo1_uni = (self.lfo1_cos + 1.0) * 0.5;
        let ap1_mod_delay = 14.0 + lfo1_uni * 120.0;
        let ap1_mod_read = self.ap1.read_at_f(ap1_mod_delay);
        self.ap1.write_at(138, ap1_mod_read);

        // --- Input diffusion: 4 series allpass filters ---
        let sig = input * self.input_gain;
        let ap1_out = Self::allpass(&mut self.ap1, sig, kap);
        let ap2_out = Self::allpass(&mut self.ap2, ap1_out, kap);
        let ap3_out = Self::allpass(&mut self.ap3, ap2_out, kap);
        let apout = Self::allpass(&mut self.ap4, ap3_out, kap);

        // --- Tank branch 1 ---
        let lfo2_uni = (self.lfo2_cos + 1.0) * 0.5;
        let del2_mod_delay = 6311.0 + lfo2_uni * 276.0;
        let del2_read = self.del2.read_at_f(del2_mod_delay);
        let tank1_in = apout + del2_read * krt;

        // LP damping
        self.lp1_state += klp * (tank1_in - self.lp1_state);
        let lp1_out = self.lp1_state;

        // Tank allpasses
        let dap1a_out = Self::allpass(&mut self.dap1a, lp1_out, kap);
        let dap1b_out = Self::allpass(&mut self.dap1b, dap1a_out, kap);
        self.del1.write_and_advance(dap1b_out);

        // --- Tank branch 2 ---
        let del1_read = self.del1.read_at(self.del1.len() - 1);
        let tank2_in = apout + del1_read * krt;

        // LP damping
        self.lp2_state += klp * (tank2_in - self.lp2_state);
        let lp2_out = self.lp2_state;

        // Tank allpasses
        let dap2a_out = Self::allpass(&mut self.dap2a, lp2_out, kap);
        let dap2b_out = Self::allpass(&mut self.dap2b, dap2a_out, kap);
        self.del2.write_and_advance(dap2b_out);

        // --- Output taps ---
        let wet_l = del1_read;
        let wet_r = del2_read;

        let left = input * (1.0 - self.amount) + wet_l * self.amount;
        let right = input * (1.0 - self.amount) + wet_r * self.amount;

        (left, right)
    }
}

/// Karplus-Strong pluck synthesis: a short burst of noise into a delay line
/// with filtered feedback. The delay length sets the pitch, the feedback
/// filtering makes it decay like a plucked string.
struct PluckVoice {
    delay: DelayLine,
    /// One-pole LP state for the feedback filter (string damping)
    lp_state: f32,
    /// Feedback amount (0.0-1.0, higher = longer sustain)
    feedback: f32,
    /// Whether this voice is currently active
    active: bool,
}

impl PluckVoice {
    fn new(max_delay: usize) -> Self {
        Self {
            delay: DelayLine::new(max_delay),
            lp_state: 0.0,
            feedback: 0.996,
            active: false,
        }
    }

    fn next_sample(&mut self) -> f32 {
        if !self.active {
            return 0.0;
        }

        let out = self.delay.read_at(self.delay.len() - 1);

        // Karplus-Strong averaging filter: average adjacent samples, then feedback
        let prev = self.delay.read_at(self.delay.len() - 2);
        let averaged = (out + prev) * 0.5;

        // One-pole LP for extra warmth
        self.lp_state += 0.7 * (averaged - self.lp_state);
        let fed_back = self.lp_state * self.feedback;

        self.delay.write_and_advance(fed_back);

        // Deactivate when energy is negligible
        if out.abs() < 0.0001 {
            self.active = false;
        }

        out
    }
}

/// BBD (Bucket Brigade Device) delay emulation.
/// Characteristics: limited bandwidth, clock wobble, gentle rolloff.
struct BbdDelay {
    /// The main delay buffer (the "buckets")
    buffer: DelayLine,
    /// One-pole LP filter per tap to emulate bandwidth limiting
    lp_state: f32,
    /// Feedback amount
    feedback: f32,
    /// Delay time in samples (base)
    delay_samples: f32,
    /// LFO phase for clock wobble (modulates delay time)
    wobble_phase: f32,
    /// Wobble rate in Hz
    wobble_rate: f32,
    /// Wobble depth in samples
    wobble_depth: f32,
    /// Wet/dry mix
    mix: f32,
}

impl BbdDelay {
    fn new(
        delay_ms: f32,
        feedback: f32,
        mix: f32,
        wobble_rate: f32,
        wobble_depth_ms: f32,
        rng: &mut impl Rng,
    ) -> Self {
        let delay_samples = delay_ms * SAMPLE_RATE / 1000.0;
        let max_samples = (delay_samples + wobble_depth_ms * SAMPLE_RATE / 1000.0 + 100.0) as usize;
        Self {
            buffer: DelayLine::new(max_samples),
            lp_state: 0.0,
            feedback,
            delay_samples,
            wobble_phase: rng.r#gen::<f32>(),
            wobble_rate,
            wobble_depth: wobble_depth_ms * SAMPLE_RATE / 1000.0,
            mix,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        // Clock wobble: modulate delay time with slow LFO
        let wobble = (self.wobble_phase * std::f32::consts::TAU).sin() * self.wobble_depth;
        let current_delay = self.delay_samples + wobble;

        // Read from the delay with interpolation
        let delayed = self.buffer.read_at_f(current_delay);

        // BBD bandwidth limiting: one-pole LP at each read (~4kHz rolloff)
        self.lp_state += 0.45 * (delayed - self.lp_state);
        let filtered = self.lp_state;

        // Write input + feedback into the buffer
        let write_sample = input + filtered * self.feedback;
        self.buffer.write_and_advance(write_sample);

        // Advance wobble LFO
        self.wobble_phase += self.wobble_rate / SAMPLE_RATE;
        if self.wobble_phase >= 1.0 {
            self.wobble_phase -= 1.0;
        }

        // Mix dry + wet
        input * (1.0 - self.mix) + filtered * self.mix
    }
}

/// Engine that stochastically triggers pluck voices and feeds them through
/// a BBD delay. Produces stereo output via two slightly different BBD delays.
struct PluckEngine {
    voices: Vec<PluckVoice>,
    bbd_left: BbdDelay,
    bbd_right: BbdDelay,
    /// Available frequencies to pluck from (upper-mid pentatonic)
    freqs: Vec<f32>,
    /// Countdown in samples until next pluck
    next_pluck_in: u32,
    /// Min/max interval between plucks in samples
    min_interval: u32,
    max_interval: u32,
    /// Overall amplitude
    amplitude: f32,
    /// RNG state (simple xorshift for audio thread — no allocations)
    rng_state: u64,
}

impl PluckEngine {
    fn new(freqs: Vec<f32>, amplitude: f32, rng: &mut impl Rng) -> Self {
        let num_voices = 6; // polyphony
        let voices = (0..num_voices)
            .map(|_| PluckVoice::new(512)) // max ~86Hz, we'll use higher
            .collect();

        // Two BBD delays with slightly different times for stereo width
        let bbd_left = BbdDelay::new(340.0, 0.35, 0.5, 0.3, 1.5, rng);
        let bbd_right = BbdDelay::new(370.0, 0.35, 0.5, 0.25, 1.8, rng);

        // Plucks every 1-4 seconds
        let min_interval = (SAMPLE_RATE * 1.0) as u32;
        let max_interval = (SAMPLE_RATE * 4.0) as u32;

        let rng_state = rng.r#gen::<u64>() | 1; // ensure non-zero

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

    /// Simple xorshift64 RNG for the audio thread (no allocation, no locking).
    fn xorshift(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    fn next_sample(&mut self) -> (f32, f32) {
        // Check if it's time to trigger a new pluck
        if self.next_pluck_in == 0 {
            // Pick a random frequency
            let freq_idx = (Self::xorshift(&mut self.rng_state) as usize) % self.freqs.len();
            let freq = self.freqs[freq_idx];
            let period = (SAMPLE_RATE / freq) as usize;

            // Find an inactive voice index (or steal voice 0)
            let voice_idx = self
                .voices
                .iter()
                .position(|v| !v.active)
                .unwrap_or(0);

            // Fill the voice's delay with noise for the pluck
            let voice = &mut self.voices[voice_idx];
            for s in voice.delay.buffer.iter_mut() {
                *s = 0.0;
            }
            for i in 0..period.min(voice.delay.len()) {
                let r = Self::xorshift(&mut self.rng_state);
                voice.delay.buffer[i] = (r as f32) / (u64::MAX as f32) - 0.5;
            }
            voice.delay.write_pos = period % voice.delay.len();
            voice.lp_state = 0.0;
            voice.active = true;

            // Schedule next pluck
            let range = self.max_interval - self.min_interval;
            self.next_pluck_in =
                self.min_interval + (Self::xorshift(&mut self.rng_state) as u32) % range;
        } else {
            self.next_pluck_in -= 1;
        }

        // Sum all active voices
        let dry: f32 = self.voices.iter_mut().map(|v| v.next_sample()).sum::<f32>() * self.amplitude;

        // Feed through BBD delays for stereo
        let left = self.bbd_left.process(dry);
        let right = self.bbd_right.process(dry);

        (left, right)
    }
}

/// A layer of oscillators in a frequency range, with optional effects.
/// Returns stereo (L, R) samples.
struct Layer {
    oscillators: Vec<Oscillator>,
    filter: Option<ResonantLpf>,
    reverb: Option<DattorroReverb>,
}

impl Layer {
    fn new(freqs: &[f32], amplitude: f32, sparsity: f32, rng: &mut impl Rng) -> Self {
        let oscillators = freqs
            .iter()
            .map(|&f| Oscillator::new(f, amplitude, sparsity, rng))
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

    /// Returns (left, right) sample pair.
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
struct MusicBox {
    bass: Layer,
    mid: Layer,
    high: Layer,
    plucks: PluckEngine,
    limiter_gain: f32,
    fade_pos: u32,
    fade_state: u8,
}

impl MusicBox {
    fn new(rng: &mut impl Rng) -> Self {
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

        println!(
            "Layers — bass: {} osc, mid: {} osc, high: {} osc, plucks: {} notes",
            bass_freqs.len(),
            mid_freqs.len(),
            high_freqs.len(),
            pluck_freqs.len()
        );

        let bass = Layer::new(&bass_freqs, 0.15, 0.7, rng);
        let mid_filter = ResonantLpf::new(200.0, 1200.0, 0.3, 0.065, rng);
        let mid = Layer::new(&mid_freqs, 0.08, 0.5, rng).with_filter(mid_filter);
        let high_reverb = DattorroReverb::new(0.85, 0.4, 0.7, 0.04, rng);
        let high = Layer::new(&high_freqs, 0.04, 0.3, rng).with_reverb(high_reverb);
        let plucks = PluckEngine::new(pluck_freqs, 0.3, rng);

        Self {
            bass,
            mid,
            high,
            plucks,
            limiter_gain: 1.0,
            fade_pos: 0,
            fade_state: STATE_FADING_IN,
        }
    }

    /// Begin fade-out.
    fn start_fade_out(&mut self) {
        if self.fade_state == STATE_FADING_IN || self.fade_state == STATE_PLAYING {
            self.fade_state = STATE_FADING_OUT;
        }
    }

    fn is_done(&self) -> bool {
        self.fade_state == STATE_DONE
    }

    /// Generate the next stereo sample pair, including fade and limiting.
    fn next_sample(&mut self) -> (f32, f32) {
        // Master fade
        let master_gain = match self.fade_state {
            STATE_FADING_IN => {
                self.fade_pos += 1;
                if self.fade_pos >= FADE_SAMPLES {
                    self.fade_state = STATE_PLAYING;
                }
                let t = self.fade_pos as f32 / FADE_SAMPLES as f32;
                t * t
            }
            STATE_PLAYING => 1.0,
            STATE_FADING_OUT => {
                if self.fade_pos == 0 {
                    self.fade_state = STATE_DONE;
                    0.0
                } else {
                    self.fade_pos = self.fade_pos.saturating_sub(1);
                    let t = self.fade_pos as f32 / FADE_SAMPLES as f32;
                    t * t
                }
            }
            _ => 0.0,
        };

        if self.fade_state == STATE_DONE {
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
fn parse_duration(s: &str) -> Option<f32> {
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
    // If there's a trailing number with no unit, treat as seconds
    if !num_buf.is_empty() {
        let n: f32 = num_buf.parse().ok()?;
        total += n;
    }
    if total > 0.0 { Some(total) } else { None }
}

fn run_live() {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("no audio output device found");

    println!("Using audio device: {}", device.name().unwrap_or_default());

    let config = device.default_output_config().unwrap();
    println!("Audio format: {:?}", config);

    let channels = config.channels() as usize;
    let mut rng = rand::thread_rng();
    let mut engine = MusicBox::new(&mut rng);

    // Signal handler for graceful fade-out
    let master_state = Arc::new(AtomicU8::new(STATE_FADING_IN));
    let master_state_signal = Arc::clone(&master_state);

    ctrlc::set_handler(move || {
        let prev = master_state_signal.load(Ordering::Relaxed);
        if prev == STATE_FADING_IN || prev == STATE_PLAYING {
            println!("\nFading out...");
            master_state_signal.store(STATE_FADING_OUT, Ordering::Relaxed);
        }
    })
    .expect("failed to set signal handler");

    let master_state_cb = Arc::clone(&master_state);
    let callback = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
        // Check if signal handler requested fade-out
        let sig_state = master_state_cb.load(Ordering::Relaxed);
        if sig_state == STATE_FADING_OUT && !engine.is_done() {
            engine.start_fade_out();
        }

        for frame in data.chunks_mut(channels) {
            let (left, right) = engine.next_sample();

            if channels >= 2 {
                frame[0] = left;
                frame[1] = right;
                for s in frame[2..].iter_mut() {
                    *s = (left + right) * 0.5;
                }
            } else {
                frame[0] = (left + right) * 0.5;
            }

            if engine.is_done() {
                master_state_cb.store(STATE_DONE, Ordering::Relaxed);
            }
        }
    };

    let err_callback = |err: cpal::StreamError| {
        eprintln!("Audio stream error: {}", err);
    };

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device
            .build_output_stream(&config.into(), callback, err_callback, None)
            .unwrap(),
        _ => panic!("unsupported sample format — expected F32"),
    };

    stream.play().unwrap();
    println!("musicbox is playing... press Ctrl+C to stop");

    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if master_state.load(Ordering::Relaxed) == STATE_DONE {
            std::thread::sleep(std::time::Duration::from_millis(200));
            println!("Goodbye.");
            break;
        }
    }
}

fn run_render(duration_str: &str, output_path: &str) {
    let duration_secs = parse_duration(duration_str)
        .unwrap_or_else(|| {
            eprintln!("Invalid duration: '{}'. Examples: 10m, 1h30m, 90s", duration_str);
            std::process::exit(1);
        });

    let total_samples = (SAMPLE_RATE * duration_secs) as u64;
    // Add fade-out at the end
    let body_samples = total_samples - FADE_SAMPLES as u64;

    let mut rng = rand::thread_rng();
    let mut engine = MusicBox::new(&mut rng);

    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SAMPLE_RATE as u32,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };

    let mut writer = hound::WavWriter::create(output_path, spec)
        .unwrap_or_else(|e| {
            eprintln!("Failed to create {}: {}", output_path, e);
            std::process::exit(1);
        });

    println!(
        "Rendering {:.0}s ({}) to {}...",
        duration_secs, duration_str, output_path
    );

    let progress_interval = SAMPLE_RATE as u64 * 10; // print every 10 seconds

    for i in 0..total_samples {
        // Trigger fade-out at the right time
        if i == body_samples {
            engine.start_fade_out();
        }

        let (left, right) = engine.next_sample();
        writer.write_sample(left).unwrap();
        writer.write_sample(right).unwrap();

        if i % progress_interval == 0 && i > 0 {
            let pct = (i as f32 / total_samples as f32) * 100.0;
            print!("\r  {:.0}% ({:.0}s / {:.0}s)", pct, i as f32 / SAMPLE_RATE, duration_secs);
        }
    }

    writer.finalize().unwrap();
    println!("\r  100% — wrote {}", output_path);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() >= 3 && args[1] == "--render" {
        // --render <duration> [output.wav]
        let duration = &args[2];
        let output = if args.len() >= 4 {
            args[3].as_str()
        } else {
            "musicbox.wav"
        };
        run_render(duration, output);
    } else if args.len() >= 2 && (args[1] == "--help" || args[1] == "-h") {
        println!("musicbox v0.1.0 — generative ambient audio");
        println!();
        println!("Usage:");
        println!("  musicbox              Play live (Ctrl+C to fade out and stop)");
        println!("  musicbox --render <duration> [output.wav]");
        println!();
        println!("Duration examples: 10m, 1h30m, 90s, 5m30s");
        println!();
        println!("Each run is unique — the generative engine is seeded randomly.");
    } else {
        run_live();
    }
}
