use rand::Rng;
use rand::SeedableRng;

use crate::effects::{BbdDelay, DattorroReverb, DubDelay, Phaser, ResonantLpf};
use crate::instruments::{ClaveVoice, DubStab, HiHat, Kick, MonoSynth, Snare808, SynthPad};
use crate::clocks::{PulseOscillator, SwingLfo};
use crate::util::prng::Xorshift64;
use crate::track::{State, Track};

/// Counts down an `Option<u32>` timer and runs `$body` when it hits zero.
macro_rules! tick_timer {
    ($timer:expr, $body:block) => {
        if let Some(t) = $timer {
            if t == 0 {
                $body
                $timer = None;
            } else {
                $timer = Some(t - 1);
            }
        }
    };
}

// A natural minor scale, two octaves from A2
const A_MINOR_SCALE: [f32; 15] = [
    110.00, // 0  A2
    123.47, // 1  B2
    130.81, // 2  C3
    146.83, // 3  D3
    164.81, // 4  E3
    174.61, // 5  F3
    196.00, // 6  G3
    220.00, // 7  A3
    246.94, // 8  B3
    261.63, // 9  C4
    293.66, // 10 D4
    329.63, // 11 E4
    349.23, // 12 F4
    392.00, // 13 G4
    440.00, // 14 A4
];
// Base sequence as scale-degree indices (A2=0)
const MONO_SEQ_IDX: [usize; 16] = [
    0, // A2
    2, // C3
    4, // E3
    0, // A2
    2, // C3
    4, // E3
    0, // A2
    2, // C3
    5, // F3
    4, // E3
    0, // A2
    5, // F3
    4, // E3
    2, // C3
    0, // A2
    2, // C3
];

const PAD_FREQS: [f32; 5] = [440.0, 523.25, 587.33, 659.26, 784.0]; // A4–G5

// A minor pentatonic chord voicings — all three notes in scale for each root position.
// Stab1: A2–G3 range.  Stab2: one step lower, same scale.
const STAB1_CHORDS: [[f32; 3]; 5] = [
    [110.00, 130.81, 164.81], // Am:  A2 C3 E3
    [130.81, 164.81, 196.00], // C:   C3 E3 G3
    [146.83, 196.00, 220.00], // Dm7: D3 G3 A3
    [164.81, 196.00, 220.00], // Em7: E3 G3 A3
    [196.00, 220.00, 293.66], // G:   G3 A3 D4
];
const STAB2_CHORDS: [[f32; 3]; 5] = [
    [ 49.00,  55.00,  73.42], // G1 A1 D2
    [ 55.00,  65.41,  82.41], // Am: A1 C2 E2
    [ 65.41,  82.41,  98.00], // C:  C2 E2 G2
    [ 73.42,  98.00, 110.00], // Dm7: D2 G2 A2
    [ 82.41,  98.00, 110.00], // Em7: E2 G2 A2
];
const STAB3_CHORDS: [[f32; 3]; 5] = [
    [ 55.00,  65.41,  82.41], // Am  A1 C2 E2
    [ 65.41,  82.41, 110.00], // Am/C C2 E2 A2
    [ 73.42,  87.31, 110.00], // Dm  D2 F2 A2
    [ 82.41,  98.00, 123.47], // Em  E2 G2 B2
    [ 55.00,  73.42,  87.31], // Dm/A A1 D2 F2
];

/// Fixed 32-step bassline (8 beats / 2 bars). None = rest.
/// Repeating Am figure ending with F3 on the last group.
const BASSLINE_PATTERN: [Option<f32>; 32] = [
    Some(110.00), Some(55.00), Some(110.00), Some(130.81), None, Some(82.41), None, None, // beats 1–2
    Some(110.00), Some(55.00), Some(110.00), Some(130.81), None, Some(82.41), None, None, // beats 3–4
    Some(110.00), Some(55.00), Some(110.00), Some(130.81), None, Some(82.41), None, None, // beats 5–6
    Some(110.00), Some(55.00), Some(110.00), Some(130.81), None, Some(61.74), None, None, // beats 7–8 (B2)
];

// Pattern indices
const PATTERN_KICK:  usize = 0;
const PATTERN_SNARE: usize = 1;
const PATTERN_HATS:  usize = 2;
const PATTERN_RIM:   usize = 3;
const PATTERN_STAB1: usize = 4;
const PATTERN_STAB2: usize = 5;
const PATTERN_STAB3: usize = 6;
const PATTERN_PAD:   usize = 7;
const PATTERN_MONO:  usize = 8;
const PATTERN_CLAVE:    usize = 9;
const PATTERN_BASSLINE: usize = 10;
const NUM_PATTERNS:     usize = 11;

/// One instrument group. Config fields are set once; runtime fields track playback state.
#[derive(Clone, Copy)]
struct Pattern {
    // Config (set once)
    start_weight:    f32,  // probability of turning on when off (0..1)
    continue_weight: f32,  // probability of staying on after minimum_repeats (0..1)
    minimum_repeats: u32,  // must stay on at least this many 8-bar segments
    can_solo:        bool, // allowed to be the only active pattern
    // Runtime
    active:          bool,
    segments_active: u32,  // how many 8-bar segments this has been continuously active
}

impl Pattern {
    fn new(start_weight: f32, continue_weight: f32, minimum_repeats: u32, can_solo: bool) -> Self {
        Self {
            start_weight,
            continue_weight,
            minimum_repeats,
            can_solo,
            active: false,
            segments_active: 0,
        }
    }
}

const FADE_DURATION: f32 = 1.0;

/// Human resting heartbeat — the base frequency from which all rhythms derive.
const BASE_FREQ: f32 = 1.2;

/// Polyrhythmic ratios (p/q of base frequency).
/// Chosen so LCM of denominators creates long resolution period.
const RIM_RATIO: (f32, f32) = (9.0, 5.0);    // 9/5 base → 2.160 Hz — drifts against kick

/// Polyrhythmic ambient techno engine.
///
/// All timing derived from a single base frequency (human heartbeat, 1.2 Hz).
/// Each element pulses at an exact rational multiple of the base, creating
/// polyrhythms that phase in and out of alignment and fully resolve at the LCM.
///
/// ```text
/// Element   Ratio    Frequency   Resolves with base every
/// ──────────────────────────────────────────────────────────
/// Kick      1/1      1.200 Hz    -
/// Hat       7/4      2.100 Hz    4 base cycles  (3.3s)
/// Hat       1/1      1.200 Hz    -               phase offset 0.5 — lands on the off-beat
/// Rim       9/5      2.160 Hz    5 base cycles   drifts against kick
/// Stab      3/5      0.720 Hz    5 base cycles  (4.2s)
/// Grain     1/7      0.171 Hz    7 base cycles  (5.8s)
///
/// Full alignment: LCM(4, 5, 7) = 140 base cycles ≈ 116.7s ≈ ~2 minutes
/// ```
pub struct AmbientTechno {
    kick: Kick,
    kick_pulse: PulseOscillator,
    kick_timer: Option<u32>,
    ghost_kick: Kick,
    beat_count: u32,
    ghost_count: u32,
    ghost_timer: Option<u32>,
    hat: HiHat,
    closed_hat: HiHat,
    closed_hat_lpf: ResonantLpf,
    hat_phaser: Phaser,
    beat_phase: u32,
    beat_duration: u32,
    swing: SwingLfo,
    /// Pre-computed trigger positions (in samples from kick) for the current beat, accounting for swing.
    open_hat_position: u32,
    closed_hat_positions: [u32; 3],
    roll_active: bool,
    snare: Snare808,
    snare_timer: Option<u32>,
    snare_reverb: DattorroReverb,
    ghost_snare: Snare808,
    ghost_snare1_timer: Option<u32>,
    ghost_snare2_timer: Option<u32>,
    ghost_snare_reverb: DattorroReverb,
    /// Reverse reverb: noise swell that rises over the beat before each snare hit.
    rev_rev_noise: Xorshift64,
    rev_rev_amp: f32,
    rev_rev_rise_rate: f32,
    rev_rev_active: bool,
    rev_rev_bp_low: f32,
    rev_rev_bp_band: f32,
    rev_rev_reverb: DattorroReverb,
    rim: HiHat,
    rim_pulse: PulseOscillator,
    rim_delay: BbdDelay,
    rim_reverb: DattorroReverb,
    stab: DubStab,
    stab_lpf: ResonantLpf,
    stab_delay: DubDelay,
    stab_phaser: Phaser,
    stab_timer: Option<u32>,
    stab_rng: Xorshift64,
    last_stab_idx: usize,
    stab2: DubStab,
    stab2_lpf: ResonantLpf,
    stab2_delay: DubDelay,
    stab2_timer: Option<u32>,
    stab3: DubStab,
    stab3_lpf: ResonantLpf,
    stab3_reverb: DattorroReverb,
    pad: SynthPad,
    pad_lpf: ResonantLpf,
    pad_chorus: BbdDelay,
    pad_reverb: DattorroReverb,
    pad_rng: Xorshift64,
    mono: MonoSynth,
    mono_reverb: DattorroReverb,
    mono_step: usize,
    mono_seq_freqs: [f32; 16],  // realized sequence (rerolled every 8 playthroughs)
    mono_seq_repeats: u8,       // how many full playthroughs of the current sequence
    mono_downbeat_timer: Option<u32>,
    mono_rng: Xorshift64,
    clave: ClaveVoice,
    clave_delay: DubDelay,
    clave_reverb: DattorroReverb,
    clave_timer: Option<u32>,
    bass: MonoSynth,
    bass_reverb: DattorroReverb,
    bassline_downbeat_timer: Option<u32>,
    patterns: [Pattern; NUM_PATTERNS],
    pattern_rng: Xorshift64,
    sample_rate: f32,
    limiter_gain: f32,
    fade_pos: u32,
    fade_state: State,
    fade_samples: u32,
}

impl AmbientTechno {
    pub fn new(sample_rate: u32, seed: u64) -> Self {
        let sr = sample_rate as f32;
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);

        let mut engine = Self {
            kick: Kick::new(sr),
            kick_pulse: PulseOscillator::new(BASE_FREQ, sr),
            kick_timer: None,
            ghost_kick: Kick::new_ghost(sr),
            beat_count: 0,
            ghost_count: 0,
            ghost_timer: None,
            hat: HiHat::new(sr, 0xDEADBEEF),
            closed_hat: HiHat::new_closed(sr, 0xFEEDFACE),
            closed_hat_lpf: ResonantLpf::new(3000.0, 6000.0, 0.2, 0.1, sr, &mut rng),
            hat_phaser: Phaser::new(0.7, 0.3, 0.25, sr),
            beat_phase: 0,
            beat_duration: (sr / BASE_FREQ) as u32,
            swing: SwingLfo::new(),
            open_hat_position: (sr / BASE_FREQ) as u32 / 2,
            closed_hat_positions: [0; 3],
            roll_active: false,
            snare: Snare808::new(sr, rng.r#gen::<u64>() | 1),
            snare_timer: None,
            snare_reverb: DattorroReverb::new(0.5, 0.3, 0.75, 0.04, sr, &mut rng),
            ghost_snare: Snare808::new(sr, rng.r#gen::<u64>() | 1),
            ghost_snare1_timer: None,
            ghost_snare2_timer: None,
            ghost_snare_reverb: DattorroReverb::new(0.4, 0.2, 0.7, 0.03, sr, &mut rng),
            rev_rev_noise: Xorshift64::new(rng.r#gen::<u64>() | 1),
            rev_rev_amp: 0.0,
            rev_rev_rise_rate: 0.0,
            rev_rev_active: false,
            rev_rev_bp_low: 0.0,
            rev_rev_bp_band: 0.0,
            rev_rev_reverb: DattorroReverb::new(0.92, 0.7, 0.85, 0.03, sr, &mut rng),
            rim: HiHat::new_rim(sr, 0xCAFEBABE),
            rim_pulse: PulseOscillator::new(BASE_FREQ * RIM_RATIO.0 / RIM_RATIO.1, sr),
            rim_delay: BbdDelay::new(40.0, 0.65, 0.75, 0.2, 1.0, sr, &mut rng),
            rim_reverb: DattorroReverb::new(0.92, 0.7, 0.95, 0.05, sr, &mut rng),
            stab: DubStab::new(sr),
            stab_lpf: ResonantLpf::new(300.0, 2500.0, 0.3, 0.5, sr, &mut rng), // 2s LFO
            stab_delay: DubDelay::new(416.0, 0.70, 0.75, sr),
            stab_phaser: Phaser::new(0.2, 0.6, 0.5, sr),
            stab_timer: None,
            stab_rng: Xorshift64::new(rng.r#gen::<u64>() | 1),
            last_stab_idx: 2, // D3 default until stab1 fires
            stab2: DubStab::new(sr),
            stab2_lpf: ResonantLpf::new(75.0, 625.0, 0.3, 0.5, sr, &mut rng),
            stab2_delay: DubDelay::new(416.0, 0.70, 0.75, sr),
            stab2_timer: None,
            stab3: DubStab::new_long(sr),
            stab3_lpf: ResonantLpf::new(40.0, 300.0, 0.3, 0.5, sr, &mut rng),
            stab3_reverb: DattorroReverb::new(0.95, 0.6, 0.9, 0.03, sr, &mut rng),
            pad: SynthPad::new(sr),
            pad_lpf: ResonantLpf::new(200.0, 1000.0, 0.05, 0.08, sr, &mut rng), // ~12s LFO
            pad_chorus: BbdDelay::new(20.0, 0.05, 0.6, 2.0, 3.0, sr, &mut rng),
            pad_reverb: DattorroReverb::new(0.93, 0.5, 0.85, 0.04, sr, &mut rng),
            pad_rng: Xorshift64::new(rng.r#gen::<u64>() | 1),
            mono: MonoSynth::new(sr),
            mono_reverb: DattorroReverb::new(0.82, 0.3, 0.7, 0.05, sr, &mut rng),
            mono_step: 0,
            mono_seq_freqs: {
                let mut f = [0.0f32; 16];
                for i in 0..16 { f[i] = A_MINOR_SCALE[MONO_SEQ_IDX[i]]; }
                f
            },
            mono_seq_repeats: 0,
            mono_downbeat_timer: None,
            mono_rng: Xorshift64::new(rng.r#gen::<u64>() | 1),
            clave: ClaveVoice::new(sr),
            clave_delay: DubDelay::new(416.0, 0.45, 0.3, sr),
            clave_reverb: DattorroReverb::new(0.93, 0.7, 0.85, 0.04, sr, &mut rng),
            clave_timer: None,
            bass: MonoSynth::new_bass(sr),
            bass_reverb: DattorroReverb::new(0.6, 0.2, 0.5, 0.05, sr, &mut rng),
            bassline_downbeat_timer: None,
            // Placeholder weights — caller will configure via set_pattern() before first use.
            patterns: [
                Pattern::new(0.2, 0.6, 4, false), // PATTERN_KICK
                Pattern::new(0.1, 0.4, 4, false), // PATTERN_SNARE
                Pattern::new(0.5, 0.5, 4, false), // PATTERN_HATS
                Pattern::new(0.8, 0.4, 8, true),  // PATTERN_RIM
                Pattern::new(0.3, 0.5, 4, true),  // PATTERN_STAB1
                Pattern::new(0.2, 0.5, 4, true),  // PATTERN_STAB2
                Pattern::new(0.7, 0.3, 16, true), // PATTERN_STAB3
                Pattern::new(0.2, 0.4, 8, true),  // PATTERN_PAD
                Pattern::new(0.1, 0.3, 8, true),  // PATTERN_MONO
                Pattern::new(0.1, 0.3, 2, false),   // PATTERN_CLAVE
                Pattern::new(0.1, 0.4, 4, true),    // PATTERN_BASSLINE
            ],
            pattern_rng: Xorshift64::new(rng.r#gen::<u64>() | 1),
            sample_rate: sr,
            limiter_gain: 1.0,
            fade_pos: 0,
            fade_state: State::FadingIn,
            fade_samples: (sr * FADE_DURATION) as u32,
        };
        engine.update_patterns(); // initial pattern selection
        engine
    }

    fn regenerate_mono_seq(&mut self) {
        // Pick 4 unique positions from 1..=15 via partial Fisher-Yates (step 0 is always the anchor)
        let mut positions: [usize; 15] = [1,2,3,4,5,6,7,8,9,10,11,12,13,14,15];
        for i in 0..4 {
            let j = i + (self.mono_rng.next() as usize % (15 - i));
            positions.swap(i, j);
        }
        for &pos in &positions[..4] {
            let base_idx = MONO_SEQ_IDX[pos];
            let shift = (self.mono_rng.next() % 7) as i32 - 3; // -3..=3
            let idx = (base_idx as i32 + shift).clamp(0, A_MINOR_SCALE.len() as i32 - 1) as usize;
            self.mono_seq_freqs[pos] = A_MINOR_SCALE[idx];
        }
    }

    fn next_f32(&mut self) -> f32 {
        self.pattern_rng.next() as f32 / u64::MAX as f32
    }

    fn update_patterns(&mut self) {
        // Step 1: tick active patterns; deactivate those past minimum_repeats that fail continue roll.
        for i in 0..NUM_PATTERNS {
            if self.patterns[i].active {
                self.patterns[i].segments_active += 1;
                if self.patterns[i].segments_active >= self.patterns[i].minimum_repeats {
                    if self.next_f32() > self.patterns[i].continue_weight {
                        self.patterns[i].active = false;
                        self.patterns[i].segments_active = 0;
                    }
                }
            }
        }

        // Step 2: try to activate inactive patterns by start_weight.
        for i in 0..NUM_PATTERNS {
            if !self.patterns[i].active && self.next_f32() < self.patterns[i].start_weight {
                self.patterns[i].active = true;
                self.patterns[i].segments_active = 0;
            }
        }

        // Step 3: if nothing is active, force the highest-start_weight pattern on.
        let active_count = self.patterns.iter().filter(|p| p.active).count();
        if active_count == 0 {
            let best = (0..NUM_PATTERNS)
                .max_by(|&a, &b| self.patterns[a].start_weight
                    .partial_cmp(&self.patterns[b].start_weight).unwrap())
                .unwrap();
            self.patterns[best].active = true;
            self.patterns[best].segments_active = 0;
        }

        // Step 4: if only one pattern is active and it can't solo, force-add the
        // highest-start_weight inactive pattern.
        let active_count = self.patterns.iter().filter(|p| p.active).count();
        if active_count == 1 {
            let solo_idx = (0..NUM_PATTERNS).find(|&i| self.patterns[i].active).unwrap();
            if !self.patterns[solo_idx].can_solo {
                if let Some(best) = (0..NUM_PATTERNS)
                    .filter(|&i| !self.patterns[i].active)
                    .max_by(|&a, &b| self.patterns[a].start_weight
                        .partial_cmp(&self.patterns[b].start_weight).unwrap())
                {
                    self.patterns[best].active = true;
                    self.patterns[best].segments_active = 0;
                }
            }
        }
    }

    fn advance_mono_step(&mut self) {
        self.mono_step += 1;
        if self.mono_step >= 16 {
            self.mono_step = 0;
            self.mono_seq_repeats += 1;
            if self.mono_seq_repeats >= 8 {
                self.mono_seq_repeats = 0;
                self.regenerate_mono_seq();
            }
        }
    }

    fn fade_ramp(&self) -> f32 {
        self.fade_pos as f32 / self.fade_samples as f32
    }

    pub fn set_param(&mut self, _name: &str, _value: f32) {}

    pub fn get_params(&self) -> Vec<(&str, f32, f32, f32)> {
        vec![]
    }

    fn next_sample(&mut self) -> (f32, f32) {
        let master_gain = match self.fade_state {
            State::FadingIn => {
                self.fade_pos += 1;
                if self.fade_pos >= self.fade_samples {
                    self.fade_state = State::Playing;
                }
                let t = self.fade_ramp();
                t * t
            }
            State::Playing => 1.0,
            State::FadingOut => {
                if self.fade_pos == 0 {
                    self.fade_state = State::Done;
                    0.0
                } else {
                    self.fade_pos = self.fade_pos.saturating_sub(1);
                    let t = self.fade_ramp();
                    t * t
                }
            }
            State::Done => 0.0,
        };

        if self.fade_state == State::Done {
            return (0.0, 0.0);
        }

        if self.kick_pulse.tick() {
            self.beat_phase = 0;
            // Advance the swing LFO and recompute all trigger positions (including the kick itself).
            self.swing.advance();
            let sw = self.swing.offset_samples(self.beat_duration);

            // Every 8 bars (32 beats): re-evaluate which patterns are active.
            if self.beat_count % 32 == 0 {
                self.update_patterns();
            }

            if self.patterns[PATTERN_KICK].active {
                self.kick_timer = Some(sw);
            }
            if self.patterns[PATTERN_MONO].active {
                self.mono_downbeat_timer = Some(sw);
            }
            if self.patterns[PATTERN_BASSLINE].active {
                self.bassline_downbeat_timer = Some(sw);
            }
            let sixteenth = self.beat_duration / 4;
            self.open_hat_position = self.beat_duration / 2 + sw;
            self.closed_hat_positions = [sixteenth + sw, sixteenth * 2, sixteenth * 3 + sw];
            self.roll_active = self.patterns[PATTERN_HATS].active && self.beat_count % 8 == 7;
            if self.beat_count % 8 == 0 && self.patterns[PATTERN_PAD].active {
                // Every 8 beats: pick a new high pentatonic note for the pad
                let idx = (self.pad_rng.next() as usize) % PAD_FREQS.len();
                self.pad.trigger_minor_chord(PAD_FREQS[idx]);
            }
            if self.beat_count % 32 == 0 && self.patterns[PATTERN_STAB3].active {
                // Every 32nd beat (0, 32, 64…): trigger stab3 on the downbeat
                let idx = (self.stab_rng.next() as usize) % STAB3_CHORDS.len();
                self.stab3.trigger_with_chord_and_cutoff(STAB3_CHORDS[idx], 600.0, &mut self.stab_rng);
            }
            if self.beat_count % 4 == 2 && self.patterns[PATTERN_SNARE].active {
                self.snare_timer = Some(sw);
                self.rev_rev_active = false;
            }
            if self.beat_count % 4 == 1 && self.patterns[PATTERN_SNARE].active {
                self.rev_rev_active = true;
                self.rev_rev_amp = 0.0;
                self.rev_rev_rise_rate = 1.0 / self.beat_duration as f32;
                self.rev_rev_bp_low = 0.0;
                self.rev_rev_bp_band = 0.0;
            }
            self.beat_count += 1;
            if self.beat_count % 2 == 0 && self.patterns[PATTERN_KICK].active {
                let eighth_note = (self.sample_rate / (BASE_FREQ * 2.0)) as u32;
                self.ghost_timer = Some(eighth_note + sw);
            }
            if self.beat_count % 8 == 7 && self.patterns[PATTERN_STAB1].active {
                let sixteenth = (self.sample_rate / (BASE_FREQ * 4.0)) as u32;
                let beat = (self.sample_rate / BASE_FREQ) as u32;
                self.stab_timer = Some(beat - sixteenth + sw);
            }
            if self.beat_count % 4 == 3 && self.patterns[PATTERN_STAB2].active {
                let sixteenth = (self.sample_rate / (BASE_FREQ * 4.0)) as u32;
                let beat = (self.sample_rate / BASE_FREQ) as u32;
                self.stab2_timer = Some(beat - sixteenth + sw);
            }
            // Clave: one hit per 4-bar loop, 1/16th before the 3rd beat of the 4th bar.
            // beat_count % 16 == 14 is the 2nd beat of bar 4 (after increment); fire timer at
            // beat - sixteenth + sw so it lands exactly one 16th note early.
            if self.beat_count % 16 == 14 && self.patterns[PATTERN_CLAVE].active {
                let sixteenth = (self.sample_rate / (BASE_FREQ * 4.0)) as u32;
                let beat = (self.sample_rate / BASE_FREQ) as u32;
                self.clave_timer = Some(beat - sixteenth + sw);
            }
        }

        tick_timer!(self.kick_timer, {
            self.kick.trigger();
        });

        tick_timer!(self.mono_downbeat_timer, {
            self.mono.trigger(self.mono_seq_freqs[self.mono_step], self.mono_step % 3 == 0);
            self.advance_mono_step();
        });

        tick_timer!(self.bassline_downbeat_timer, {
            let step = ((self.beat_count.wrapping_sub(1)) % 8 * 4) as usize;
            if let Some(freq) = BASSLINE_PATTERN[step] {
                self.bass.trigger(freq, false);
            }
        });

        tick_timer!(self.ghost_timer, {
            // Cycle: normal, minor third up, normal, minor third down
            const MINOR_THIRD: f32 = 1.18921; // 2^(3/12)
            const MINOR_SECOND: f32 = 1.05946; // 2^(1/12)
            let pitch_mul = match self.ghost_count % 4 {
                1 => MINOR_THIRD,
                3 => 1.0 / MINOR_SECOND,
                _ => 1.0,
            };
            self.ghost_kick.trigger_with_amp_and_pitch(0.125, pitch_mul);
            self.ghost_count += 1;
        });
        tick_timer!(self.stab_timer, {
            let idx = (self.stab_rng.next() as usize) % STAB1_CHORDS.len();
            self.stab.trigger_with_chord(STAB1_CHORDS[idx], &mut self.stab_rng);
            self.last_stab_idx = idx;
        });

        tick_timer!(self.stab2_timer, {
            self.stab2.trigger_with_chord(STAB2_CHORDS[self.last_stab_idx], &mut self.stab_rng);
        });

        tick_timer!(self.clave_timer, {
            self.clave.trigger_with_note("A6");
        });

        tick_timer!(self.snare_timer, {
            self.snare.trigger();
            let sixteenth = self.beat_duration / 4;
            self.ghost_snare1_timer = Some(self.beat_duration);
            self.ghost_snare2_timer = Some(self.beat_duration + sixteenth);
        });

        tick_timer!(self.ghost_snare1_timer, {
            self.ghost_snare.trigger_ghost();
        });

        tick_timer!(self.ghost_snare2_timer, {
            self.ghost_snare.trigger_ghost();
        });

        // Open hat is phase-locked to kick: fires at the swung halfway point of each beat (off-beat).
        if self.patterns[PATTERN_HATS].active && self.beat_phase == self.open_hat_position {
            self.hat.trigger();
        }

        let at_closed_hat_pos = self.beat_phase == self.closed_hat_positions[0]
            || self.beat_phase == self.closed_hat_positions[1]
            || self.beat_phase == self.closed_hat_positions[2];

        // Closed hats are phase-locked to the kick: fire at swung 16th note positions.
        if self.patterns[PATTERN_HATS].active && at_closed_hat_pos {
            self.closed_hat.trigger();
        }

        // Monosynth off-beat 16th notes share the same swung positions as the closed hats.
        if self.patterns[PATTERN_MONO].active && at_closed_hat_pos {
            self.mono.trigger(self.mono_seq_freqs[self.mono_step], self.mono_step % 3 == 0);
            self.advance_mono_step();
        }

        // Bassline off-beat 16ths: steps 1, 2, 3 within each beat.
        if self.patterns[PATTERN_BASSLINE].active {
            let base = ((self.beat_count.wrapping_sub(1)) % 8 * 4) as usize;
            let step_offset =
                if self.beat_phase == self.closed_hat_positions[0] { Some(1) }
                else if self.beat_phase == self.closed_hat_positions[1] { Some(2) }
                else if self.beat_phase == self.closed_hat_positions[2] { Some(3) }
                else { None };
            if let Some(off) = step_offset {
                if let Some(freq) = BASSLINE_PATTERN[base + off] {
                    self.bass.trigger(freq, false);
                }
            }
        }

        // On the last beat of every 2-measure cycle, fire a roll of 8 evenly spaced closed hats.
        // Odd-indexed hits (off-beat 32nd notes) are nudged by the current swing offset.
        if self.roll_active {
            let spacing = self.beat_duration / 8; // 32nd-note spacing
            let sw = self.swing.offset_samples(self.beat_duration);
            for i in 0..8u32 {
                let pos = spacing * i + if i % 2 == 1 { sw } else { 0 };
                if self.beat_phase == pos {
                    self.closed_hat.trigger();
                    break;
                }
            }
        }

        self.beat_phase += 1;
        if self.patterns[PATTERN_RIM].active && self.rim_pulse.tick() {
            self.rim.trigger();
        }

        let kick = self.kick.next_sample() + self.ghost_kick.next_sample();
        let snare_dry = self.snare.next_sample();
        let (snare_l, snare_r) = self.snare_reverb.process(snare_dry);
        let ghost_snare_dry = self.ghost_snare.next_sample();
        let (ghost_snare_l, ghost_snare_r) = self.ghost_snare_reverb.process(ghost_snare_dry);
        // Reverse reverb: rising noise swell fed into a long reverb, input cut at the hit.
        let rev_rev_input = if self.rev_rev_active {
            let white = self.rev_rev_noise.white();
            let f = (std::f32::consts::PI * 800.0 / self.sample_rate).sin() * 2.0;
            let high = white - self.rev_rev_bp_low - 0.4 * self.rev_rev_bp_band;
            self.rev_rev_bp_band += f * high;
            self.rev_rev_bp_low += f * self.rev_rev_bp_band;
            // t² curve: starts near-silent, accelerates toward the hit
            let out = self.rev_rev_bp_band * (self.rev_rev_amp * self.rev_rev_amp * 0.25);
            self.rev_rev_amp = (self.rev_rev_amp + self.rev_rev_rise_rate).min(1.0);
            out
        } else {
            0.0
        };
        let (rev_rev_l, rev_rev_r) = self.rev_rev_reverb.process(rev_rev_input);
        let hat = self.hat.next_sample();
        let closed_hat = self.closed_hat_lpf.process(self.closed_hat.next_sample());
        let (hat_l, hat_r) = self.hat_phaser.process(hat + closed_hat);
        let rim_dry = self.rim.next_sample();
        let rim_echoed = self.rim_delay.process(rim_dry);
        let (rim_l, rim_r) = self.rim_reverb.process(rim_echoed);
        let stab_filtered = self.stab_lpf.process(self.stab.next_sample());
        let (stab_dl, stab_dr) = self.stab_delay.process(stab_filtered);
        let (stab_l, stab_r) = self.stab_phaser.process((stab_dl + stab_dr) * 0.5);
        let stab2_filtered = self.stab2_lpf.process(self.stab2.next_sample());
        let (stab2_l, stab2_r) = self.stab2_delay.process(stab2_filtered);
        let stab3_filtered = self.stab3_lpf.process(self.stab3.next_sample());
        let (stab3_l, stab3_r) = self.stab3_reverb.process(stab3_filtered);
        let pad_filtered = self.pad_lpf.process(self.pad.next_sample());
        let pad_chorused = self.pad_chorus.process(pad_filtered);
        let (pad_l, pad_r) = self.pad_reverb.process(pad_chorused);
        let mono_dry = self.mono.next_sample();
        let (mono_rev_l, mono_rev_r) = self.mono_reverb.process(mono_dry);
        let mono_l = mono_dry + mono_rev_l * 0.25;
        let mono_r = mono_dry + mono_rev_r * 0.25;
        let clave_dry = self.clave.next_sample();
        let (clave_dl, clave_dr) = self.clave_delay.process(clave_dry);
        let (clave_l, clave_r) = self.clave_reverb.process((clave_dl + clave_dr) * 0.5);
        let bass_dry = self.bass.next_sample();
        let (bass_rev_l, bass_rev_r) = self.bass_reverb.process(bass_dry);
        let bass_l = bass_dry + bass_rev_l * 0.08;
        let bass_r = bass_dry + bass_rev_r * 0.08;

        // Kick centre, snare centre, hat panned slightly left, closed hat centre, rim slightly right, stabs, pad and mono centre
        let mut left = kick + snare_l * 0.425 + ghost_snare_l * 0.3 + rev_rev_l * 0.25 + hat_l * 0.7 + rim_l * 0.8 + stab_l * 0.6 + stab2_l * 0.6 + stab3_l * 0.7 + pad_l + mono_l * 0.09375 + clave_l * 0.5 + bass_l * 0.09;
        let mut right = kick + snare_r * 0.425 + ghost_snare_r * 0.3 + rev_rev_r * 0.25 + hat_r * 0.7 + rim_r * 0.8 + stab_r * 0.6 + stab2_r * 0.6 + stab3_r * 0.7 + pad_r + mono_r * 0.09375 + clave_r * 0.5 + bass_r * 0.09;

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

impl Track for AmbientTechno {
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let len = left.len().min(right.len());
        for i in 0..len {
            let (l, r) = self.next_sample();
            left[i] = l;
            right[i] = r;
        }
    }

    fn start_fade_out(&mut self) {
        if self.fade_state == State::FadingIn || self.fade_state == State::Playing {
            self.fade_state = State::FadingOut;
        }
    }

    fn state(&self) -> State {
        self.fade_state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instruments::GranularEngine;

    #[test]
    fn pulse_oscillator_fires_at_correct_rate() {
        let sample_rate = 44100.0;
        let freq = 2.0; // 2 Hz = trigger every 0.5 seconds = every 22050 samples
        let mut pulse = PulseOscillator::new(freq, sample_rate);

        let mut trigger_count = 0;
        let mut first_trigger_at = None;
        let mut second_trigger_at = None;

        // Run for 1 second (should get ~2 triggers)
        for i in 0..44100 {
            if pulse.tick() {
                trigger_count += 1;
                if first_trigger_at.is_none() {
                    first_trigger_at = Some(i);
                } else if second_trigger_at.is_none() {
                    second_trigger_at = Some(i);
                }
            }
        }

        assert_eq!(trigger_count, 2, "2 Hz should trigger twice per second");

        // Check interval between triggers is ~22050 samples
        let interval = second_trigger_at.unwrap() - first_trigger_at.unwrap();
        assert!(
            (interval as i32 - 22050).unsigned_abs() < 5,
            "interval between triggers should be ~22050 samples, got {}",
            interval
        );
    }

    #[test]
    fn kick_produces_signal_after_trigger() {
        let mut kick = Kick::new(44100.0);
        assert_eq!(kick.next_sample(), 0.0, "kick should be silent before trigger");

        kick.trigger();
        let mut has_signal = false;
        for _ in 0..4410 {
            if kick.next_sample().abs() > 0.01 {
                has_signal = true;
                break;
            }
        }
        assert!(has_signal, "kick should produce signal after trigger");
    }

    #[test]
    fn kick_decays_to_silence() {
        let mut kick = Kick::new(44100.0);
        kick.trigger();

        // Run for 2 seconds — should be silent by then
        for _ in 0..88200 {
            kick.next_sample();
        }
        assert!(!kick.active, "kick should be inactive after decay");
        assert_eq!(kick.next_sample(), 0.0);
    }

    #[test]
    fn ambient_techno_renders_kicks() {
        let mut engine = AmbientTechno::new(44100, 42);

        // Render 1 second — at ~2.08 Hz we should get at least 1 kick
        let mut left = vec![0.0f32; 44100];
        let mut right = vec![0.0f32; 44100];
        engine.render(&mut left, &mut right);

        let peak = left.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.01, "should have audible kick signal, got peak {}", peak);
    }

    #[test]
    fn ambient_techno_output_within_bounds() {
        let mut engine = AmbientTechno::new(44100, 42);
        let mut left = vec![0.0f32; 4096];
        let mut right = vec![0.0f32; 4096];

        for _ in 0..20 {
            engine.render(&mut left, &mut right);
            for &s in left.iter().chain(right.iter()) {
                assert!(s.abs() <= 1.0, "sample {} exceeds [-1, 1] range", s);
            }
        }
    }

    #[test]
    fn ambient_techno_deterministic_with_same_seed() {
        let mut e1 = AmbientTechno::new(44100, 99);
        let mut e2 = AmbientTechno::new(44100, 99);

        let mut l1 = vec![0.0f32; 2048];
        let mut r1 = vec![0.0f32; 2048];
        let mut l2 = vec![0.0f32; 2048];
        let mut r2 = vec![0.0f32; 2048];

        e1.render(&mut l1, &mut r1);
        e2.render(&mut l2, &mut r2);

        assert_eq!(l1, l2);
        assert_eq!(r1, r2);
    }

    #[test]
    fn hihat_produces_signal_and_decays() {
        let mut hat = HiHat::new(44100.0, 12345);
        assert_eq!(hat.next_sample(), 0.0, "hat silent before trigger");

        hat.trigger();
        let mut peak = 0.0f32;
        for _ in 0..4410 {
            peak = peak.max(hat.next_sample().abs());
        }
        assert!(peak > 0.01, "hat should produce signal after trigger");

        // After ~200ms should be essentially silent
        for _ in 0..8820 {
            hat.next_sample();
        }
        assert!(hat.next_sample().abs() < 0.001, "hat should decay to silence");
    }

    #[test]
    fn pulse_param_changes_kick_rate() {
        // Test pulse oscillator directly — more reliable than detecting kicks in mixed audio
        let mut fast = PulseOscillator::new(2.08, 44100.0);
        let mut slow = PulseOscillator::new(1.3, 44100.0);

        let mut fast_count = 0u32;
        let mut slow_count = 0u32;

        // Count triggers over 3 seconds
        for _ in 0..(44100 * 3) {
            if fast.tick() { fast_count += 1; }
            if slow.tick() { slow_count += 1; }
        }

        assert!(fast_count > slow_count,
            "2.08 Hz ({} triggers) should fire more than 1.3 Hz ({} triggers)",
            fast_count, slow_count);
    }

    #[test]
    fn dub_stab_produces_signal_and_decays() {
        let mut stab = DubStab::new(44100.0);
        assert_eq!(stab.next_sample(), 0.0, "stab silent before trigger");

        let mut noise = Xorshift64::new(42);
        stab.trigger(130.0, &mut noise);

        let mut peak = 0.0f32;
        for _ in 0..4410 {
            peak = peak.max(stab.next_sample().abs());
        }
        assert!(peak > 0.01, "stab should produce signal after trigger");

        // After 3 seconds should be silent
        for _ in 0..(44100 * 3) {
            stab.next_sample();
        }
        assert!(!stab.active, "stab should decay to inactive");
    }

    #[test]
    fn granular_engine_produces_signal() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let mut grain = GranularEngine::new(44100.0, 42, &mut rng);
        grain.set_level(0.5);

        // Spawn a grain and check it produces signal
        grain.spawn_grain();
        let mut has_signal = false;
        for _ in 0..44100 {
            let (l, r) = grain.next_sample();
            if l.abs() > 0.001 || r.abs() > 0.001 {
                has_signal = true;
                break;
            }
        }
        assert!(has_signal, "granular engine should produce signal after spawn");
    }

    #[test]
    fn full_engine_with_all_elements_stays_bounded() {
        let mut engine = AmbientTechno::new(44100, 42);
        // Crank everything up
        engine.set_param("haze", 1.0);
        engine.set_param("density", 1.0);
        engine.set_param("grain", 1.0);
        engine.set_param("drift", 1.0);

        let mut left = vec![0.0f32; 4096];
        let mut right = vec![0.0f32; 4096];

        for _ in 0..30 {
            engine.render(&mut left, &mut right);
            for &s in left.iter().chain(right.iter()) {
                assert!(s.abs() <= 1.0, "sample {} exceeds [-1, 1] range", s);
            }
        }
    }
}
