'use strict';
// =====================================================================
// Sounder window page. The Rust Session is the only source of truth: this
// file draws `state` and the bulk data (design/window-api.md) and sends the
// human's actions back as calls. There is no audio or DSP here.
// =====================================================================

// ---------- transport: sounder.call over window.ipc, or the fixture in a plain browser ----------
const IN_WINDOW = !!(window.ipc && window.ipc.postMessage);
const FONT_BASE = IN_WINDOW ? 'sounder://fonts/' : 'fonts/';
const sounder = window.sounder = {
  _id: 0, _pending: new Map(),
  call(name, args = {}) {
    if (!IN_WINDOW) return FIX.call(name, args);
    return new Promise((resolve, reject) => {
      const id = ++this._id; this._pending.set(id, { resolve, reject });
      window.ipc.postMessage(JSON.stringify({ id, name, args }));
    });
  },
  _reply(id, r) { const p = this._pending.get(id); if (!p) return; this._pending.delete(id); r && r.ok ? p.resolve(r.result) : p.reject(new Error(r && r.error ? r.error : 'the call failed')); },
  changed(rev) { onChanged(rev); },
  played(ev) { onPlayed(ev); },
};
// GET bulk data as an ArrayBuffer (sounder://render/...). In a plain browser the fixture answers.
async function bulk(path, q) {
  const qs = Object.entries(q || {}).filter(([, v]) => v !== undefined && v !== null).map(([k, v]) => `${k}=${encodeURIComponent(v)}`).join('&');
  if (!IN_WINDOW) return FIX.bulk(path, q || {});
  const r = await fetch(`sounder://${path}${qs ? '?' + qs : ''}`);
  if (!r.ok) throw new Error(`${path}: ${r.status}`);
  return r.arrayBuffer();
}
const errText = e => (e && e.message) || String(e);

// ---------- presentation tables (no signal processing) ----------
const ENUMS = { shape: ['sine', 'saw', 'square', 'tri', 'pulse'], mode: ['lp', 'hp', 'bp', 'notch', 'peak'], curve: ['linear', 'exp', 'smooth'], color: ['white', 'pink', 'brown'], op: ['add', 'sub', 'mul', 'mix', 'min', 'max'], window: ['hann', 'tri', 'rect'] };
const BLURB = {
  osc: 'band-limited oscillator; freq may be a node (pitch sweeps, FM)', noise: 'white / pink / brown noise (seeded)', impulse: 'single-sample clicks at the given times', sample: 'a WAV file as a source',
  env: 'breakpoint envelope [[t, v], ...]', adsr: 'attack / decay / sustain / release envelope', decay: 'exponential fall from 1 (time = seconds to -60 dB)', follower: 'follows the level of a signal',
  filter: 'state-variable filter; freq and q take signals', onepole: 'one-pole low / high pass', comb: 'comb filter', allpass: 'allpass filter',
  resonators: 'bank of ringing modes [[hz, decay_s, gain], ...]', ramp: 'a sweep from one value to another', lfo: 'low-frequency oscillator, -1..1 or a range',
  wander: 'smooth random motion at a rate (seeded)', sample_hold: 'holds a value, sampling on a rate', slew: 'limits how fast a signal may change',
  math: 'arithmetic on signals or numbers', remap: 'maps a range onto another', mix: 'sums inputs with gain and time offset', gain: 'scales by db or gain', pan: 'places a mono signal in the stereo field', width: 'stereo width',
  delay: 'interpolated delay with feedback', reverb: 'reverb', convolve: 'convolves with an impulse response', shape: 'waveshaper', compress: 'compressor', limit: 'limiter',
  reverse: 'plays the sound backwards', normalize: 'scales to a target level', trim: 'cuts the start / end', fade: 'fades in / out',
  pitch_shift: 'offline pitch shift (phase vocoder)', time_stretch: 'offline time stretch (phase vocoder)', spectral: 'offline spectral freeze / blur / gate / morph',
  once: 'a single event', pattern: 'events at fixed times', poisson: 'random events at a rate', bounce: 'events of a bouncing ball', burst: 'a cluster of events', on_threshold: 'an event at each rising crossing',
  grains: 'windowed grains placed at events', voices: 'a sub-patch rendered once per event', sound: 'another sound, instanced', expr: 'per-sample maths',
  output: 'gain, loudness target, edge fades, ceiling: exactly one per sound',
};
const ICON = { osc: 'wave', noise: 'noise', impulse: 'ring', sample: 'wave', env: 'env', adsr: 'env', decay: 'env', follower: 'env', filter: 'filter', onepole: 'filter', comb: 'filter', allpass: 'filter', resonators: 'ring', ramp: 'ctrl', lfo: 'ctrl', wander: 'ctrl', sample_hold: 'ctrl', slew: 'ctrl', math: 'math', remap: 'math', gain: 'math', mix: 'mix', pan: 'mix', width: 'mix', delay: 'mix', reverb: 'mix', convolve: 'mix', shape: 'filter', output: 'out' };
const CLAIMS = { centroid: 'centroid', decay: 'ring-out', crest: 'crest', lufs: 'loudness', flatness: 'flatness', swing: 'loudness swing' };
const BLAB = ['63', '125', '250', '500', '1k', '2k', '4k', '8k+'];
const AGENT_COLORS = ['var(--a1)', 'var(--a2)', 'var(--a3)'];

// ---------- helpers ----------
const $ = (s, r = document) => r.querySelector(s);
const ic = id => `<svg class="i"><use href="#i-${id}"/></svg>`;
const esc = s => String(s ?? '').replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const fmtHz = f => f >= 1000 ? (f / 1000).toFixed(f >= 10000 ? 0 : 1) + 'k' : Math.round(f) + '';
const clone = o => JSON.parse(JSON.stringify(o));
const pinText = on => on.node ? `node ${on.node}` : on.t ? `${on.t[0].toFixed(2)}-${on.t[1].toFixed(2)} s${on.hz ? ` · ${fmtHz(on.hz[0])}-${fmtHz(on.hz[1])} Hz` : ''}` : 'whole sound';
const onOf = c => ({ node: c.node || null, t: c.time || null, hz: c.hz || null });
const sme = v => v == null ? NaN : typeof v === 'number' ? v : (v.middle ?? v.mid ?? v.start);
function agentColor(name) {
  if (name === 'you' || name === 'window') return 'var(--a3)';
  const others = (ST.agents || []).map(a => a.name).filter(n => n !== 'you' && n !== 'window'); const i = others.indexOf(name);
  return AGENT_COLORS[Math.max(0, i) % 2];
}

// ---------- state ----------
let ST = null;                 // the last `state` result
const S = { view: 'doc', centre: 'sound', sel: null, tab: 'node', loop: false, region: null, zoom: null, hot: null, pendingPin: null };
const P = { playing: false, pos: 0, loop: false, span: null, node: null, at: 0 };   // player mirror, extrapolated with rAF
let CMAP = null;               // 256 RGBA colormap bytes from sounder://render/colormap
const doc = () => ST.sound;
const nodeOf = name => doc().nodes.find(n => n.name === name);
const isOut = name => { const n = nodeOf(name); return n && n.type === 'output'; };
let PLOTREV = 0, PLOTBUST = 0; // cache busters for the bulk GETs

// ---------- pulling state ----------
let pulling = null, pullAgain = false, pullTimer = null, dragRight = false;
function schedulePull(ms = 25) { clearTimeout(pullTimer); pullTimer = setTimeout(() => pullState(), ms); }
function pullState(docName) {
  if (pulling) { pullAgain = true; return pulling; }
  pulling = (async () => {
    try { do { pullAgain = false; applyState(await sounder.call('state', docName ? { doc: docName } : {})); docName = null; } while (pullAgain); }
    catch (e) { toast(errText(e)); }
    finally { pulling = null; }
  })();
  return pulling;
}
function defaultSel(s) { return (s.nodes.find(n => n.type === 'filter') || s.nodes[0] || {}).name || null; }
function applyState(s) {
  const prev = ST;
  if (KD && prev && s.sound && prev.sound) { const m = s.sound.macros.find(x => x.name === KD.name), o = prev.sound.macros.find(x => x.name === KD.name); if (m && o) m.value = o.value; }
  ST = s; PLOTREV = Math.max(PLOTREV, s.rev || 0);
  const docChanged = !prev || prev.doc !== s.doc;
  if (docChanged) { S.region = null; S.zoom = null; S.pendingPin = null; S.hot = null; }
  if (!s.sound.nodes.some(n => n.name === S.sel)) S.sel = defaultSel(s.sound);
  if (prev && prev.playing !== s.playing) { P.playing = s.playing; P.at = performance.now(); }
  if (!prev) P.playing = s.playing;
  renderAll();
}
function onChanged(rev) {
  if (rev <= PLOTREV && ST) return;
  PLOTREV = rev;
  if (KD) { refreshPlot(); return; }       // a knob is being dragged: follow with the picture only
  schedulePull(0);
}
function onPlayed(ev) {
  P.playing = !!ev.playing; P.pos = ev.pos || 0; P.loop = !!ev.loop; P.span = ev.span || null; P.node = ev.node ?? null; P.at = performance.now();
  S.loop = !!ev.loop && !(ST && ST.sound.kind === 'loop' && !ev.span);
  if (ST) renderTop();
}
// A mutating call: show the Rust side's text on error, re-pull state when it returns.
async function act(name, args) {
  try { const r = await sounder.call(name, args); schedulePull(10); return r; }
  catch (e) { toast(errText(e)); schedulePull(10); }
}

// ---------- player (the playhead is extrapolated here; audio is Rust's) ----------
function position() {
  if (!P.playing) return P.pos;
  let p = P.pos + (performance.now() - P.at) / 1000; const L = doc().seconds;
  if (P.loop) { const [a, b] = P.span || [0, L]; if (b > a && p >= b) p = a + ((p - a) % (b - a)); }
  else if (p >= L) { P.playing = false; P.pos = 0; renderTop(); return 0; }
  return p;
}
function playArgs(from) { const d = doc(), loop = S.loop || d.kind === 'loop'; const a = { from, loop }; if (S.region && S.loop) a.span = [S.region.t[0], S.region.t[1]]; return a; }
function toggle() { if (P.playing) act('stop', {}); else act('play', playArgs(S.region ? S.region.t[0] : 0)); }
function restartKeeping() { if (P.playing) act('play', playArgs(position())); }

// ---------- top bar ----------
function renderTop() {
  const d = doc();
  $('#gridbtn').setAttribute('aria-pressed', S.view === 'grid'); $('#docbtn').setAttribute('aria-pressed', S.view !== 'grid');
  $('#doccount').textContent = ST.sounds.length; $('#cdoc').textContent = ST.doc; $('.docproj').textContent = `${ST.project || 'friendSLOP'} /`;
  $('#transport').hidden = S.view === 'grid';
  $('#playbtn').innerHTML = ic(P.playing ? 'stop' : 'play'); $('#playbtn').setAttribute('aria-label', P.playing ? 'Stop' : 'Play');
  $('#loopbtn').setAttribute('aria-pressed', S.loop || d.kind === 'loop');
  $('#seedv').textContent = `seed ${d.seed}`;
  const what = ST.solo ? `node <b>${esc(ST.solo)}</b>` : `<b>out</b>`;
  $('#hearing').innerHTML = `${ic('phones')} Hearing ${what}${S.region && S.loop ? ' · region' : ''}${ST.solo ? `<button data-unsolo aria-label="Stop soloing">${ic('x')}</button>` : ''}`;
  $('#agents').innerHTML = (ST.agents || []).filter(a => a.name !== 'you' && a.name !== 'window').map(a => `<span class="agent" title="${esc(a.name)}: ${a.live ? 'working now' : 'idle'}"><i class="dot ${a.live ? 'live' : ''}" style="background:${agentColor(a.name)}"></i><span>${esc(a.name)}</span></span>`).join('');
}

// ---------- centre: hud ----------
function renderHud() {
  const d = doc();
  const z = S.zoom ? `<span class="chip">${ic('zoom')}<b>${S.zoom.t[0].toFixed(2)}-${S.zoom.t[1].toFixed(2)} s${S.zoom.hz ? ` · ${fmtHz(S.zoom.hz[0])}-${fmtHz(S.zoom.hz[1])} Hz` : ''}</b><button data-unzoom aria-label="Reset zoom">${ic('x')}</button></span>` : '';
  const stats = `<span class="chip"><b>${d.seconds.toFixed(2)} s</b></span><span class="chip">pk <b>${d.peak_db.toFixed(1)}</b></span><span class="chip"><b>${d.lufs.toFixed(1)}</b> LUFS</span>`;
  $('#hudbar').innerHTML = `<div class="seg" role="group" aria-label="Centre"><button data-centre="sound" aria-pressed="${S.centre === 'sound'}">Spectrogram <kbd>1</kbd></button><button data-centre="graph" aria-pressed="${S.centre === 'graph'}">Graph <kbd>2</kbd></button></div>${z}<span class="grow"></span>${stats}<button class="tbtn" data-hist title="History (H): undo / redo and compare">${ic('hist')}<span class="lbl">v${d.version}</span>${ic('chev')}</button>`;
}

// ---------- centre: plot (draws bulk data; nothing is computed here) ----------
const G = { gut: 46, waveH: 86, loudH: 54, axisH: 20, gap: 8 };
const DPR = Math.min(2, window.devicePixelRatio || 1);
function geom() { const c = $('#base'), W = c.clientWidth, H = c.clientHeight; const specH = Math.max(80, H - G.waveH - G.loudH - G.axisH - 3 * G.gap - 6); return { W, H, pw: W - G.gut - 10, specT: 6, specH, waveT: 6 + specH + G.gap, loudT: 6 + specH + 2 * G.gap + G.waveH }; }
function span() { const L = doc().seconds; const t = S.zoom ? S.zoom.t : [0, L]; const hz = S.zoom && S.zoom.hz ? S.zoom.hz : [20, Math.min(20000, doc().sample_rate / 2)]; return { t, hz, L }; }
const xOf = (t, g, sp) => G.gut + (t - sp.t[0]) / (sp.t[1] - sp.t[0]) * g.pw;
const tOf = (x, g, sp) => sp.t[0] + (x - G.gut) / g.pw * (sp.t[1] - sp.t[0]);
const yOf = (hz, g, sp) => g.specT + (1 - Math.log(hz / sp.hz[0]) / Math.log(sp.hz[1] / sp.hz[0])) * g.specH;
const hzOf = (y, g, sp) => sp.hz[0] * (sp.hz[1] / sp.hz[0]) ** (1 - (y - g.specT) / g.specH);

const PLOT = { key: null, spec: null, wave: null, loud: null, sp: null };
let plotSeq = 0, plotDrawn = 0;
const parseSpec = buf => { const dv = new DataView(buf); const w = dv.getUint32(0, true), h = dv.getUint32(4, true); return { w, h, t0: dv.getFloat32(8, true), t1: dv.getFloat32(12, true), db: new Float32Array(buf, 16, w * h) }; };
// dB grid -> a canvas, with the colormap bytes (-90 dB = black, 0 = pale)
function specCanvas(s) {
  const c = document.createElement('canvas'); c.width = s.w; c.height = s.h; const ctx = c.getContext('2d'), img = ctx.createImageData(s.w, s.h), d = img.data;
  for (let i = 0; i < s.db.length; i++) { const k = Math.max(0, Math.min(255, Math.round((s.db[i] + 90) / 90 * 255))) * 4, o = i * 4; d[o] = CMAP[k]; d[o + 1] = CMAP[k + 1]; d[o + 2] = CMAP[k + 2]; d[o + 3] = 255; }
  ctx.putImageData(img, 0, 0); return c;
}
function plotParams() { const g = geom(), sp = span(); return { w: Math.max(8, Math.round(g.pw * DPR)), h: Math.max(8, Math.round(g.specH * DPR)), pw: Math.max(8, Math.round(g.pw)), t0: sp.t[0], t1: sp.t[1], hz0: sp.hz[0], hz1: sp.hz[1], node: ST.solo || undefined }; }
async function refreshPlot(force) {
  if (!ST || S.view === 'grid' || S.centre !== 'sound' || !CMAP) return;
  const g = geom(); if (!g.W || !g.H) return;
  const p = plotParams(), name = ST.doc, key = JSON.stringify([name, PLOTREV, PLOTBUST, p]);
  if (key === PLOT.key && !force) return;
  PLOT.key = key; const seq = ++plotSeq, base = { rev: PLOTREV, b: PLOTBUST, node: p.node };
  try {
    const [sb, wb, lb] = await Promise.all([
      bulk(`render/${name}/spectrogram`, { ...base, w: p.w, h: p.h, t0: p.t0, t1: p.t1, hz0: p.hz0, hz1: p.hz1 }),
      bulk(`render/${name}/wave`, { ...base, w: p.pw, t0: p.t0, t1: p.t1 }),
      bulk(`render/${name}/loudness`, { ...base, w: p.pw, t0: p.t0, t1: p.t1 })]);
    if (seq < plotDrawn || ST.doc !== name) return; plotDrawn = seq;
    PLOT.spec = specCanvas(parseSpec(sb)); PLOT.wave = new Float32Array(wb); PLOT.loud = new Float32Array(lb); PLOT.sp = { t: [p.t0, p.t1], hz: [p.hz0, p.hz1] };
    renderPlot();
  } catch (e) { PLOT.key = null; toast(errText(e)); }
}
function renderPlot() {
  const c = $('#base'), o = $('#ov'), W = c.clientWidth, H = c.clientHeight; if (!W || !H) return;
  c.width = W * DPR; c.height = H * DPR; o.width = W; o.height = H;
  const g = geom(), sp = PLOT.sp || span(), ctx = c.getContext('2d'); ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
  ctx.fillStyle = '#121418'; ctx.fillRect(0, 0, W, H);
  if (PLOT.spec) ctx.drawImage(PLOT.spec, G.gut, g.specT, g.pw, g.specH);
  ctx.font = '11px IBM Plex Mono, monospace'; ctx.textBaseline = 'middle';
  for (const [f, l] of [[31.25, '31'], [62.5, '63'], [125, '125'], [250, '250'], [500, '500'], [1000, '1k'], [2000, '2k'], [4000, '4k'], [8000, '8k'], [16000, '16k']]) {
    if (f < sp.hz[0] || f > sp.hz[1]) continue; const y = yOf(f, g, sp); ctx.strokeStyle = 'rgba(255,255,255,.08)'; ctx.setLineDash([2, 3]); ctx.beginPath(); ctx.moveTo(G.gut, y); ctx.lineTo(G.gut + g.pw, y); ctx.stroke(); ctx.setLineDash([]); ctx.fillStyle = '#8a91a0'; ctx.fillText(l, 8, y); }
  // waveform: peak dim, RMS bright (columns: min, max, rms back to back)
  const mid = g.waveT + G.waveH / 2, half = G.waveH / 2 - 3; ctx.fillStyle = '#181b21'; ctx.fillRect(G.gut, g.waveT, g.pw, G.waveH);
  if (PLOT.wave) { const n = PLOT.wave.length / 3, mn = PLOT.wave.subarray(0, n), mx = PLOT.wave.subarray(n, 2 * n), rm = PLOT.wave.subarray(2 * n);
    for (let i = 0; i < n; i++) { const x = G.gut + i * g.pw / n, w = Math.max(1, g.pw / n);
      ctx.fillStyle = 'rgba(120,200,255,.35)'; ctx.fillRect(x, mid - mx[i] * half, w, Math.max(1, (mx[i] - mn[i]) * half)); ctx.fillStyle = 'rgba(120,200,255,.95)'; ctx.fillRect(x, mid - rm[i] * half, w, Math.max(1, 2 * rm[i] * half)); } }
  ctx.fillStyle = '#5b6270'; ctx.fillText('wave', 8, g.waveT + 8);
  // loudness (LUFS columns)
  const yl = l => g.loudT + Math.min(1, Math.max(0, -l / 60)) * G.loudH; ctx.fillStyle = '#181b21'; ctx.fillRect(G.gut, g.loudT, g.pw, G.loudH);
  for (const l of [-12, -24, -36, -48]) { ctx.strokeStyle = 'rgba(255,255,255,.07)'; ctx.beginPath(); ctx.moveTo(G.gut, yl(l)); ctx.lineTo(G.gut + g.pw, yl(l)); ctx.stroke(); }
  ctx.fillStyle = '#5b6270'; ctx.fillText('LUFS', 8, g.loudT + 8); ctx.fillText('-24', 14, yl(-24));
  if (PLOT.loud) { const n = PLOT.loud.length; ctx.strokeStyle = '#f6a04d'; ctx.lineWidth = 1.5; ctx.beginPath(); for (let i = 0; i < n; i++) { const X = G.gut + (i + 0.5) * g.pw / n; i ? ctx.lineTo(X, yl(PLOT.loud[i])) : ctx.moveTo(X, yl(PLOT.loud[i])); } ctx.stroke(); ctx.lineWidth = 1; }
  // time axis
  const dur = sp.t[1] - sp.t[0], raw = dur / 7, p10 = 10 ** Math.floor(Math.log10(raw)); const step = [1, 2, 5, 10].map(m => m * p10).find(s => s >= raw);
  ctx.textAlign = 'center'; for (let t = Math.ceil(sp.t[0] / step) * step; t <= sp.t[1] + 1e-9; t += step) { const X = xOf(t, g, sp); ctx.strokeStyle = 'rgba(255,255,255,.06)'; ctx.beginPath(); ctx.moveTo(X, g.specT); ctx.lineTo(X, g.loudT + G.loudH); ctx.stroke(); ctx.fillStyle = '#8a91a0'; ctx.fillText(t.toFixed(step < 0.01 ? 3 : step < 0.1 ? 2 : 1) + 's', Math.min(W - 18, Math.max(G.gut + 12, X)), g.loudT + G.loudH + 11); }
  ctx.textAlign = 'left';
  renderPins();
}
function renderPins() {
  const g = geom(), sp = span(), d = doc(); const host = $('#pins'); host.innerHTML = '';
  if (S.centre !== 'sound' || ST.solo) return;
  d.comments.forEach(cm => { if (!cm.time) return; const x = xOf(cm.time[0], g, sp), y = cm.hz ? yOf(cm.hz[1], g, sp) : g.waveT; if (x < G.gut - 2 || x > G.gut + g.pw) return;
    const b = document.createElement('div'); b.className = 'pinb' + (cm.resolved ? ' done' : ''); b.style.left = x + 'px'; b.style.top = Math.max(10, y) + 'px'; b.textContent = cm.id; b.title = cm.text; b.dataset.cmt = cm.id; host.append(b); });
}
let DRAG = null;
function overlay() {
  const o = $('#ov'), ctx = o.getContext('2d'); ctx.clearRect(0, 0, o.width, o.height); if (!ST || S.centre !== 'sound' || S.view === 'grid') return requestAnimationFrame(overlay);
  const g = geom(), sp = span(), d = doc();
  if (!ST.solo) d.comments.forEach(cm => { if (!cm.time) return; const x0 = xOf(cm.time[0], g, sp), x1 = xOf(cm.time[1], g, sp); const y0 = cm.hz ? yOf(cm.hz[1], g, sp) : g.specT, y1 = cm.hz ? yOf(cm.hz[0], g, sp) : g.loudT + G.loudH;
    ctx.strokeStyle = cm.resolved ? 'rgba(138,145,160,.45)' : 'rgba(226,178,79,.9)'; ctx.setLineDash(cm.resolved ? [3, 3] : []); ctx.strokeRect(x0, y0, x1 - x0, y1 - y0); ctx.setLineDash([]); });
  const r = DRAG ? DRAG.r : S.region;
  if (r) { const x0 = xOf(r.t[0], g, sp), x1 = xOf(r.t[1], g, sp); const y0 = r.hz ? yOf(r.hz[1], g, sp) : g.specT, y1 = r.hz ? yOf(r.hz[0], g, sp) : g.loudT + G.loudH;
    ctx.fillStyle = 'rgba(111,168,255,.12)'; ctx.fillRect(x0, y0, x1 - x0, y1 - y0); ctx.strokeStyle = '#6fa8ff'; ctx.strokeRect(x0 + .5, y0 + .5, x1 - x0, y1 - y0);
    ctx.fillStyle = '#6fa8ff'; ctx.font = '11px IBM Plex Mono, monospace'; ctx.fillText(pinText(r), x0 + 4, y0 - 7 < 8 ? y1 + 12 : y0 - 7); }
  if (P.playing || P.pos > 0) { const X = xOf(position(), g, sp); if (X >= G.gut && X <= G.gut + g.pw) { ctx.fillStyle = '#e3e6ec'; ctx.fillRect(X, g.specT, 1.5, g.loudT + G.loudH - g.specT); } }
  if (ST.solo) { ctx.fillStyle = 'rgba(111,168,255,.9)'; ctx.font = '600 11px IBM Plex Sans, sans-serif'; const t = `solo · ${ST.solo}`; ctx.fillRect(G.gut + 6, g.specT + 6, ctx.measureText(t).width + 12, 18); ctx.fillStyle = '#111'; ctx.fillText(t, G.gut + 12, g.specT + 15); }
  requestAnimationFrame(overlay);
}
function showRegbar() {
  const bar = $('#regbar'); if (!S.region || S.centre !== 'sound') { bar.hidden = true; return; }
  const g = geom(), sp = span(); const x = Math.min(g.W - 330, Math.max(8, xOf(S.region.t[0], g, sp))); const y0 = S.region.hz ? yOf(S.region.hz[1], g, sp) : g.specT;
  bar.innerHTML = `<button data-rg="play">${ic('play')}Play</button><button data-rg="loop">${ic('loop')}Loop it</button><button data-rg="zoom">${ic('zoom')}Zoom</button><span class="sep"></span><button data-rg="comment">${ic('pin')}Comment</button><button data-rg="clear" aria-label="Clear region">${ic('x')}</button>`;
  bar.style.left = x + 'px'; bar.style.top = (y0 - 52 > 4 ? y0 - 52 : (S.region.hz ? yOf(S.region.hz[0], g, sp) : g.loudT + G.loudH) + 20) + 'px'; bar.hidden = false;
}

// ---------- centre: graph ----------
const SPARKS = new Map();
function getSpark(name) {
  const key = `${ST.doc}|${PLOTREV}|${PLOTBUST}|${name}`;
  if (!SPARKS.has(key)) { if (SPARKS.size > 400) SPARKS.clear(); SPARKS.set(key, bulk(`render/${ST.doc}/node/${name}/spark`, { rev: PLOTREV, b: PLOTBUST }).then(b => new Float32Array(b))); }
  return SPARKS.get(key);
}
function drawSpark(c, x) {
  const g = c.getContext('2d'), W = c.width = 92, H = c.height = 32; g.clearRect(0, 0, W, H); if (!x || !x.length) return; let pk = 1e-9; for (const v of x) pk = Math.max(pk, Math.abs(v)); g.fillStyle = 'rgba(120,200,255,.75)';
  for (let i = 0; i < W; i++) { const m = Math.abs(x[Math.min(x.length - 1, Math.floor(i * x.length / W))]); const h = Math.max(1, m / pk * (H / 2 - 1)); g.fillRect(i, H / 2 - h, 1, 2 * h); }
}
function macroTargets(name) { const m = doc().macros.find(x => x.name === name); return m ? m.map.map(x => x.node) : []; }
function renderGraph() {
  const host = $('#graphhost'); host.hidden = S.centre !== 'graph'; if (host.hidden) return;
  const d = doc(), nodes = d.nodes, has = new Set(nodes.map(n => n.name));
  const cols = {}; nodes.forEach(n => (cols[n.depth] ||= []).push(n)); const pos = {};
  Object.entries(cols).forEach(([c, ns]) => ns.forEach((n, i) => pos[n.name] = [c * 190, i * 84 + (c % 2) * 18]));
  const W = Math.max(...Object.values(pos).map(p => p[0])) + 170, H = Math.max(...Object.values(pos).map(p => p[1])) + 80;
  const wires = nodes.flatMap(n => n.reads.filter(r => has.has(r)).map(r => { const [x0, y0] = pos[r], [x1, y1] = pos[n.name]; const a = [x0 + 150, y0 + 28], b = [x1, y1 + 28]; return `<path d="M${a[0]},${a[1]} C${a[0] + 40},${a[1]} ${b[0] - 40},${b[1]} ${b[0]},${b[1]}" stroke="${S.sel === n.name || S.sel === r ? '#6fa8ff' : '#3a404c'}" stroke-width="1.5" fill="none"/>`; })).join('');
  const hotNodes = S.hot ? macroTargets(S.hot) : [];
  host.innerHTML = `<div class="graph" style="width:${W}px;height:${H}px"><svg class="wires">${wires}</svg>${nodes.map(n => `<div class="gnode ${n.type === 'output' ? 'out' : ''} ${S.sel === n.name ? 'sel' : ''} ${ST.solo === n.name ? 'solo' : ''} ${hotNodes.includes(n.name) ? 'mac' : ''}" data-node="${esc(n.name)}" style="left:${pos[n.name][0]}px;top:${pos[n.name][1]}px"><div class="n">${ic(ICON[n.type] || 'wave')}${esc(n.name)}</div><button class="gsolo ${ST.solo === n.name ? 'on' : ''}" data-solo="${esc(n.name)}" aria-label="Solo ${esc(n.name)}" title="Hear this node">${ic('phones')}</button><div class="gbadges"><span class="t">${esc(n.type)}</span>${n.open_comments ? `<span class="pinc" title="open comments on this node">${n.open_comments}</span>` : ''}${n.unread ? `<span class="warnc" title="nothing reads this node">${ic('warn')}</span>` : ''}</div><canvas data-gspark="${esc(n.name)}"></canvas></div>`).join('')}</div>`;
  host.querySelectorAll('[data-gspark]').forEach(c => { const name = c.dataset.gspark, doc0 = ST.doc; getSpark(name).then(x => { if (c.isConnected && ST.doc === doc0) drawSpark(c, x); }).catch(() => {}); });
}

// ---------- macro knobs ----------
function polar(cx, cy, r, deg) { const a = deg * Math.PI / 180; return [cx + r * Math.cos(a), cy + r * Math.sin(a)]; }
function arc(cx, cy, r, a0, a1) { const [x0, y0] = polar(cx, cy, r, a0), [x1, y1] = polar(cx, cy, r, a1); return `M${x0.toFixed(2)},${y0.toFixed(2)} A${r},${r} 0 ${a1 - a0 > 180 ? 1 : 0} 1 ${x1.toFixed(2)},${y1.toFixed(2)}`; }
function dial(m) {
  const A0 = 135, A = v => A0 + 270 * v, [px, py] = polar(33, 31, 17, A(m.value)), [t0x, t0y] = polar(33, 31, 25, A(m.default)), [t1x, t1y] = polar(33, 31, 29, A(m.default));
  return `<path d="${arc(33, 31, 25, A0, A0 + 270)}" stroke="#2e333e" stroke-width="4" fill="none" stroke-linecap="round"/>` +
    `<path d="${arc(33, 31, 25, A0, Math.max(A0 + 0.5, A(m.value)))}" stroke="${m.game ? 'var(--a3)' : 'var(--accent)'}" stroke-width="4" fill="none" stroke-linecap="round"/>` +
    `<line x1="${t0x}" y1="${t0y}" x2="${t1x}" y2="${t1y}" stroke="#8a91a0" stroke-width="2" stroke-linecap="round"><title>agent default ${m.default.toFixed(2)}</title></line>` +
    `<circle cx="33" cy="31" r="17" fill="#252932" stroke="#3a404c"/><line x1="33" y1="31" x2="${px}" y2="${py}" stroke="#e3e6ec" stroke-width="2.2" stroke-linecap="round"/>`;
}
function knobHTML(m) {
  return `<div class="knob ${S.hot === m.name ? 'hot' : ''}" data-knob="${esc(m.name)}"><div class="kn">${esc(m.name)}${m.game ? '<span class="game" title="exported: the game sets this at runtime">GAME</span>' : ''}</div>
    <svg class="dial" viewBox="0 0 66 60" tabindex="0" role="slider" aria-label="${esc(m.name)}" aria-valuemin="0" aria-valuemax="1" aria-valuenow="${m.value.toFixed(2)}" data-dial="${esc(m.name)}">${dial(m)}</svg>
    <div class="kval"><span data-kv="${esc(m.name)}">${m.value.toFixed(2)}</span></div></div>`;
}
function renderDock() {
  const d = doc(), dk = $('#dock'); dk.hidden = S.view === 'grid'; if (KD) return;
  const moved = d.macros.filter(m => Math.abs(m.value - m.default) > 0.005).length;
  dk.innerHTML = `<div class="dockh"><span class="t">Macros</span><span class="grow"></span>${moved ? `<button class="tbtn" data-kreset title="Back to the agent's settings">Reset</button>` : ''}</div>
    <div class="knobs">${d.macros.map(knobHTML).join('')}</div>`;
}
let KD = null, KT = null; const KS = {};
const macroOf = name => doc().macros.find(x => x.name === name);
// Dial and value follow the pointer locally; move_knob goes out throttled (~15 Hz) and on release.
function setMacro(name, v) {
  const m = macroOf(name); m.value = Math.max(0, Math.min(1, v));
  const svg = document.querySelector(`[data-dial="${name}"]`); if (svg) { svg.innerHTML = dial(m); svg.setAttribute('aria-valuenow', m.value.toFixed(2)); }
  const kv = document.querySelector(`[data-kv="${name}"]`); if (kv) kv.textContent = m.value.toFixed(2);
  if (!KT) KT = setTimeout(() => { KT = null; sendKnob(name); }, 66);
}
async function sendKnob(name) {
  const k = KS[name] || (KS[name] = {}); if (k.run) { k.dirty = true; return k.run; }
  k.run = (async () => {
    try { do { k.dirty = false; const m = macroOf(name); if (!m) break; mergeSound(await sounder.call('move_knob', { name, value: m.value })); PLOTBUST++; renderHud(); if (S.tab === 'sound') renderRight(); refreshPlot(); } while (k.dirty); }
    catch (e) { toast(errText(e)); } finally { k.run = null; }
  })();
  return k.run;
}
// merge a move_knob reply (macros, moves, seconds, peak_db, lufs) into state.sound
function mergeSound(r) {
  const d = doc(); if (!r) return;
  for (const k of ['seconds', 'peak_db', 'lufs', 'moves']) if (r[k] !== undefined) d[k] = r[k];
  if (r.macros) d.macros = r.macros.map(m => { const old = d.macros.find(x => x.name === m.name); return KD && KD.name === m.name && old ? { ...m, value: old.value } : m; });
}
async function finishMacro(name, from) {
  clearTimeout(KT); KT = null; await sendKnob(name); const m = macroOf(name);
  if (m && Math.abs(m.value - from) > 0.005) toast(`${name} ${from.toFixed(2)} → ${m.value.toFixed(2)} · agents see it in summary`);
  renderDock(); renderRight(); renderGraph(); schedulePull(10);
}
function claimText(m) {
  if (!m.claim) return '';
  const L = CLAIMS[m.claim[0]] || m.claim[0], arrow = m.claim[1] > 0 ? '↑' : '↓', r = m.result;
  if (!r) return `<span>${L} ${arrow} · checking…</span>`;
  const i = r.detail.lastIndexOf(': '), vals = i >= 0 ? r.detail.slice(i + 2) : r.detail;
  return r.pass ? `<span class="ok">✓</span> ${L} ${arrow}<br><b>${esc(vals).replace(' -&gt; ', '</b> → <b>')}</b>` : `<span class="bad">✗</span> ${esc(r.detail)}`;
}
function mapText(m) {
  const f = x => Math.abs(x) >= 100 ? fmtHz(x) : +x.toFixed(m.col != null ? 2 : 3);
  return [`${m.node}.${m.field}${m.col != null ? (m.col ? ' decays' : ' pitches') + ' ×' : ''}${m.index != null ? `[${m.index}]${m.key ? '.' + m.key : ''}` : ''}`, `${f(m.from)} → ${f(m.to)}${m.curve === 'exp' ? ' exp' : ''}`];
}
function showKpop(name, el) {
  const m = macroOf(name); if (!m) return; const p = $('#kpop'), r = el.getBoundingClientRect();
  p.innerHTML = `<div class="d">${esc(m.doc)}</div><div class="m">${m.map.map(x => mapText(x).map(t => `<span>${esc(t)}</span>`).join('')).join('')}</div><div class="who">${claimText(m).replace('<br>', ' · ')}</div><div class="who">${m.game ? 'Exported to the game as a runtime input. ' : ''}Agent default ${m.default.toFixed(2)} · double-click resets.</div>`;
  p.hidden = false; p.style.left = Math.max(8, Math.min(innerWidth - 310, r.left)) + 'px'; p.style.top = (r.top - p.offsetHeight - 8) + 'px';
}

// ---------- right pane ----------
function slider(key) {
  const lg = /freq|from|to$|^to|root|hz/.test(key), t = /time|decay|attack|release|duration/.test(key);
  if (lg) return { min: Math.log(20), max: Math.log(20000), step: 0.001, to: Math.exp, from: Math.log, fmt: x => fmtHz(x) + ' Hz' };
  if (t) return { min: Math.log(0.001), max: Math.log(4), step: 0.001, to: Math.exp, from: Math.log, fmt: x => x < 0.1 ? (x * 1000).toFixed(0) + ' ms' : x.toFixed(2) + ' s' };
  if (key === 'q') return { min: 0.3, max: 20, step: 0.05, fmt: x => x.toFixed(2) };
  if (/db|lufs/.test(key)) return { min: key === 'lufs' ? -40 : -24, max: key === 'lufs' ? -6 : 24, step: 0.5, fmt: x => x.toFixed(1) + (key === 'lufs' ? '' : ' dB') };
  if (key === 'b' || key === 'gain' || key === 'drive') return { min: 0, max: 8, step: 0.05, fmt: x => x.toFixed(2) };
  return { min: 0, max: 1, step: 0.01, fmt: x => x.toFixed(2) };
}
function fieldRow(n, k, v) {
  const d = doc(), names = new Set(d.nodes.map(x => x.name));
  const by = d.macros.filter(m => m.map.some(x => x.node === n.name && x.field === k));
  if (by.length) { const ev = n.effective && k in n.effective ? n.effective[k] : v; const show = typeof ev === 'number' ? slider(k).fmt(ev) : JSON.stringify(ev);
    return `<div class="field"><label>${esc(k)}</label><div class="link">${by.map(m => `<span class="macchip" data-kfocus="${esc(m.name)}" title="Turn the ${esc(m.name)} knob">◎ ${esc(m.name)}</span>`).join('')}<span class="val grow" style="text-align:right" title="${esc(JSON.stringify(ev))}">${esc(show)}</span></div></div>`; }
  if (ENUMS[k] && typeof v === 'string' && ENUMS[k].includes(v)) return `<div class="field"><label>${esc(k)}</label><div class="opts">${ENUMS[k].map(o => `<button data-enum="${k}" data-v="${o}" aria-pressed="${o === v}">${o}</button>`).join('')}</div></div>`;
  if (typeof v === 'string' && names.has(v.split('.')[0])) return `<div class="field"><label>${esc(k)}</label><div class="link"><button data-goto="${esc(v)}" title="Select ${esc(v)}">← ${esc(v)}</button><span class="hint">signal</span></div></div>`;
  if (typeof v === 'string' && /^[$=]/.test(v)) return `<div class="field"><label>${esc(k)}</label><div class="link"><span class="mono" style="color:var(--a3)">${esc(v)}</span></div></div>`;
  if (typeof v === 'number') { const s = slider(k); const pos = s.from ? s.from(Math.max(v, 1e-6)) : v; return `<div class="field"><label>${esc(k)}</label><input type="range" data-num="${esc(k)}" min="${s.min}" max="${s.max}" step="${s.step}" value="${pos}"><span class="val" data-numv="${esc(k)}">${s.fmt(v)}</span></div>`; }
  return `<div class="field"><label>${esc(k)}</label><pre>${esc(JSON.stringify(v))}</pre></div>`;
}
function renderRight() {
  if (dragRight) return;
  const d = doc(), r = $('#right'); const n = nodeOf(S.sel); const open = d.comments.filter(c => !c.resolved).length;
  const tabs = `<div class="tabs" role="tablist">${[['node', 'Node'], ['sound', 'Sound'], ['comments', 'Comments']].map(([k, l]) => `<button role="tab" data-tab="${k}" aria-selected="${S.tab === k}">${l}${k === 'comments' && open ? `<span class="n">${open}</span>` : ''}</button>`).join('')}</div>`;
  let body = '';
  if (S.tab === 'node') {
    if (!n) body = `<div class="sect hint">Select a node in the graph.</div>`;
    else { const readBy = d.nodes.filter(x => x.reads.includes(n.name)).map(x => x.name); const link = x => `<a href="#" data-goto="${esc(x)}" style="color:var(--accent)">${esc(x)}</a>`;
      body = `<div class="sect"><div class="nodehead">${ic(ICON[n.type] || 'wave')}<span class="nm">${esc(n.name)}</span><span class="badge">${esc(n.type)}</span><span class="grow"></span><button class="tbtn" data-solo="${esc(n.name)}" aria-pressed="${ST.solo === n.name}" title="Hear this node">${ic('phones')}Solo</button></div><div class="blurb">${esc(BLURB[n.type] || '')}</div></div>
      <div class="sect"><div class="sh">Fields</div>${Object.entries(n.fields).map(([k, v]) => fieldRow(n, k, v)).join('')}</div>
      <div class="sect"><div class="sh">Wiring</div><dl class="kv"><dt>reads</dt><dd>${n.reads.filter(x => d.nodes.some(y => y.name === x)).map(link).join(', ') || '—'}</dd><dt>read by</dt><dd>${readBy.map(link).join(', ') || (n.type === 'output' ? 'the sound' : '<span style="color:var(--warn)">nothing</span>')}</dd><dt>peak</dt><dd data-nodepeak="${esc(n.name)}">…</dd></dl></div>
      <div class="sect"><div class="sh">JSON <span class="hint" style="text-transform:none;letter-spacing:0">what sound_ops sends</span></div><pre class="mono" style="margin:0;white-space:pre-wrap;color:var(--dim);font-size:11px">${esc(JSON.stringify({ type: n.type, name: n.name, ...n.fields }))}</pre></div>`;
      const name = n.name, doc0 = ST.doc; getSpark(name).then(x => { const el = document.querySelector(`[data-nodepeak="${name}"]`); if (el && ST.doc === doc0) { let pk = 0; for (const v of x) pk = Math.max(pk, Math.abs(v)); el.textContent = `${(20 * Math.log10(Math.max(pk, 1e-10))).toFixed(1)} dBFS`; } }).catch(() => {}); }
  } else if (S.tab === 'sound') {
    const ds = d.descriptors || {}, bmax = -6, bmin = -72;
    const cell = v => { const t = Math.max(0, Math.min(1, (v - bmin) / (bmax - bmin))); const k = Math.round((t * 0.92 + 0.04) * 255) * 4; return `<span style="background:rgb(${CMAP[k]},${CMAP[k + 1]},${CMAP[k + 2]})" title="${v.toFixed(1)} dB">${v < -99 ? '' : Math.round(v)}</span>`; };
    const checks = (d.checks || []).map(c => [c.name, c.pass, esc(c.detail).replace(/^(-?[\d.]+)/, '<b>$1</b>')]);
    if (!checks.some(c => c[0].startsWith('macro'))) d.macros.forEach(m => { if (m.result) checks.push([`macro ${m.name}`, m.result.pass, esc(m.result.detail)]); });
    const bands = ds.bands || [], by = (ST.sounds.find(s => s.name === ST.doc) || {}).by || '';
    const params = Object.entries(d.params || {});
    body = `<div class="sect"><div class="sh">Sound</div><dl class="kv"><dt>kind</dt><dd>${esc(d.kind)}</dd><dt>duration</dt><dd>${d.duration ? d.duration + ' s' : 'auto (until -80 dBFS)'}</dd><dt>seed</dt><dd>${d.seed}</dd><dt>sample rate</dt><dd>${d.sample_rate}</dd><dt>causal</dt><dd>${d.causal ? 'yes' : 'no'}</dd></dl></div>
      ${d.macros.length ? `<div class="sect"><div class="sh">Macros <span class="hint" style="text-transform:none;letter-spacing:0">by ${esc(by)}</span></div>${d.macros.map(m => `<div style="margin:0 0 9px"><div style="display:flex;gap:6px;align-items:center"><span class="macchip" data-kfocus="${esc(m.name)}">◎ ${esc(m.name)}</span>${m.game ? '<span class="game">GAME</span>' : ''}<span class="grow"></span><span class="mono">${m.value.toFixed(2)}</span><span class="hint">default ${m.default.toFixed(2)}</span></div><div class="hint" style="margin:3px 0">${esc(m.doc)}</div><div class="fb">${m.map.map(x => mapText(x).map(t => `<span class="mono" style="font-size:11px">${esc(t)}</span>`).join('')).join('')}</div></div>`).join('')}</div>` : ''}
      ${(d.moves || []).length ? `<div class="sect"><div class="sh">Your moves <span class="hint" style="text-transform:none;letter-spacing:0">agents see these in summary</span></div><div class="fb">${d.moves.map(f => `<span>${esc(f.name)} <span class="mono">${f.from.toFixed(2)} → ${f.to.toFixed(2)}</span></span><span>now</span>`).join('')}</div><div class="hint" style="margin-top:6px">An agent can keep them as the new defaults, or re-centre a knob's range around where you left it.</div></div>` : ''}
      ${params.length ? `<div class="sect"><div class="sh">Params</div>${params.map(([k, v]) => `<div class="field"><label>$${esc(k)}</label><span class="val grow mono" style="grid-column:2/4;text-align:left">${esc(JSON.stringify(v))}</span></div>`).join('')}</div>` : ''}
      <div class="sect"><div class="sh">Checks <span class="badge ${checks.every(c => c[1]) ? 'ok' : 'bad'}">${checks.filter(c => c[1]).length}/${checks.length}</span></div>${checks.map(([k, p, t]) => `<div class="chk"><span class="p ${p ? 'ok' : 'bad'}">${p ? 'PASS' : 'FAIL'}</span><span class="d">${esc(k)}: ${t}</span></div>`).join('')}${(d.warnings || []).map(w => `<div class="wrn">${ic('warn')}${esc(w)}</div>`).join('')}</div>
      ${bands.length ? `<div class="sect"><div class="sh">Bands (dB) × time</div><div class="bt"><span class="lab"></span>${['0', '20', '40', '60', '80%'].map(h => `<span class="hd">${h}</span>`).join('')}${bands.map((row, b) => `<span class="lab">${BLAB[b] || ''}</span>${row.map(cell).join('')}`).reverse().join('')}</div></div>` : ''}
      <div class="sect"><div class="sh">Descriptors</div><dl class="kv"><dt>length</dt><dd>${d.seconds.toFixed(3)} s</dd><dt>peak</dt><dd>${d.peak_db.toFixed(1)} dBFS</dd><dt>loudness</dt><dd>${d.lufs.toFixed(1)} LUFS</dd>${ds.rms_db != null ? `<dt>RMS</dt><dd>${ds.rms_db.toFixed(1)} dBFS</dd>` : ''}${ds.centroid_hz != null ? `<dt>centroid</dt><dd>${fmtHz(sme(ds.centroid_hz))} Hz</dd>` : ''}</dl><div class="hint" style="margin-top:6px">The full table (onsets, pitch, modes…) is what listen returns to agents.</div></div>`;
  } else {
    body = d.comments.length ? d.comments.map(c => { const rep = c.reply && (typeof c.reply === 'string' ? { by: 'agent', text: c.reply } : c.reply);
      return `<div class="cmt ${c.resolved ? 'done' : ''}" data-cmt="${c.id}"><span class="num">${c.id}</span><div><div class="hd"><b>${esc(c.by)}</b>${c.ago ? `<span>${esc(c.ago)}</span>` : ''}<span class="on">${esc(pinText(onOf(c)))}</span>${c.resolved ? '<span style="color:var(--ok)">resolved</span>' : ''}</div>${esc(c.text)}${rep ? `<div class="reply"><div class="hd"><b>${esc(rep.by || 'agent')}</b><span>agent</span></div>${esc(rep.text)}</div>` : ''}</div></div>`; }).join('') : `<div class="sect hint">No comments on ${esc(ST.doc)}. Drag a region on the spectrogram or pick a node, then write what you hear. Agents see open comments in summary.</div>`;
    const pin = S.pendingPin || (S.sel ? { node: S.sel } : {});
    body += `<div class="grow"></div><div class="compose"><div class="pinto">${ic('pin')} on <span class="on mono" style="border:1px solid var(--line);border-radius:4px;padding:0 5px">${esc(pinText(pin))}</span>${S.pendingPin ? '<button class="hint" data-unpin>(whole sound)</button>' : ''}</div><div class="row2"><input id="cin" placeholder="What do you hear?" autocomplete="off"><button class="primary" id="csend">Post</button></div></div>`;
  }
  const keep = $('#cin') ? $('#cin').value : null;
  r.innerHTML = tabs + body; if (keep && $('#cin')) $('#cin').value = keep;
}

// ---------- grid of all sounds ----------
const THUMBS = new Map();
function renderOverview() {
  $('#overview').hidden = S.view !== 'grid'; if (S.view !== 'grid') return;
  const sounds = ST.sounds;
  $('#ovgrid').innerHTML = `<div class="ovsec">${esc(ST.project || 'friendSLOP')} · ${sounds.length} open</div>` + sounds.map(d => `<div class="tile" data-doc="${esc(d.name)}" tabindex="0"><div class="thumb"><canvas data-th="${esc(d.name)}"></canvas>${d.working ? `<span class="chip working" style="font-size:11px"><i class="dot live" style="background:${agentColor(d.by)}"></i>${esc(d.by)} editing</span>` : ''}<button class="tp" data-audition="${esc(d.name)}" aria-label="Play ${esc(d.name)}">${ic('play')}</button></div><div class="info"><span class="n">${esc(d.name)}</span><span class="badge ${d.kind === 'loop' ? 'loop' : ''}">${esc(d.kind)}</span></div><div class="sub"><span class="grow">${d.seconds != null ? `${d.seconds.toFixed(2)} s${d.lufs != null ? ` · ${d.lufs.toFixed(1)} LUFS` : ''}` : ''}</span><span>${esc(d.by)} · ${esc(d.ago)}${d.saved ? '' : ' · <span style="color:var(--warn)">unsaved</span>'}</span></div></div>`).join('');
  $('#feed').innerHTML = (ST.feed || []).map(f => { const i = f.what.indexOf(' · '); const what = i > 0 ? `<b>${esc(f.what.slice(0, i))}</b>${esc(f.what.slice(i))}` : esc(f.what);
    return `<div class="ev" data-doc="${esc(f.doc)}"><i class="dot" style="background:${agentColor(f.agent)}"></i><span class="what"><span class="mono">${esc(f.tool)}</span> ${what}</span><time>${esc(f.ago)}</time></div>`; }).join('');
  requestAnimationFrame(() => document.querySelectorAll('[data-th]').forEach(async c => {
    const name = c.dataset.th, W = c.width = Math.max(8, Math.round(c.clientWidth * DPR)), H = c.height = Math.max(8, Math.round(c.clientHeight * DPR)); if (!CMAP) return;
    const key = `${name}|${PLOTREV}|${PLOTBUST}|${W}x${H}`;
    if (!THUMBS.has(key)) { if (THUMBS.size > 60) THUMBS.clear(); THUMBS.set(key, bulk(`render/${name}/spectrogram`, { rev: PLOTREV, b: PLOTBUST, w: W, h: H }).then(b => specCanvas(parseSpec(b)))); }
    try { const sc = await THUMBS.get(key); if (c.isConnected) c.getContext('2d').drawImage(sc, 0, 0, W, H); } catch (e) { THUMBS.delete(key); }
  }));
}

// ---------- toast, render all ----------
function toast(t) { const e = $('#toast'); e.innerHTML = `<span class="chip"><b>${esc(t)}</b></span>`; e.hidden = false; clearTimeout(toast.h); toast.h = setTimeout(() => e.hidden = true, 2600); }
function renderCentre() { renderHud(); renderGraph(); requestAnimationFrame(() => { renderPlot(); showRegbar(); refreshPlot(); }); }
function renderAll() { renderTop(); renderOverview(); renderCentre(); renderDock(); renderRight(); }

// =====================================================================
// Wiring (every action is a call; the page keeps no copy of the sound)
// =====================================================================
$('#gridbtn').onclick = () => { if (P.playing) act('stop', {}); S.view = 'grid'; renderAll(); };
$('#docbtn').onclick = () => { if (S.view === 'grid') { S.view = 'doc'; renderAll(); return; } const p = $('#docpop'); p.hidden = !p.hidden; if (!p.hidden) { renderDocList(); $('#docq').focus(); loadCatalog(); } };
let CATALOG = [];
async function loadCatalog() { try { const r = await sounder.call('catalog_list', {}); CATALOG = (Array.isArray(r) ? r : (r && r.sounds) || []).map(x => typeof x === 'string' ? { name: x } : x); renderDocList(); } catch (e) { /* the saved list is optional */ } }
function renderDocList() {
  const q = $('#docq').value.toLowerCase();
  const open = ST.sounds.filter(d => d.name.includes(q)).map(d => {
    return `<div class="docrow ${d.name === ST.doc ? 'on' : ''}" data-doc="${esc(d.name)}">${ic(d.kind === 'loop' ? 'loop' : 'wave')}<span style="display:flex;gap:6px;align-items:center">${esc(d.name)}${d.open_comments ? `<span class="pinc" style="font:600 10px var(--sans);background:var(--warn);color:#1b1600;border-radius:99px;padding:0 5px" title="${d.open_comments} open comments">${d.open_comments}</span>` : ''}${d.working ? `<i class="dot live" style="background:${agentColor(d.by)}" title="${esc(d.by)} is editing"></i>` : ''}${d.saved ? '' : '<span style="color:var(--warn)" title="unsaved">●</span>'}</span><span class="r">${esc(d.by)} · ${esc(d.ago)}</span></div>`; }).join('');
  const openNames = new Set(ST.sounds.map(d => d.name));
  const saved = CATALOG.filter(c => !openNames.has(c.name) && c.name.includes(q));
  $('#doclist').innerHTML = open + (saved.length ? `<div class="h" style="font-size:11px;letter-spacing:.06em;text-transform:uppercase;color:var(--faint);padding:8px 8px 4px">Saved</div>` + saved.map(c => `<div class="docrow" data-load="${esc(c.name)}">${ic(c.kind === 'loop' ? 'loop' : 'wave')}<span>${esc(c.name)}</span><span class="r">open</span></div>`).join('') : '');
}
$('#docq').oninput = renderDocList;
async function openSound(name) { S.view = 'doc'; try { await sounder.call('select_sound', { doc: name }); } catch (e) { toast(errText(e)); } pullState(name); }
$('#doclist').onclick = async e => {
  const l = e.target.closest('[data-load]'); if (l) { $('#docpop').hidden = true; try { await sounder.call('catalog_load', { name: l.dataset.load }); await openSound(l.dataset.load); } catch (er) { toast(errText(er)); } return; }
  const r = e.target.closest('[data-doc]'); if (r) { $('#docpop').hidden = true; openSound(r.dataset.doc); } };
$('#playbtn').onclick = toggle;
$('#loopbtn').onclick = () => { if (doc().kind === 'loop') return toast('A loop sound always loops'); S.loop = !S.loop; renderTop(); restartKeeping(); };
const setSeed = s => act('set_seed', { seed: Math.max(1, s) });
$('#seeddn').onclick = () => setSeed(doc().seed - 1); $('#seedup').onclick = () => setSeed(doc().seed + 1); $('#seedroll').onclick = () => setSeed(1 + Math.floor(Math.random() * 999));
function openHist(anchor) { const d = doc(), p = $('#bpop'); const r = anchor.getBoundingClientRect(); p.style.left = Math.max(8, Math.min(innerWidth - 330, r.right - 320)) + 'px';
  p.innerHTML = `<div class="h">History · click to jump (undo / redo)</div>${[...d.history].reverse().map(h => `<div class="hstep" data-hid="${h.id}"><span class="mono" style="color:var(--faint)">v${h.version}</span><span class="l" title="${esc(h.label)}">${esc(h.label)}</span><span class="r" style="display:flex;gap:5px;align-items:center;color:var(--faint);font-size:11px">${h.current ? '<span class="tagA">now</span>' : ''}<i class="dot" style="background:${agentColor(h.by)}"></i>${esc(h.by)}</span></div>`).join('')}`; p.hidden = false; }
$('#bpop').onclick = async e => { const h = e.target.closest('[data-hid]'); if (!h) return; const step = doc().history.find(x => x.id == h.dataset.hid); $('#bpop').hidden = true; if (!step || step.current) return; const r = await act('jump', { id: step.id }); if (r !== undefined || true) toast(`Jumped to v${step.version}`); };
const solo = node => act('solo', { node });
$('#hearing').onclick = e => { if (e.target.closest('[data-unsolo]')) solo(null); };
$('#hudbar').onclick = e => { const c = e.target.closest('[data-centre]'); if (c) { S.centre = c.dataset.centre; renderCentre(); return; }
  if (e.target.closest('[data-unzoom]')) { S.zoom = null; renderCentre(); return; } const hb = e.target.closest('[data-hist]'); if (hb) openHist(hb); };
function toggleSolo(node) { solo(ST.solo === node || isOut(node) ? null : node); }
$('#graphhost').onclick = e => {
  const so = e.target.closest('[data-solo]'); if (so) { e.stopPropagation(); toggleSolo(so.dataset.solo); return; }
  const n = e.target.closest('[data-node]'); if (n) { S.sel = n.dataset.node; S.tab = 'node'; renderGraph(); renderRight(); } };

// knobs
$('#dock').addEventListener('pointerdown', e => { const k = e.target.closest('[data-dial]'); if (!k) return; e.preventDefault(); k.setPointerCapture(e.pointerId); const m = macroOf(k.dataset.dial); KD = { name: m.name, y: e.clientY, v: m.value, from: m.value }; $('#kpop').hidden = true; });
$('#dock').addEventListener('pointermove', e => { if (!KD) return; setMacro(KD.name, KD.v + (KD.y - e.clientY) / (e.shiftKey ? 600 : 150)); });
$('#dock').addEventListener('pointerup', () => { if (!KD) return; const k = KD; KD = null; finishMacro(k.name, k.from); });
$('#dock').addEventListener('dblclick', e => { const k = e.target.closest('[data-dial]'); if (!k) return; const m = macroOf(k.dataset.dial); const from = m.value; setMacro(m.name, m.default); finishMacro(m.name, from); });
$('#dock').addEventListener('wheel', e => { const k = e.target.closest('[data-dial]'); if (!k) return; e.preventDefault(); const m = macroOf(k.dataset.dial); if (!KD) KD = { name: m.name, from: m.value, wheel: true }; setMacro(m.name, m.value - e.deltaY * 0.0015); clearTimeout(KD.t); KD.t = setTimeout(() => { const kk = KD; KD = null; finishMacro(kk.name, kk.from); }, 350); }, { passive: false });
$('#dock').addEventListener('keydown', e => { const k = e.target.closest('[data-dial]'); if (!k || !['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(e.key)) return; e.preventDefault(); e.stopPropagation(); const m = macroOf(k.dataset.dial); const from = m.value; setMacro(m.name, m.value + (/Up|Right/.test(e.key) ? 1 : -1) * (e.shiftKey ? 0.01 : 0.05)); finishMacro(m.name, from).then(() => requestAnimationFrame(() => document.querySelector(`[data-dial="${m.name}"]`)?.focus())); });
$('#dock').addEventListener('mouseover', e => { const k = e.target.closest('[data-knob]'); if (!k || KD) return; if (S.hot !== k.dataset.knob) { S.hot = k.dataset.knob; document.querySelectorAll('.knob').forEach(x => x.classList.toggle('hot', x.dataset.knob === S.hot)); renderGraph(); } showKpop(k.dataset.knob, k); });
$('#dock').addEventListener('mouseleave', () => { if (KD) return; S.hot = null; $('#kpop').hidden = true; document.querySelectorAll('.knob').forEach(x => x.classList.remove('hot')); renderGraph(); });
$('#dock').onclick = async e => { if (e.target.closest('[data-kreset]')) { await act('reset_knobs', {}); toast('Knobs back at the agent\'s defaults'); } };

// right pane
$('#right').onclick = e => {
  const t = e.target.closest('[data-tab]'); if (t) { S.tab = t.dataset.tab; renderRight(); return; }
  const so = e.target.closest('[data-solo]'); if (so) { toggleSolo(so.dataset.solo); return; }
  const kf = e.target.closest('[data-kfocus]'); if (kf) { const el = document.querySelector(`[data-dial="${kf.dataset.kfocus}"]`); if (el) { el.focus(); el.closest('.knob').animate([{ boxShadow: '0 0 0 4px rgba(111,168,255,.5)' }, { boxShadow: '0 0 0 0 rgba(111,168,255,0)' }], 900); } return; }
  const go = e.target.closest('[data-goto]'); if (go) { e.preventDefault(); S.sel = go.dataset.goto.split('.')[0]; renderRight(); renderGraph(); return; }
  const en = e.target.closest('[data-enum]'); if (en) { act('set_field', { node: S.sel, field: en.dataset.enum, value: en.dataset.v }); return; }
  const cm = e.target.closest('[data-cmt]'); if (cm) { const c = doc().comments.find(x => x.id == cm.dataset.cmt); if (c.node) S.sel = c.node; if (c.time) { S.region = { t: c.time, hz: c.hz || undefined }; showRegbar(); } return; }
  if (e.target.closest('[data-unpin]')) { S.pendingPin = {}; renderRight(); return; }
  if (e.target.id === 'csend') postComment();
};
$('#right').onkeydown = e => { if (e.target.id === 'cin' && e.key === 'Enter') postComment(); };
async function postComment() {
  const v = $('#cin').value.trim(); if (!v) return; const on = S.pendingPin || (S.sel ? { node: S.sel } : {}); const a = { text: v };
  if (on.node) a.node = on.node; if (on.t) a.time = on.t; if (on.hz) a.hz = on.hz;
  S.pendingPin = null; S.region = null; $('#cin').value = '';
  const r = await act('add_annotation', a); if (r !== undefined) toast('Posted: agents see it in summary');
}
// Node-tab sliders: label follows the pointer, set_field goes out on release.
$('#right').addEventListener('pointerdown', e => { if (e.target.dataset && e.target.dataset.num) dragRight = true; });
addEventListener('pointerup', () => { if (dragRight) setTimeout(() => { dragRight = false; renderRight(); }, 0); });
$('#right').oninput = e => {
  const k = e.target.dataset.num; if (!k) return; const s = slider(k); const v = s.to ? s.to(+e.target.value) : +e.target.value; const r3 = Math.abs(v) >= 100 ? Math.round(v) : +v.toFixed(4); $(`[data-numv="${k}"]`).textContent = s.fmt(r3);
};
$('#right').onchange = e => {
  const k = e.target.dataset.num; if (!k) return; const s = slider(k); const v = s.to ? s.to(+e.target.value) : +e.target.value; const r3 = Math.abs(v) >= 100 ? Math.round(v) : +v.toFixed(4);
  dragRight = false; act('set_field', { node: S.sel, field: k, value: r3 });
};

// grid of sounds
$('#ovgrid').onclick = async e => {
  const a = e.target.closest('[data-audition]'); if (a) { e.stopPropagation(); try { await sounder.call('select_sound', { doc: a.dataset.audition }); await sounder.call('play', { from: 0 }); } catch (er) { toast(errText(er)); } pullState(); return; }
  const t = e.target.closest('[data-doc]'); if (t) openSound(t.dataset.doc); };
$('#feed').onclick = e => { const r = e.target.closest('[data-doc]'); if (r) openSound(r.dataset.doc); };
$('#pins').onclick = e => { const p = e.target.closest('[data-cmt]'); if (p) { S.tab = 'comments'; renderRight(); } };
$('#regbar').onclick = e => { const b = e.target.closest('[data-rg]'); if (!b) return; const a = b.dataset.rg, r = S.region;
  if (a === 'play') act('play', { from: r.t[0], span: [r.t[0], r.t[1]], loop: false });
  if (a === 'loop') { S.loop = true; act('play', { from: r.t[0], span: [r.t[0], r.t[1]], loop: true }); }
  if (a === 'zoom') { S.zoom = { t: r.t, hz: r.hz }; S.region = null; renderCentre(); }
  if (a === 'comment') { S.pendingPin = { t: r.t.map(x => +x.toFixed(3)), hz: r.hz && r.hz.map(Math.round) }; S.tab = 'comments'; renderRight(); setTimeout(() => $('#cin') && $('#cin').focus(), 0); }
  if (a === 'clear') { S.region = null; if (S.loop) { S.loop = false; restartKeeping(); } }
  renderTop(); showRegbar(); };

// region drag on the plot; a plain click seeks
$('#ov').onmousedown = e => { if (S.centre !== 'sound') return; const g = geom(), r = e.target.getBoundingClientRect(); const x = e.clientX - r.left, y = e.clientY - r.top; if (x < G.gut) return;
  DRAG = { x0: x, y0: y, spec: y >= g.specT && y <= g.specT + g.specH, moved: false, r: null }; };
addEventListener('mousemove', e => { if (!DRAG) return; const g = geom(), sp = span(), r = $('#ov').getBoundingClientRect(); const x = Math.max(G.gut, Math.min(G.gut + g.pw, e.clientX - r.left)), y = Math.max(g.specT, Math.min(g.specT + g.specH, e.clientY - r.top));
  if (Math.abs(x - DRAG.x0) + Math.abs(y - DRAG.y0) > 4) DRAG.moved = true; if (!DRAG.moved) return;
  const t = [tOf(Math.min(x, DRAG.x0), g, sp), tOf(Math.max(x, DRAG.x0), g, sp)].map(v => Math.max(0, v));
  DRAG.r = DRAG.spec ? { t, hz: [hzOf(Math.max(y, DRAG.y0), g, sp), hzOf(Math.min(y, DRAG.y0), g, sp)] } : { t }; $('#regbar').hidden = true; });
addEventListener('mouseup', () => { if (!DRAG) return; const d = DRAG; DRAG = null;
  if (!d.moved) { const g = geom(), sp = span(); const t = Math.max(0, tOf(d.x0, g, sp)); S.region = null; showRegbar(); P.pos = t; P.at = performance.now(); act('seek', { t }); return; }
  S.region = d.r; showRegbar(); renderTop(); });
addEventListener('resize', () => renderCentre());
addEventListener('keydown', e => {
  if (!ST) return;
  if (e.key === 'Escape' && (!$('#docpop').hidden || !$('#bpop').hidden)) { $('#docpop').hidden = true; $('#bpop').hidden = true; e.target.blur?.(); return; }
  if (e.target.closest?.('input, textarea')) return; const k = e.key.toLowerCase();
  if (e.key === ' ') { e.preventDefault(); toggle(); }
  else if (k === 'l') $('#loopbtn').click(); else if (k === 'r') $('#seedroll').click();
  else if (k === '1' || k === '2') { S.centre = k === '1' ? 'sound' : 'graph'; renderCentre(); }
  else if (k === 'h') { const hb = document.querySelector('[data-hist]'); if (hb) openHist(hb); }
  else if (k === 'c') { S.tab = 'comments'; renderRight(); }
  else if (e.key === 'Escape') { $('#docpop').hidden = true; $('#bpop').hidden = true; if (S.region) { S.region = null; showRegbar(); } else if (S.zoom) { S.zoom = null; renderCentre(); } else if (ST.solo) solo(null); }
});
document.addEventListener('mousedown', e => { if (!e.target.closest('#docpop, #docbtn')) $('#docpop').hidden = true; if (!e.target.closest('#bpop, [data-hist]')) $('#bpop').hidden = true; });

// ---------- boot ----------
async function loadFonts() {
  const faces = [['IBM Plex Sans', 400, 'IBMPlexSans-Regular.ttf'], ['IBM Plex Sans', 500, 'IBMPlexSans-Medium.ttf'], ['IBM Plex Sans', 600, 'IBMPlexSans-SemiBold.ttf'], ['IBM Plex Mono', 400, 'IBMPlexMono-Regular.ttf']];
  await Promise.all(faces.map(async ([fam, w, file]) => { try { const f = new FontFace(fam, `url(${FONT_BASE}${file})`, { weight: String(w) }); await f.load(); document.fonts.add(f); } catch (e) { /* falls back to system-ui */ } }));
}
function loadScript(src) { return new Promise((res, rej) => { const s = document.createElement('script'); s.src = src; s.onload = res; s.onerror = () => rej(new Error('could not load ' + src)); document.head.append(s); }); }
async function boot() {
  try {
    if (!IN_WINDOW) await loadScript('fixture.js');   // plain browser: fixture.js answers sounder.call and the bulk GETs
    await loadFonts();
    CMAP = new Uint8Array(await bulk('render/colormap', {}));
    applyState(await sounder.call('state', {}));
    requestAnimationFrame(overlay);
  } catch (e) { document.body.insertAdjacentHTML('beforeend', `<div class="toast"><span class="chip warn">${esc(errText(e))}</span></div>`); }
}
boot();
