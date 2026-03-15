use wasm_bindgen::prelude::*;
use musicbox_core::MusicBox;

/// WASM wrapper around the MusicBox DSP engine.
/// Holds the engine and reusable render buffers.
#[wasm_bindgen]
pub struct MusicBoxWeb {
    engine: MusicBox,
    left: Vec<f32>,
    right: Vec<f32>,
}

#[wasm_bindgen]
impl MusicBoxWeb {
    /// Create a new engine with the given sample rate and RNG seed.
    #[wasm_bindgen(constructor)]
    pub fn new(sample_rate: u32, seed: u64) -> Self {
        Self {
            engine: MusicBox::new(sample_rate, seed),
            left: Vec::new(),
            right: Vec::new(),
        }
    }

    /// Render `frames` stereo samples. Returns interleaved [L, R, L, R, ...] f32 data.
    /// AudioWorkletProcessor.process() provides output buffers of 128 frames,
    /// but we support arbitrary sizes.
    pub fn render(&mut self, frames: usize) -> Vec<f32> {
        // Resize internal buffers if needed
        if self.left.len() < frames {
            self.left.resize(frames, 0.0);
            self.right.resize(frames, 0.0);
        }

        self.engine.render(&mut self.left[..frames], &mut self.right[..frames]);

        // Interleave for easy consumption in JS
        let mut out = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            out.push(self.left[i]);
            out.push(self.right[i]);
        }
        out
    }

    /// Signal the engine to begin fading out.
    pub fn start_fade_out(&mut self) {
        self.engine.start_fade_out();
    }

    /// True once fade-out is complete.
    pub fn is_done(&self) -> bool {
        self.engine.is_done()
    }
}
