# Musicbox Design

A self-contained generative audio and LED art box, running on a Raspberry Pi (or similar), encased in frosted perspex.

## Language

Rust

## Architecture

Single process, shared parameter space. Three concerns running concurrently:

### Parameter Core
- A set of slowly drifting values driven by LFOs at varying rates
- Controls all aspects of audio and visual output
- Later becomes the input point for gyroscope/accelerometer data
- Clean abstraction: consumers read parameters, they don't care about the source

### Audio Engine (real-time thread)
- **Low end:** Slowly evolving sine/triangle oscillators (sub/bass drones)
- **Mids:** Layered evolving oscillators with slow detuning
- **High-mids:** Karplus-Strong pluck synthesis, stochastically triggered, with delay
- **Highs:** White/pink noise through HPF
- **Processing:**
  - Oscillating LPF (cutoff modulated by LFO)
  - Oscillating reverb (wet/dry or decay modulated by LFO)
  - Subtle stereo movement via slow LFO panning and slight L/R detuning
- **Output:** `cpal` crate → ALSA on Pi

### LED Driver (lower priority thread)
- Reads from shared parameter space
- Maps parameter values to colour, brightness, pattern
- Outputs over SPI to LED strip

## Sound Design Summary

| Range | Source | Processing |
|-------|--------|------------|
| Sub/Bass | Slow sine/triangle oscillators | Stereo detuning |
| Mids | Layered evolving oscillators | Oscillating LPF, reverb |
| High-mids | Karplus-Strong plucks | Delay |
| Highs | White/pink noise | HPF |
| Stereo | Slow auto-pan, L/R detuning | Later: gyroscope-driven |

## Implementation Plan

### Phase 1: Audio foundation
1. Scaffold Rust project with `cpal` and `dasp` dependencies
2. Get a simple sine oscillator producing sound
3. Add oscillator bank with layered low/mid voices
4. Implement slow parameter drift (LFOs controlling pitch, amplitude, detuning)

### Phase 2: Sound design
5. Add Karplus-Strong pluck synthesis with stochastic triggering
6. Add delay line for plucks
7. Add noise generator with HPF
8. Implement oscillating LPF
9. Implement reverb with oscillating parameters
10. Implement stereo field (panning LFOs, L/R detuning)

### Phase 3: Parameter abstraction
11. Extract shared parameter space into its own module
12. Wire audio engine to read from parameter space
13. Add LED driver reading from same parameter space (SPI output)

### Phase 4: Motion input
14. Integrate accelerometer/gyroscope as parameter source
15. Blend motion input with LFO drift
