// ── Groovebox: Stones in a Pond ──
// Adjacent circles, each a polyrhythmic ratio.
// Drop voice sigils inside a circle to activate them.
// Circles ripple gently at their ratio's pulse.

(function() {

const PONDS = [
  { label: '1:1', hz: 1.2   },
  { label: '7:4', hz: 2.1   },
  { label: '3:5', hz: 0.72  },
  { label: '9:5', hz: 2.16  },
  { label: '1:7', hz: 0.171 },
];

const VOICES = [
  { id: 'kick',  param: 'kick_mute',  label: 'KCK' },
  { id: 'snare', param: 'snare_mute', label: 'SNR' },
  { id: 'hats',  param: 'hats_mute',  label: 'HAT' },
  { id: 'rim',   param: 'rim_mute',   label: 'RIM' },
  { id: 'stab1', param: 'stab1_mute', label: 'ST1' },
  { id: 'stab2', param: 'stab2_mute', label: 'ST2' },
  { id: 'stab3', param: 'stab3_mute', label: 'ST3' },
  { id: 'pad',   param: 'pad_mute',   label: 'PAD' },
  { id: 'mono',  param: 'mono_mute',  label: 'MNO' },
  { id: 'clave', param: 'clave_mute', label: 'CLV' },
  { id: 'bass',  param: 'bass_mute',  label: 'BAS' },
];

const SIGIL_PATHS = {
  kick:  'M14 2 A12 12 0 1 0 14 26 A12 12 0 1 0 14 2Z',
  snare: 'M4 4 L24 24 M24 4 L4 24',
  hats:  'M14 4 L24 24 L4 24 Z',
  rim:   'M14 4 L24 14 L14 24 L4 14 Z',
  stab1: 'M6 6 H22 V22 H6 Z',
  stab2: 'M14 4 L24 14 L14 24 L4 14 Z',
  stab3: 'M14 3 L25 11 L21 24 L7 24 L3 11 Z',
  pad:   'M4 14 H24 M4 14 A2 2 0 1 0 4 14.01 M24 14 A2 2 0 1 0 24 14.01',
  mono:  'M4 14 L8 6 L12 22 L16 6 L20 22 L24 14',
  clave: 'M10 14 A4 4 0 1 0 10 14.01 M18 14 A4 4 0 1 0 18 14.01',
  bass:  'M4 11 H24 V17 H4 Z',
};

const SIGIL_STYLE = {
  kick:  { fill: true },
  snare: { fill: false },
  hats:  { fill: false },
  rim:   { fill: false },
  stab1: { fill: false },
  stab2: { fill: true },
  stab3: { fill: false },
  pad:   { fill: false },
  mono:  { fill: false },
  clave: { fill: true },
  bass:  { fill: true },
};

const COLORS = [
  { id: 'haze',  label: 'HAZE',  mod: 'haze'  },
  { id: 'drift', label: 'DRIFT', mod: 'drift' },
  { id: 'sweep', label: 'SWEEP', mod: 'sweep' },
  { id: 'echo',  label: 'ECHO',  mod: 'echo'  },
  { id: 'fade',  label: 'FADE',  mod: 'fade'  },
];

// Color stone SVG shapes — concentric rings / wave motifs
const COLOR_PATHS = {
  haze:  'M14 6 A8 8 0 1 0 14 22 A8 8 0 1 0 14 6 M14 10 A4 4 0 1 0 14 18 A4 4 0 1 0 14 10',
  drift: 'M4 14 Q9 8 14 14 Q19 20 24 14',
  sweep: 'M4 20 Q8 4 14 14 Q20 24 24 8',
  echo:  'M8 14 A6 6 0 1 0 8 14.01 M4 14 A10 10 0 1 0 4 14.01 M1 14 A13 13 0 1 0 1 14.01',
  fade:  'M4 14 L8 8 L12 14 L16 8 L20 14 L24 8',
};

const SVG_NS = 'http://www.w3.org/2000/svg';
const POND_RADIUS = 54;
const POND_GAP = 16;
const SIGIL_SIZE = 28;
const MAX_RIPPLES = 3;

// Layout: ponds arranged in a gentle arc
function getPondCenter(index) {
  const cols = 5;
  const row = Math.floor(index / cols);
  const col = index % cols;
  const totalW = cols * (POND_RADIUS * 2 + POND_GAP) - POND_GAP;
  const startX = (700 - totalW) / 2 + POND_RADIUS;
  return {
    x: startX + col * (POND_RADIUS * 2 + POND_GAP),
    y: 120 + row * (POND_RADIUS * 2 + POND_GAP),
  };
}

let placements = {};       // voiceId -> { pondIndex }
let colorPlacements = {};  // colorId -> { pondIndex }
let ripples = {};          // pondIndex -> [{ phase, birth }]
let animId = null;

function initGroovebox() {
  const container = document.getElementById('groovebox');
  if (!container) return;
  container.innerHTML = '';

  const svg = document.createElementNS(SVG_NS, 'svg');
  svg.setAttribute('width', '700');
  svg.setAttribute('height', '390');
  svg.setAttribute('viewBox', '0 0 700 390');
  svg.style.touchAction = 'none';
  svg.style.maxWidth = '100%';
  container.appendChild(svg);

  // Draw ponds
  PONDS.forEach((pond, i) => {
    const c = getPondCenter(i);

    // Pond circle — the drop zone
    const circle = document.createElementNS(SVG_NS, 'circle');
    circle.setAttribute('cx', c.x);
    circle.setAttribute('cy', c.y);
    circle.setAttribute('r', POND_RADIUS);
    circle.setAttribute('fill', 'none');
    circle.setAttribute('stroke', 'var(--rule)');
    circle.setAttribute('stroke-width', '1');
    circle.classList.add('pond');
    circle.dataset.pondIndex = i;
    svg.appendChild(circle);

    // Ripple circles (hidden, animated later)
    for (let r = 0; r < MAX_RIPPLES; r++) {
      const ripple = document.createElementNS(SVG_NS, 'circle');
      ripple.setAttribute('cx', c.x);
      ripple.setAttribute('cy', c.y);
      ripple.setAttribute('r', '0');
      ripple.setAttribute('fill', 'none');
      ripple.setAttribute('stroke', 'var(--muted)');
      ripple.setAttribute('stroke-width', '0.5');
      ripple.setAttribute('opacity', '0');
      ripple.classList.add('ripple');
      ripple.dataset.pondIndex = i;
      ripple.dataset.rippleIndex = r;
      svg.appendChild(ripple);
    }

    // Label below
    const label = document.createElementNS(SVG_NS, 'text');
    label.setAttribute('x', c.x);
    label.setAttribute('y', c.y + POND_RADIUS + 14);
    label.setAttribute('text-anchor', 'middle');
    label.setAttribute('fill', 'var(--muted)');
    label.setAttribute('font-size', '9');
    label.setAttribute('font-family', "'DM Sans', sans-serif");
    label.setAttribute('letter-spacing', '0.1em');
    label.textContent = pond.label;
    svg.appendChild(label);
  });

  // Create sigils in dock
  VOICES.forEach((voice, i) => {
    const g = document.createElementNS(SVG_NS, 'g');
    g.classList.add('sigil-voice');
    g.dataset.voiceId = voice.id;
    g.style.cursor = 'grab';

    const path = document.createElementNS(SVG_NS, 'path');
    path.setAttribute('d', SIGIL_PATHS[voice.id]);
    const style = SIGIL_STYLE[voice.id];
    if (style.fill) {
      path.setAttribute('fill', 'var(--ink)');
      path.setAttribute('stroke', 'none');
    } else {
      path.setAttribute('fill', 'none');
      path.setAttribute('stroke', 'var(--ink)');
      path.setAttribute('stroke-width', '1.5');
      path.setAttribute('stroke-linecap', 'round');
      path.setAttribute('stroke-linejoin', 'round');
    }

    const lbl = document.createElementNS(SVG_NS, 'text');
    lbl.setAttribute('x', SIGIL_SIZE / 2);
    lbl.setAttribute('y', SIGIL_SIZE + 10);
    lbl.setAttribute('text-anchor', 'middle');
    lbl.setAttribute('fill', 'var(--muted)');
    lbl.setAttribute('font-size', '7');
    lbl.setAttribute('font-family', "'DM Sans', sans-serif");
    lbl.setAttribute('letter-spacing', '0.1em');
    lbl.textContent = voice.label;

    g.appendChild(path);
    g.appendChild(lbl);
    svg.appendChild(g);

    setupDrag(g, svg, 'voice');
  });

  // Create color stones in dock
  COLORS.forEach((color, i) => {
    const g = document.createElementNS(SVG_NS, 'g');
    g.classList.add('sigil-color');
    g.dataset.colorId = color.id;
    g.style.cursor = 'grab';

    const path = document.createElementNS(SVG_NS, 'path');
    path.setAttribute('d', COLOR_PATHS[color.id]);
    path.setAttribute('fill', 'none');
    path.setAttribute('stroke', 'var(--muted)');
    path.setAttribute('stroke-width', '1.2');
    path.setAttribute('stroke-linecap', 'round');
    path.setAttribute('stroke-linejoin', 'round');
    path.setAttribute('stroke-dasharray', '3 2');

    const lbl = document.createElementNS(SVG_NS, 'text');
    lbl.setAttribute('x', SIGIL_SIZE / 2);
    lbl.setAttribute('y', SIGIL_SIZE + 10);
    lbl.setAttribute('text-anchor', 'middle');
    lbl.setAttribute('fill', 'var(--muted)');
    lbl.setAttribute('font-size', '6');
    lbl.setAttribute('font-family', "'DM Sans', sans-serif");
    lbl.setAttribute('letter-spacing', '0.12em');
    lbl.textContent = color.label;

    g.appendChild(path);
    g.appendChild(lbl);
    svg.appendChild(g);

    setupDrag(g, svg, 'color');
  });

  positionDock(svg);
}

function getDockPosition(index, row) {
  if (row === 0) {
    // Voice stones: 11 in a row
    const cols = 11;
    const col = index % cols;
    const totalW = cols * 48;
    const startX = (700 - totalW) / 2 + 24;
    return { x: startX + col * 48, y: 270 };
  } else {
    // Color stones: 5 centered
    const cols = 5;
    const col = index % cols;
    const totalW = cols * 64;
    const startX = (700 - totalW) / 2 + 32;
    return { x: startX + col * 64, y: 330 };
  }
}

function positionDock(svg) {
  svg.querySelectorAll('.sigil-voice').forEach((g, i) => {
    const voiceId = g.dataset.voiceId;
    if (!placements[voiceId]) {
      const pos = getDockPosition(i, 0);
      g.setAttribute('transform', `translate(${pos.x - SIGIL_SIZE/2}, ${pos.y - SIGIL_SIZE/2})`);
    }
  });
  svg.querySelectorAll('.sigil-color').forEach((g, i) => {
    const colorId = g.dataset.colorId;
    if (!colorPlacements[colorId]) {
      const pos = getDockPosition(i, 1);
      g.setAttribute('transform', `translate(${pos.x - SIGIL_SIZE/2}, ${pos.y - SIGIL_SIZE/2})`);
    }
  });
}

function setupDrag(g, svg, stoneType) {
  let dragging = false;
  let offsetX = 0, offsetY = 0;
  let currentX = 0, currentY = 0;

  function getPointerPos(e) {
    const pt = svg.createSVGPoint();
    pt.x = e.clientX;
    pt.y = e.clientY;
    return pt.matrixTransform(svg.getScreenCTM().inverse());
  }

  function onDown(e) {
    e.preventDefault();
    dragging = true;
    g.style.cursor = 'grabbing';
    const pos = getPointerPos(e);
    const transform = g.getAttribute('transform');
    const match = transform && transform.match(/translate\(([\d.-]+),\s*([\d.-]+)\)/);
    if (match) {
      currentX = parseFloat(match[1]);
      currentY = parseFloat(match[2]);
    }
    offsetX = pos.x - currentX - SIGIL_SIZE/2;
    offsetY = pos.y - currentY - SIGIL_SIZE/2;
    g.parentNode.appendChild(g);
    g.setPointerCapture(e.pointerId);
  }

  function onMove(e) {
    if (!dragging) return;
    e.preventDefault();
    const pos = getPointerPos(e);
    currentX = pos.x - offsetX - SIGIL_SIZE/2;
    currentY = pos.y - offsetY - SIGIL_SIZE/2;
    g.setAttribute('transform', `translate(${currentX}, ${currentY})`);
  }

  function findDropPond() {
    const cx = currentX + SIGIL_SIZE / 2;
    const cy = currentY + SIGIL_SIZE / 2;
    let droppedPond = -1;
    PONDS.forEach((_, i) => {
      const pc = getPondCenter(i);
      const dist = Math.sqrt((cx - pc.x) ** 2 + (cy - pc.y) ** 2);
      if (dist < POND_RADIUS - 4) {
        droppedPond = i;
      }
    });
    return droppedPond;
  }

  function onUp() {
    if (!dragging) return;
    dragging = false;
    g.style.cursor = 'grab';

    const droppedPond = findDropPond();

    if (stoneType === 'voice') {
      const voiceId = g.dataset.voiceId;
      if (droppedPond >= 0) {
        const oldPond = placements[voiceId]?.pondIndex;
        placements[voiceId] = { pondIndex: droppedPond };
        if (oldPond !== undefined && oldPond !== droppedPond) {
          layoutPondStones(svg, oldPond);
        }
        layoutPondStones(svg, droppedPond);
        onVoiceActivated(voiceId, droppedPond);
      } else {
        const oldPond = placements[voiceId]?.pondIndex;
        delete placements[voiceId];
        if (oldPond !== undefined) {
          layoutPondStones(svg, oldPond);
        }
        const idx = VOICES.findIndex(v => v.id === voiceId);
        const pos = getDockPosition(idx, 0);
        g.setAttribute('transform', `translate(${pos.x - SIGIL_SIZE/2}, ${pos.y - SIGIL_SIZE/2})`);
        onVoiceDeactivated(voiceId);
      }
    } else {
      const colorId = g.dataset.colorId;
      if (droppedPond >= 0) {
        const oldPond = colorPlacements[colorId]?.pondIndex;
        colorPlacements[colorId] = { pondIndex: droppedPond };
        if (oldPond !== undefined && oldPond !== droppedPond) {
          layoutPondStones(svg, oldPond);
        }
        layoutPondStones(svg, droppedPond);
        onColorActivated(colorId, droppedPond);
      } else {
        const oldPond = colorPlacements[colorId]?.pondIndex;
        delete colorPlacements[colorId];
        if (oldPond !== undefined) {
          layoutPondStones(svg, oldPond);
        }
        const idx = COLORS.findIndex(c => c.id === colorId);
        const pos = getDockPosition(idx, 1);
        g.setAttribute('transform', `translate(${pos.x - SIGIL_SIZE/2}, ${pos.y - SIGIL_SIZE/2})`);
        onColorDeactivated(colorId, oldPond);
      }
    }
  }

  g.addEventListener('pointerdown', onDown);
  g.addEventListener('pointermove', onMove);
  g.addEventListener('pointerup', onUp);
  g.addEventListener('pointercancel', onUp);
}

// Arrange all stones (voice + color) inside a pond
function layoutPondStones(svg, pondIndex) {
  const pc = getPondCenter(pondIndex);

  // Gather all stones in this pond
  const allStones = [];
  Object.entries(placements)
    .filter(([_, p]) => p.pondIndex === pondIndex)
    .forEach(([id]) => allStones.push({ id, type: 'voice', selector: `[data-voice-id="${id}"]` }));
  Object.entries(colorPlacements)
    .filter(([_, p]) => p.pondIndex === pondIndex)
    .forEach(([id]) => allStones.push({ id, type: 'color', selector: `[data-color-id="${id}"]` }));

  if (allStones.length === 0) return;

  allStones.forEach((stone, i) => {
    const g = svg.querySelector(stone.selector);
    if (!g) return;

    let sx, sy;
    if (allStones.length === 1) {
      sx = pc.x;
      sy = pc.y;
    } else {
      const angle = (i / allStones.length) * 2 * Math.PI - Math.PI / 2;
      const spread = Math.min(12 + allStones.length * 4, POND_RADIUS - SIGIL_SIZE/2 - 4);
      sx = pc.x + spread * Math.cos(angle);
      sy = pc.y + spread * Math.sin(angle);
    }

    g.setAttribute('transform', `translate(${sx - SIGIL_SIZE/2}, ${sy - SIGIL_SIZE/2})`);
  });
}

function onVoiceActivated(voiceId, pondIndex) {
  if (typeof window.grooveboxOnVoice === 'function') {
    window.grooveboxOnVoice(voiceId, true, pondIndex);
  }
}

function onVoiceDeactivated(voiceId) {
  if (typeof window.grooveboxOnVoice === 'function') {
    window.grooveboxOnVoice(voiceId, false, -1);
  }
}

function onColorActivated(colorId, pondIndex) {
  if (typeof window.grooveboxOnColor === 'function') {
    window.grooveboxOnColor(colorId, pondIndex, true);
  }
}

function onColorDeactivated(colorId, oldPondIndex) {
  if (typeof window.grooveboxOnColor === 'function') {
    window.grooveboxOnColor(colorId, oldPondIndex, false);
  }
}

// ── Ripple animation ──
// Each pond with at least one stone emits gentle ripples at its ratio frequency

function startAnimation() {
  let lastTime = performance.now();

  // Initialize ripple state
  PONDS.forEach((_, i) => {
    ripples[i] = { phase: 0 };
  });

  function animate(now) {
    const dt = (now - lastTime) / 1000;
    lastTime = now;

    const svg = document.querySelector('#groovebox svg');
    if (!svg) { animId = requestAnimationFrame(animate); return; }

    // Which ponds have stones?
    const activePonds = new Set();
    for (const [_, p] of Object.entries(placements)) {
      activePonds.add(p.pondIndex);
    }

    PONDS.forEach((pond, i) => {
      const state = ripples[i];
      if (!activePonds.has(i)) {
        // Reset ripples for inactive ponds
        state.phase = 0;
        for (let r = 0; r < MAX_RIPPLES; r++) {
          const el = svg.querySelector(`.ripple[data-pond-index="${i}"][data-ripple-index="${r}"]`);
          if (el) el.setAttribute('opacity', '0');
        }
        return;
      }

      // Advance phase
      state.phase += pond.hz * dt;

      // Each ripple is offset by 1/MAX_RIPPLES of the cycle
      for (let r = 0; r < MAX_RIPPLES; r++) {
        const el = svg.querySelector(`.ripple[data-pond-index="${i}"][data-ripple-index="${r}"]`);
        if (!el) continue;

        const ripplePhase = (state.phase + r / MAX_RIPPLES) % 1;
        const radius = ripplePhase * POND_RADIUS;
        const opacity = 0.3 * (1 - ripplePhase);

        el.setAttribute('r', radius);
        el.setAttribute('opacity', opacity);
      }

      // Pulse the pond border gently
      const pondEl = svg.querySelector(`.pond[data-pond-index="${i}"]`);
      if (pondEl) {
        const pulse = 0.5 + 0.5 * Math.sin(state.phase * 2 * Math.PI);
        const sw = 0.5 + pulse * 0.8;
        pondEl.setAttribute('stroke-width', sw);
      }
    });

    animId = requestAnimationFrame(animate);
  }

  animId = requestAnimationFrame(animate);
}

function stopAnimation() {
  if (animId) {
    cancelAnimationFrame(animId);
    animId = null;
  }
}

window.groovebox = {
  init: initGroovebox,
  startAnimation,
  stopAnimation,
  getPlacements: () => placements,
  getColorPlacements: () => colorPlacements,
  VOICES,
  COLORS,
};

})();
