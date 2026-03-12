# musicbox v0.1.0

Generative ambient audio written in Rust. Every run is unique.

Layered pentatonic drones with evolving oscillators, resonant filter sweeps,
Karplus-Strong plucks through BBD delay, and a Dattorro plate reverb — all
running in ~89KB of memory.

## Listen

```
cargo run --release
```

Ctrl+C to fade out and stop.

## Render to file

```
cargo run --release -- --render 10m output.wav
```

Renders a 32-bit float stereo WAV. Duration examples: `10m`, `1h30m`, `90s`, `5m30s`.

## What you'll hear

- **Bass:** Sparse sine drones fading in and out across A minor pentatonic
- **Mids:** Layered oscillators through a slowly sweeping resonant low-pass filter
- **High-mids:** Stochastic plucked notes through a warm BBD (bucket brigade) delay
- **Highs:** Shimmering oscillators through a Dattorro plate reverb (ported from Mutable Instruments Clouds)
- **Master:** Peak limiter, 3-second fade-in/out

## Requirements

- [Rust](https://rustup.rs/) (stable)
- An audio output device (for live playback)

## License

[CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/) — Ben Askins, 2026
