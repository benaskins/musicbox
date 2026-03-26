use wasm_bindgen::prelude::*;
use musicbox_core::{MusicBox, Track};
use musicbox_core::tracks::ambient_techno::AmbientTechno;

/// WASM wrapper around the MusicBox DSP engine.
/// Holds the engine and a persistent interleaved output buffer.
#[wasm_bindgen]
pub struct MusicBoxWeb {
    engine: MusicBox,
    left: Vec<f32>,
    right: Vec<f32>,
    interleaved: Vec<f32>,
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
            interleaved: Vec::new(),
        }
    }

    /// Render `frames` stereo samples into an internal buffer.
    /// Call `output_ptr()` and `output_len()` to read the interleaved result.
    pub fn render(&mut self, frames: usize) {
        if self.left.len() < frames {
            self.left.resize(frames, 0.0);
            self.right.resize(frames, 0.0);
            self.interleaved.resize(frames * 2, 0.0);
        }

        self.engine.render(&mut self.left[..frames], &mut self.right[..frames]);

        for i in 0..frames {
            self.interleaved[i * 2] = self.left[i];
            self.interleaved[i * 2 + 1] = self.right[i];
        }
    }

    /// Pointer to the interleaved output buffer in WASM memory.
    pub fn output_ptr(&self) -> *const f32 {
        self.interleaved.as_ptr()
    }

    /// Length of the interleaved output buffer (frames * 2).
    pub fn output_len(&self) -> usize {
        self.interleaved.len()
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

/// WASM wrapper around the AmbientTechno experiment engine.
#[wasm_bindgen]
pub struct AmbientTechnoWeb {
    engine: AmbientTechno,
    left: Vec<f32>,
    right: Vec<f32>,
    interleaved: Vec<f32>,
}

#[wasm_bindgen]
impl AmbientTechnoWeb {
    #[wasm_bindgen(constructor)]
    pub fn new(sample_rate: u32, seed: u64) -> Self {
        Self {
            engine: AmbientTechno::new(sample_rate, seed),
            left: Vec::new(),
            right: Vec::new(),
            interleaved: Vec::new(),
        }
    }

    pub fn render(&mut self, frames: usize) {
        if self.left.len() < frames {
            self.left.resize(frames, 0.0);
            self.right.resize(frames, 0.0);
            self.interleaved.resize(frames * 2, 0.0);
        }

        self.engine.render(&mut self.left[..frames], &mut self.right[..frames]);

        for i in 0..frames {
            self.interleaved[i * 2] = self.left[i];
            self.interleaved[i * 2 + 1] = self.right[i];
        }
    }

    pub fn output_ptr(&self) -> *const f32 {
        self.interleaved.as_ptr()
    }

    pub fn output_len(&self) -> usize {
        self.interleaved.len()
    }

    pub fn start_fade_out(&mut self) {
        self.engine.start_fade_out();
    }

    pub fn is_done(&self) -> bool {
        self.engine.is_done()
    }

    pub fn set_param(&mut self, name: &str, value: f32) {
        self.engine.set_param(name, value);
    }
}
