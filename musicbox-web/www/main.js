let audioCtx = null;
let workletNode = null;
let analyser = null;
let wasmBytes = null;
let animFrameId = null;
let dotAnimFrameId = null;

const btn = document.getElementById("toggle");
const status = document.getElementById("status");

// ── Dot animation along circle arc ──
const CX = 120, CY = 120, CR = 100;
const TARGET_ANGLE = Math.PI / 2; // bottom of circle (SVG y-down)
const DOT_MORPH_MS = 600; // slower than waveform morph for organic feel

// Starting angles for the 4 outer dots (atan2 from center, SVG y-down)
const DOT_START_ANGLES = [
    Math.atan2(150.9 - CY, 24.9 - CX),   // ~2.827 rad (lower-left)
    Math.atan2(39.1 - CY, 61.2 - CX),     // ~-2.200 rad (upper-left)
    Math.atan2(39.1 - CY, 178.8 - CX),    // ~-0.942 rad (upper-right)
    Math.atan2(150.9 - CY, 215.1 - CX),   // ~0.314 rad (lower-right)
];

// Compute shortest arc direction and distance to target for each dot
const DOT_ARCS = DOT_START_ANGLES.map(start => {
    let diff = TARGET_ANGLE - start;
    // Normalize to [-π, π]
    while (diff > Math.PI) diff -= 2 * Math.PI;
    while (diff < -Math.PI) diff += 2 * Math.PI;
    return diff; // signed arc distance
});

function easeInOutCubic(t) {
    return t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2;
}

function setDotPositions(blend) {
    // blend: 0 = home positions, 1 = all at bottom
    const dots = document.querySelectorAll(".sigil-dot");
    dots.forEach((dot, i) => {
        const startAngle = DOT_START_ANGLES[i];
        const arcDist = DOT_ARCS[i];
        // Stagger: dots with longer arcs start moving first
        const maxArc = Math.max(...DOT_ARCS.map(Math.abs));
        const stagger = 1.0 - (Math.abs(arcDist) / maxArc) * 0.3;
        const t = easeInOutCubic(Math.min(blend / stagger, 1.0));

        const angle = startAngle + arcDist * t;
        const x = CX + CR * Math.cos(angle);
        const y = CY + CR * Math.sin(angle);
        dot.setAttribute("cx", x.toFixed(1));
        dot.setAttribute("cy", y.toFixed(1));
    });
}

function animateDotsToBottom() {
    const startTime = performance.now();
    function step() {
        const blend = Math.min((performance.now() - startTime) / DOT_MORPH_MS, 1.0);
        setDotPositions(blend);
        if (blend < 1.0) {
            dotAnimFrameId = requestAnimationFrame(step);
        }
    }
    step();
}

function animateDotsToHome() {
    const startTime = performance.now();
    // Capture current blend (should be 1.0 if fully coalesced)
    function step() {
        const elapsed = performance.now() - startTime;
        const blend = 1.0 - Math.min(elapsed / DOT_MORPH_MS, 1.0);
        setDotPositions(blend);
        if (blend > 0.0) {
            dotAnimFrameId = requestAnimationFrame(step);
        }
    }
    step();
}

// Static path data for morph-back
const SINE_PATH = "M 40 120 C 40 120, 50 74, 60 74 C 70 74, 80 120, 80 120 C 80 120, 90 166, 100 166 C 110 166, 120 120, 120 120 C 120 120, 130 74, 140 74 C 150 74, 160 120, 160 120 C 160 120, 170 166, 180 166 C 190 166, 200 120, 200 120";
const NOISE_PATH = "M 40 120 L 48 89 L 56 145 L 62 78 L 70 153 L 78 95 L 88 162 L 96 83 L 104 148 L 112 74 L 118 139 L 126 92 L 134 166 L 140 80 L 150 151 L 158 90 L 164 133 L 172 74 L 182 146 L 190 98 L 200 120";

function setStatus(text) {
    status.textContent = text;
}

async function loadWasm() {
    if (wasmBytes) return wasmBytes;
    setStatus("Loading WASM...");
    const response = await fetch("pkg/musicbox_web_bg.wasm");
    wasmBytes = await response.arrayBuffer();
    return wasmBytes;
}

const NUM_POINTS = 21;
const MORPH_MS = 300;
const X_MIN = 40, X_MAX = 200, Y_CENTER = 120, Y_RANGE = 80;

// Pre-compute the Y values for the static paths at our sample points
function staticYValues(pathId) {
    // Sample the static waveform at evenly spaced x positions
    // Sine: smooth curve, Noise: jagged
    if (pathId === "front") {
        // Sine wave: y = 120 - 46*sin(x * 2.5pi / 160)
        return Array.from({ length: NUM_POINTS }, (_, i) => {
            const t = i / (NUM_POINTS - 1);
            return Y_CENTER - 46 * Math.sin(t * 2.5 * Math.PI);
        });
    } else {
        // Noise: sample the known static path y-values
        const noiseY = [120, 89, 145, 78, 153, 95, 162, 83, 148, 74, 139, 92, 166, 80, 151, 90, 133, 74, 146, 98, 120];
        return noiseY;
    }
}

const staticFrontY = staticYValues("front");
const staticBackY = staticYValues("back");

function buildPath(yValues) {
    let d = "";
    for (let i = 0; i < yValues.length; i++) {
        const x = X_MIN + (i / (yValues.length - 1)) * (X_MAX - X_MIN);
        d += (i === 0 ? "M " : " L ") + x.toFixed(1) + " " + yValues[i].toFixed(1);
    }
    return d;
}

function sampleLiveY(dataArray, bufferLength) {
    const step = bufferLength / NUM_POINTS;
    return Array.from({ length: NUM_POINTS }, (_, i) => {
        const idx = Math.min(Math.floor(i * step), bufferLength - 1);
        const sample = (dataArray[idx] / 128.0) - 1.0;
        return Y_CENTER - sample * Y_RANGE;
    });
}

function lerpArrays(a, b, t) {
    return a.map((v, i) => v + (b[i] - v) * t);
}

let morphStartTime = 0;
let morphDirection = 1; // 1 = morphing to live, -1 = morphing to static

function startVisualization() {
    const dataArray = new Uint8Array(analyser.frequencyBinCount);
    morphStartTime = performance.now();
    morphDirection = 1;

    // Animate dots to bottom
    if (dotAnimFrameId) cancelAnimationFrame(dotAnimFrameId);
    animateDotsToBottom();

    function draw() {
        animFrameId = requestAnimationFrame(draw);
        analyser.getByteTimeDomainData(dataArray);

        const liveY = sampleLiveY(dataArray, dataArray.length);

        // Blend factor: 0 = static, 1 = fully live
        const elapsed = performance.now() - morphStartTime;
        const blend = Math.min(elapsed / MORPH_MS, 1.0);

        const frontY = lerpArrays(staticFrontY, liveY, blend);
        const backY = lerpArrays(staticBackY, liveY, blend);

        const frontPath = document.getElementById("sigil-wave-front");
        const backPath = document.getElementById("sigil-wave-back");
        if (frontPath) frontPath.setAttribute("d", buildPath(frontY));
        if (backPath) backPath.setAttribute("d", buildPath(backY));
    }

    draw();
}

function stopVisualization() {
    if (!animFrameId) return;

    // Capture the current live Y values for morph-out
    const dataArray = new Uint8Array(analyser.frequencyBinCount);
    analyser.getByteTimeDomainData(dataArray);
    const lastLiveY = sampleLiveY(dataArray, dataArray.length);

    cancelAnimationFrame(animFrameId);
    animFrameId = null;

    // Animate dots back to home positions
    if (dotAnimFrameId) cancelAnimationFrame(dotAnimFrameId);
    animateDotsToHome();

    const startTime = performance.now();

    function morphBack() {
        const elapsed = performance.now() - startTime;
        const blend = Math.min(elapsed / MORPH_MS, 1.0);

        const frontY = lerpArrays(lastLiveY, staticFrontY, blend);
        const backY = lerpArrays(lastLiveY, staticBackY, blend);

        const frontPath = document.getElementById("sigil-wave-front");
        const backPath = document.getElementById("sigil-wave-back");
        if (frontPath) frontPath.setAttribute("d", buildPath(frontY));
        if (backPath) backPath.setAttribute("d", buildPath(backY));

        if (blend < 1.0) {
            requestAnimationFrame(morphBack);
        }
    }

    morphBack();
}

async function start() {
    btn.disabled = true;
    setStatus("Starting...");

    try {
        const bytes = await loadWasm();

        audioCtx = new (window.AudioContext || window.webkitAudioContext)({ sampleRate: 44100 });
        if (audioCtx.state === "suspended") {
            await audioCtx.resume();
        }
        if (!audioCtx.audioWorklet) {
            throw new Error("AudioWorklet not supported — try a modern browser over HTTPS");
        }
        await audioCtx.audioWorklet.addModule("worklet.js");

        workletNode = new AudioWorkletNode(audioCtx, "musicbox-processor", {
            outputChannelCount: [2],
        });

        // Insert analyser between worklet and destination
        analyser = audioCtx.createAnalyser();
        analyser.fftSize = 256;
        workletNode.connect(analyser);
        analyser.connect(audioCtx.destination);

        // Wait for worklet to signal ready
        const ready = new Promise((resolve, reject) => {
            const timeout = setTimeout(() => reject(new Error("Worklet init timed out")), 10000);
            workletNode.port.onmessage = (event) => {
                if (event.data.type === "ready") {
                    clearTimeout(timeout);
                    resolve();
                } else if (event.data.type === "error") {
                    clearTimeout(timeout);
                    reject(new Error(event.data.message));
                } else if (event.data.type === "done") {
                    setStatus("Stopped.");
                    btn.textContent = "Play";
                    btn.disabled = false;
                    cleanup();
                }
            };
        });

        // Generate a random seed as two 32-bit halves (avoids BigInt compatibility issues)
        const seedHigh = Math.floor(Math.random() * 0xFFFFFFFF);
        const seedLow = Math.floor(Math.random() * 0xFFFFFFFF);

        // Send raw WASM bytes to worklet (Safari can't postMessage compiled modules)
        workletNode.port.postMessage({
            type: "init",
            data: {
                wasmBytes: bytes,
                sampleRate: audioCtx.sampleRate,
                seedHigh,
                seedLow,
            },
        });

        await ready;

        // Start the sigil visualization
        startVisualization();

        // Re-attach message handler for done events
        workletNode.port.onmessage = (event) => {
            if (event.data.type === "done") {
                setStatus("Stopped.");
                btn.textContent = "Play";
                btn.disabled = false;
                cleanup();
            }
        };

        setStatus("Playing...");
        btn.textContent = "Stop";
        btn.disabled = false;
    } catch (err) {
        setStatus("Error: " + err.message);
        btn.disabled = false;
        console.error(err);
    }
}

function stop() {
    if (workletNode) {
        setStatus("Fading out...");
        btn.disabled = true;
        workletNode.port.postMessage({ type: "fade-out" });
    }
}

function cleanup() {
    stopVisualization();
    if (workletNode) {
        workletNode.disconnect();
        workletNode = null;
    }
    if (analyser) {
        analyser.disconnect();
        analyser = null;
    }
    if (audioCtx) {
        audioCtx.close();
        audioCtx = null;
    }
}

btn.addEventListener("click", () => {
    if (audioCtx && audioCtx.state !== "closed") {
        stop();
    } else {
        start();
    }
});
