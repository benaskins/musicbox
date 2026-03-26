use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use musicbox_core::{MusicBox, State, Track, parse_duration};
use musicbox_core::tracks::ambient_techno::AmbientTechno;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

const STATE_FADING_IN: u8 = 0;
const STATE_PLAYING: u8 = 1;
const STATE_FADING_OUT: u8 = 2;
const STATE_DONE: u8 = 3;

fn random_seed() -> u64 {
    use rand::Rng;
    rand::thread_rng().r#gen()
}

fn run_live() {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("no audio output device found");

    println!("Using audio device: {}", device.name().unwrap_or_default());

    let config = device.default_output_config().unwrap();
    println!("Audio format: {:?}", config);

    let channels = config.channels() as usize;
    let sample_rate = config.sample_rate().0;
    let mut engine = MusicBox::new(sample_rate, random_seed());

    // Signal handler for graceful fade-out
    let master_state = Arc::new(AtomicU8::new(STATE_FADING_IN));
    let master_state_signal = Arc::clone(&master_state);

    ctrlc::set_handler(move || {
        let prev = master_state_signal.load(Ordering::Relaxed);
        if prev == STATE_FADING_IN || prev == STATE_PLAYING {
            println!("\nFading out...");
            master_state_signal.store(STATE_FADING_OUT, Ordering::Relaxed);
        }
    })
    .expect("failed to set signal handler");

    // Pre-allocate split buffers for the largest expected callback size
    let max_frames = 8192;
    let mut left_buf = vec![0.0f32; max_frames];
    let mut right_buf = vec![0.0f32; max_frames];

    let master_state_cb = Arc::clone(&master_state);
    let callback = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
        // Check if signal handler requested fade-out
        let sig_state = master_state_cb.load(Ordering::Relaxed);
        if sig_state == STATE_FADING_OUT
            && engine.state() != State::FadingOut
            && engine.state() != State::Done
        {
            engine.start_fade_out();
        }

        // Track engine state back to atomic for the main loop
        match engine.state() {
            State::Playing => master_state_cb.store(STATE_PLAYING, Ordering::Relaxed),
            State::Done => master_state_cb.store(STATE_DONE, Ordering::Relaxed),
            _ => {}
        }

        let frames = data.len() / channels;
        let frames = frames.min(max_frames);

        engine.render(&mut left_buf[..frames], &mut right_buf[..frames]);

        // Interleave into cpal's buffer
        for (i, frame) in data.chunks_mut(channels).enumerate().take(frames) {
            if channels >= 2 {
                frame[0] = left_buf[i];
                frame[1] = right_buf[i];
                for s in frame[2..].iter_mut() {
                    *s = (left_buf[i] + right_buf[i]) * 0.5;
                }
            } else {
                frame[0] = (left_buf[i] + right_buf[i]) * 0.5;
            }
        }
    };

    let err_callback = |err: cpal::StreamError| {
        eprintln!("Audio stream error: {}", err);
    };

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device
            .build_output_stream(&config.into(), callback, err_callback, None)
            .unwrap(),
        _ => panic!("unsupported sample format — expected F32"),
    };

    stream.play().unwrap();
    println!("musicbox is playing... press Ctrl+C to stop");

    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if master_state.load(Ordering::Relaxed) == STATE_DONE {
            std::thread::sleep(std::time::Duration::from_millis(200));
            println!("Goodbye.");
            break;
        }
    }
}

fn run_render(duration_str: &str, output_path: &str) {
    let duration_secs = parse_duration(duration_str)
        .unwrap_or_else(|| {
            eprintln!("Invalid duration: '{}'. Examples: 10m, 1h30m, 90s", duration_str);
            std::process::exit(1);
        });

    let sample_rate: u32 = 44100;
    let total_samples = (sample_rate as f32 * duration_secs) as u64;
    let fade_samples = (sample_rate as f32 * 3.0) as u64; // 3s fade
    let body_samples = total_samples - fade_samples;

    let mut engine = MusicBox::new(sample_rate, random_seed());

    let spec = hound::WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };

    let mut writer = hound::WavWriter::create(output_path, spec)
        .unwrap_or_else(|e| {
            eprintln!("Failed to create {}: {}", output_path, e);
            std::process::exit(1);
        });

    println!(
        "Rendering {:.0}s ({}) to {}...",
        duration_secs, duration_str, output_path
    );

    let progress_interval = sample_rate as u64 * 10;

    // Render in blocks for efficiency
    let block_size = 1024usize;
    let mut left_buf = vec![0.0f32; block_size];
    let mut right_buf = vec![0.0f32; block_size];
    let mut sample_pos: u64 = 0;

    while sample_pos < total_samples {
        let remaining = (total_samples - sample_pos) as usize;
        let this_block = remaining.min(block_size);

        // Check if fade-out should start within this block
        if sample_pos <= body_samples && sample_pos + this_block as u64 > body_samples {
            // Split: render up to body_samples, then trigger fade, then render rest
            let pre = (body_samples - sample_pos) as usize;
            if pre > 0 {
                engine.render(&mut left_buf[..pre], &mut right_buf[..pre]);
                for i in 0..pre {
                    writer.write_sample(left_buf[i]).unwrap();
                    writer.write_sample(right_buf[i]).unwrap();
                }
            }
            engine.start_fade_out();
            let post = this_block - pre;
            if post > 0 {
                engine.render(&mut left_buf[..post], &mut right_buf[..post]);
                for i in 0..post {
                    writer.write_sample(left_buf[i]).unwrap();
                    writer.write_sample(right_buf[i]).unwrap();
                }
            }
        } else {
            engine.render(&mut left_buf[..this_block], &mut right_buf[..this_block]);
            for i in 0..this_block {
                writer.write_sample(left_buf[i]).unwrap();
                writer.write_sample(right_buf[i]).unwrap();
            }
        }

        sample_pos += this_block as u64;

        if sample_pos % progress_interval < block_size as u64 && sample_pos > 0 {
            let pct = (sample_pos as f32 / total_samples as f32) * 100.0;
            print!("\r  {:.0}% ({:.0}s / {:.0}s)", pct, sample_pos as f32 / sample_rate as f32, duration_secs);
        }
    }

    writer.finalize().unwrap();
    println!("\r  100% — wrote {}", output_path);
}

fn run_live_techno() {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("no audio output device found");

    println!("Using audio device: {}", device.name().unwrap_or_default());

    let config = device.default_output_config().unwrap();
    println!("Audio format: {:?}", config);

    let channels = config.channels() as usize;
    let sample_rate = config.sample_rate().0;
    let mut engine = AmbientTechno::new(sample_rate, random_seed());

    let master_state = Arc::new(AtomicU8::new(STATE_FADING_IN));
    let master_state_signal = Arc::clone(&master_state);

    ctrlc::set_handler(move || {
        let prev = master_state_signal.load(Ordering::Relaxed);
        if prev == STATE_FADING_IN || prev == STATE_PLAYING {
            println!("\nFading out...");
            master_state_signal.store(STATE_FADING_OUT, Ordering::Relaxed);
        }
    })
    .expect("failed to set signal handler");

    let max_frames = 8192;
    let mut left_buf = vec![0.0f32; max_frames];
    let mut right_buf = vec![0.0f32; max_frames];

    let master_state_cb = Arc::clone(&master_state);
    let callback = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
        let sig_state = master_state_cb.load(Ordering::Relaxed);
        if sig_state == STATE_FADING_OUT
            && engine.state() != State::FadingOut
            && engine.state() != State::Done
        {
            engine.start_fade_out();
        }

        match engine.state() {
            State::Playing => master_state_cb.store(STATE_PLAYING, Ordering::Relaxed),
            State::Done => master_state_cb.store(STATE_DONE, Ordering::Relaxed),
            _ => {}
        }

        let frames = (data.len() / channels).min(max_frames);
        engine.render(&mut left_buf[..frames], &mut right_buf[..frames]);

        for (i, frame) in data.chunks_mut(channels).enumerate().take(frames) {
            if channels >= 2 {
                frame[0] = left_buf[i];
                frame[1] = right_buf[i];
                for s in frame[2..].iter_mut() {
                    *s = (left_buf[i] + right_buf[i]) * 0.5;
                }
            } else {
                frame[0] = (left_buf[i] + right_buf[i]) * 0.5;
            }
        }
    };

    let err_callback = |err: cpal::StreamError| {
        eprintln!("Audio stream error: {}", err);
    };

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device
            .build_output_stream(&config.into(), callback, err_callback, None)
            .unwrap(),
        _ => panic!("unsupported sample format — expected F32"),
    };

    stream.play().unwrap();
    println!("musicbox [techno] is playing... press Ctrl+C to stop");

    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if master_state.load(Ordering::Relaxed) == STATE_DONE {
            std::thread::sleep(std::time::Duration::from_millis(200));
            println!("Goodbye.");
            break;
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Parse optional style as first positional arg (defaults to "drone")
    let style = args.get(1).map(|s| s.as_str()).unwrap_or("drone");

    if args.len() >= 3 && args[1] == "--render" {
        let duration = &args[2];
        let output = if args.len() >= 4 {
            args[3].as_str()
        } else {
            "musicbox.wav"
        };
        run_render(duration, output);
    } else if args.len() >= 2 && (args[1] == "--help" || args[1] == "-h") {
        println!("musicbox v0.1.0 — generative ambient audio");
        println!();
        println!("Usage:");
        println!("  musicbox [style]              Play live (Ctrl+C to fade out and stop)");
        println!("  musicbox --render <duration> [output.wav]");
        println!();
        println!("Styles: drone (default), techno");
        println!("Duration examples: 10m, 1h30m, 90s, 5m30s");
        println!();
        println!("Each run is unique — the generative engine is seeded randomly.");
    } else {
        match style {
            "techno" => run_live_techno(),
            "drone" => run_live(),
            other => {
                eprintln!("Unknown style '{}'. Available styles: drone, techno", other);
                std::process::exit(1);
            }
        }
    }
}
