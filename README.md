# musicbox v0.1.0

Generative ambient audio written in Rust. Each run sounds different.

Layered pentatonic drones, resonant filter sweeps, Karplus-Strong plucks
through BBD delay, and a Dattorro plate reverb.

## Listen

```
cargo run --release
```

Or for the generative techno style:

```
cargo run --release -- techno
```

Ctrl+C to fade out and stop.

## Render to file

```
cargo run --release -- --render 10m output.wav
```

Renders a 32-bit float stereo WAV. Duration examples: `10m`, `1h30m`, `90s`, `5m30s`.

## What you'll hear

- **Bass:** Sine drones fading in and out across A minor pentatonic
- **Mids:** Oscillator layers through a sweeping resonant low-pass filter
- **High-mids:** Plucked notes through a BBD (bucket brigade) delay
- **Highs:** Oscillators through a Dattorro plate reverb (ported from [Mutable Instruments Clouds](https://github.com/pichenettes/eurorack/blob/master/clouds/dsp/fx/reverb.h))
- **Master:** Peak limiter, 3-second fade-in/out

## Run in the browser

```bash
cargo install wasm-pack
cd musicbox-web && ./build.sh
./bin/serve
```

Then open [http://localhost:8080](http://localhost:8080).

## Requirements

- [Rust](https://rustup.rs/) (stable)
- An audio output device (for live playback)
- **Linux only:** ALSA development headers — `sudo apt-get install libasound2-dev`
- **Web only:** [wasm-pack](https://rustwasm.github.io/wasm-pack/) and [Go](https://go.dev/dl/) (for the browser UI)

## License

[CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/) — Ben Askins, 2026

The Dattorro plate reverb is ported from [Mutable Instruments Eurorack](https://github.com/pichenettes/eurorack)
by Émilie Gillet, licensed under [MIT](https://github.com/pichenettes/eurorack/blob/master/LICENSE).
