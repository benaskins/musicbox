# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Setup

**Linux only:** ALSA development headers are required before building — `sudo apt-get install libasound2-dev`. macOS uses CoreAudio and needs no extra dependencies.

## Commands

```bash
# Run live playback
cargo run --release

# Render to WAV file
cargo run --release -- --render 10m output.wav
# Duration formats: 10m, 1h30m, 90s, 5m30s

# Run all tests
cargo test

# Run tests for a specific crate
cargo test -p musicbox-core

# Build web version (requires wasm-pack and Go)
cd musicbox-web && ./build.sh
```

## Architecture

Rust workspace with three crates:

- **musicbox-core** — DSP engine (no I/O); consumed by cli and web
- **musicbox-cli** — binary; live playback via `cpal` or offline WAV render via `hound`
- **musicbox-web** — `wasm-bindgen` wrapper + Go static file server (`cmd/serve/`)

### Core DSP (`musicbox-core/src/`)

`lib.rs` defines the top-level `MusicBox` struct, which mixes four independent `Layer`s:

| Layer | Freq range | Synthesis | Effects |
|-------|-----------|-----------|---------|
| Bass | < 130 Hz | Sine oscillators | — |
| Mid | 130–400 Hz | Sine oscillators | Resonant LPF |
| High | ≥ 400 Hz | Sine oscillators | Dattorro plate reverb |
| Pluck | 330–800 Hz | Karplus-Strong (`PluckVoice`) | BBD delay |

Final output passes through tanh soft clipping and an adaptive peak limiter. Fade in/out are 3-second linear ramps.

`dsp.rs` contains all DSP primitives: `Oscillator`, `ResonantLpf`, `DattorroReverb` (ported from Mutable Instruments), `DelayLine`, `PluckVoice`, `BbdDelay`, `Layer`.

`experiments/ambient_techno.rs` is an alternative generative engine with kick/hi-hat rhythm structures and parameter control via `set_param()`.

### Web (`musicbox-web/`)

The web UI uses the Web Audio API with an `AudioWorklet` (`www/worklet.js`) that calls into the WASM engine each processing block. `www/main.js` manages playback state, parameter knobs, and the animated sigil. The Go server (`cmd/serve/`) serves static files and the compiled WASM module.

### Scale / Seed

The engine uses A-minor pentatonic across multiple octaves. Every run uses a random seed; passing the same seed to `MusicBox::new(seed)` reproduces identical audio.
