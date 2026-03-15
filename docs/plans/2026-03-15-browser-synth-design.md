# Browser Synth Design

Take musicbox from a CLI-only Rust binary to a browser-playable synth, while keeping a single source of truth for all DSP code.

## Goals

1. **Listen without installing Rust** — play the generative synth in a browser tab
2. **Foundation for UI** — extensible with controls and visualizations later

## Approach

Compile the DSP core to WebAssembly. A thin JS layer handles Web Audio plumbing. The Rust DSP code is shared between native and browser targets — no reimplementation.

## Crate Structure

Cargo workspace with separate crates for core DSP, CLI host, and (future) WASM bridge:

```
musicbox/
├── Cargo.toml              (workspace root)
├── musicbox-core/          (lib — all DSP, no platform deps)
│   ├── Cargo.toml          (deps: rand)
│   └── src/lib.rs
├── musicbox-cli/           (bin — platform IO)
│   ├── Cargo.toml          (deps: musicbox-core, cpal, hound, ctrlc)
│   └── src/main.rs
└── musicbox-web/           (future — WASM bridge + JS host)
    ├── Cargo.toml          (deps: musicbox-core, wasm-bindgen, getrandom/js)
    ├── src/lib.rs           (wasm-bindgen exports)
    └── www/                 (AudioWorklet JS, HTML)
```

## Core Library API

`musicbox-core` exposes a platform-agnostic `MusicBox` struct:

```rust
pub struct MusicBox { /* ... */ }

impl MusicBox {
    /// Construct with explicit sample rate and RNG seed.
    /// Each host sources entropy its own way (thread_rng, Math.random, etc).
    pub fn new(sample_rate: u32, seed: u64) -> Self;

    /// Fill split stereo buffers. Host calls this per audio callback.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]);

    /// Signal the synth to begin fading out.
    pub fn start_fade_out(&mut self);

    /// True once fade-out is complete and output is silent.
    pub fn is_done(&self) -> bool;
}
```

### Design decisions

- **Explicit seed** — `new()` takes a `u64` seed instead of calling `thread_rng()` internally. The CLI seeds from `rand::thread_rng()`, the browser host seeds from `Math.random()` or `crypto.getRandomValues()`. Keeps the core free of platform-specific entropy.
- **Plain enum state** — fade state (`FadingIn`, `Playing`, `FadingOut`, `Done`) is a plain Rust enum inside `MusicBox`, not an `AtomicU8`. The CLI wrapper owns the `AtomicU8` + Ctrl+C signal handler and calls `start_fade_out()` when triggered. The WASM host does the same from a JS event.
- **Split stereo buffers** — `render(left, right)` rather than interleaved, since both cpal and AudioWorklet can work with split channels and it avoids shuffling inside the core.
- **No DSP changes** — the synthesis logic (oscillators, filters, Dattorro reverb, Karplus-Strong, BBD delay, mixer, limiter) moves as-is. This is a structural refactor, not a sound design change.

## Implementation Plan

### Step 1: Create workspace structure

- Convert root `Cargo.toml` to a workspace definition
- Create `musicbox-core/Cargo.toml` with `rand` dependency
- Create `musicbox-cli/Cargo.toml` with `musicbox-core`, `cpal`, `hound`, `ctrlc`

### Step 2: Extract DSP into musicbox-core

- Move all DSP types into `musicbox-core/src/lib.rs`: `Oscillator`, `ResonantLpf`, `DelayLine`, `DattorroReverb`, `PluckVoice`, `BbdDelay`, `PluckEngine`, `MusicBox`
- Move `pentatonic_frequencies()` and constants (`SAMPLE_RATE`)
- Replace `AtomicU8` state with a plain `State` enum
- Change `MusicBox::new()` to accept `(sample_rate: u32, seed: u64)`
- Add `render(&mut self, left: &mut [f32], right: &mut [f32])` method
- Add `start_fade_out()` and `is_done()` methods
- Remove all `cpal`, `hound`, `ctrlc` usage from core

### Step 3: Build musicbox-cli on top of core

- `musicbox-cli/src/main.rs` handles arg parsing, cpal setup, WAV rendering, Ctrl+C
- Uses `AtomicU8` + `ctrlc` handler to bridge signal → `musicbox.start_fade_out()`
- Calls `musicbox.render()` in the cpal callback, then interleaves into cpal's buffer
- Calls `musicbox.render()` in a loop for WAV rendering

### Step 4: Verify (DONE)

- `cargo build --release` from workspace root
- `cargo run -p musicbox-cli --release` produces identical audio behaviour
- Render a WAV and confirm it sounds right

### Step 5: Create musicbox-web WASM bridge

- Add `musicbox-web` to workspace
- `musicbox-web/Cargo.toml`: deps on `musicbox-core`, `wasm-bindgen`, `getrandom` with `js` feature
- `musicbox-web/src/lib.rs`: `#[wasm_bindgen]` wrapper exposing `create(seed)`, `render(left, right)`, `start_fade_out()`, `is_done()`
- Build with `wasm-pack build --target web`

### Step 6: AudioWorklet processor + HTML host

- `musicbox-web/www/worklet.js`: `AudioWorkletProcessor` that imports WASM, calls `render()` each frame
- `musicbox-web/www/index.html`: minimal page with start/stop button, loads worklet
- `musicbox-web/www/main.js`: creates `AudioContext`, registers worklet, handles start/stop

### Step 7: Build and verify in browser

- Build WASM with wasm-pack
- Serve `www/` and confirm audio plays in browser
