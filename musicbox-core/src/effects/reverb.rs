use rand::Rng;
use super::delay::DelayLine;

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
