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
