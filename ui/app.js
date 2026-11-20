window.__errs = []; addEventListener("error", e => window.__errs.push(String(e.message))); addEventListener("unhandledrejection", e => window.__errs.push("rejection: " + String(e.reason)));
'use strict';
// =====================================================================
// Sounder window page. The Rust Session is the only source of truth: this
// file draws `state` and the bulk data (design/window-api.md) and sends the
// human's actions back as calls. There is no audio or DSP here.
// =====================================================================

// ---------- transport: sounder.call over window.ipc, or the fixture in a plain browser ----------
const IN_WINDOW = !!(window.ipc && window.ipc.postMessage);
// Dev only: `?bridge=http://127.0.0.1:PORT` talks to `sounder --headless --dev-ui PORT` over HTTP, so a plain
// browser can drive this page against the real session (same calls, same bulk data as the webview).
const BRIDGE = IN_WINDOW ? null : new URLSearchParams(location.search).get('bridge');
const FONT_BASE = IN_WINDOW ? 'sounder://fonts/' : BRIDGE ? `${BRIDGE}/fonts/` : 'fonts/';
const sounder = window.sounder = {
  _id: 0, _pending: new Map(),
  call(name, args = {}) {
    if (BRIDGE) return fetch(`${BRIDGE}/call`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ name, args }) })
      .then(r => r.json()).then(r => { if (r && r.ok) return r.result; throw new Error(r && r.error ? r.error : 'the call failed'); });
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
  if (!IN_WINDOW && !BRIDGE) return FIX.bulk(path, q || {});
  const r = await fetch(`${BRIDGE ? BRIDGE + '/' : 'sounder://'}${path}${qs ? '?' + qs : ''}`);
  if (!r.ok) throw new Error(`${path}: ${r.status}`);
  return r.arrayBuffer();
}
const errText = e => ((e && e.message) || String(e)).replace(/`/g, "");

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
  modal: 'modes of a struck object', impact: 'one object hitting the floor or a striker', rig: 'bodies and joints',
};
const ICON = { modal: 'phys', impact: 'phys', rig: 'phys', contact: 'phys', osc: 'wave', noise: 'noise', impulse: 'ring', sample: 'wave', env: 'env', adsr: 'env', decay: 'env', follower: 'env', filter: 'filter', onepole: 'filter', comb: 'filter', allpass: 'filter', resonators: 'ring', ramp: 'ctrl', lfo: 'ctrl', wander: 'ctrl', sample_hold: 'ctrl', slew: 'ctrl', math: 'math', remap: 'math', gain: 'math', mix: 'mix', pan: 'mix', width: 'mix', delay: 'mix', reverb: 'mix', convolve: 'mix', shape: 'filter', output: 'out' };
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
const S = { openRig: null, rigSel: null, multi: null, wireSel: null, mapArm: null, gdrag: null, bdrag: null, view: 'doc', centre: 'sound', sel: null, tab: 'node', loop: false, region: null, zoom: null, hot: null, pendingPin: null };
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
  // No sound is open for the window yet (an agent made one, or none exists): open the first one.
  if (!s.sound) { if (s.sounds && s.sounds.length && !ST) setTimeout(() => pullState(s.sounds[0].name), 0); else if (!s.sounds || !s.sounds.length) showEmpty(); return; }
  $('#empty')?.remove();
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
  const ss = (ST.solo && ST.solo_stats) || d, fin = x => (Number.isFinite(x) ? x : -90);
  const stats = `<span class="chip"><b>${fin(ss.seconds ?? 0).toFixed(2)} s</b></span><span class="chip">pk <b>${fin(ss.peak_db).toFixed(1)}</b></span><span class="chip"><b>${fin(ss.lufs).toFixed(1)}</b> LUFS</span>`;
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
// Previews are keyed on the sound's own content hash (not the session rev, which moves on any
// change anywhere), kept as resolved values so a rebuilt node / tile draws in the same frame.
function cached(map, key, max, load) {
  let e = map.get(key);
  if (!e) { e = { v: null, p: load().then(v => (e.v = v), err => { map.delete(key); throw err; }) }; map.set(key, e); while (map.size > max) map.delete(map.keys().next().value); }
  return e;
}
const docKey = h => h != null ? h : `${PLOTREV}.${PLOTBUST}`;
const SPARKS = new Map();
function getSpark(name) {
  const k = docKey(ST.sound.hash), key = `${ST.doc}|${k}|${name}`;
  return cached(SPARKS, key, 400, () => bulk(`render/${ST.doc}/node/${name}/spark`, { rev: k }).then(b => new Float32Array(b)));
}
function drawSpark(c, x) {
  const g = c.getContext('2d'), W = c.width = 92, H = c.height = 32; g.clearRect(0, 0, W, H); if (!x || !x.length) return; let pk = 1e-9; for (const v of x) pk = Math.max(pk, Math.abs(v)); g.fillStyle = 'rgba(120,200,255,.75)';
  for (let i = 0; i < W; i++) { const m = Math.abs(x[Math.min(x.length - 1, Math.floor(i * x.length / W))]); const h = Math.max(1, m / pk * (H / 2 - 1)); g.fillRect(i, H / 2 - h, 1, 2 * h); }
}
function macroTargets(name) { const m = doc().macros.find(x => x.name === name); return m ? m.map.map(x => x.node) : []; }
function renderGraph() {
  const host = $('#graphhost'); host.hidden = S.centre !== 'graph'; if (host.hidden) return;
  { const rn = S.openRig && nodeOf(S.openRig); if (isRigish(rn)) return renderRigCanvas(host, doc(), rn); S.openRig = null; S.rigSel = null; }
  const d = doc(), nodes = d.nodes, by = Object.fromEntries(nodes.map(n => [n.name, n]));
  if (!nodes.length) { host.innerHTML = ''; return; }
  // Layout: columns by depth, each node pulled right to sit just before its first reader (short wires); rows
  // ordered by the average row of their inputs, a few sweeps each way (fewer crossings); columns centred.
  const hOf = n => isRigish(n) ? 96 : 76, GAP = 16, CW = 200;
  const ins = Object.fromEntries(nodes.map(n => [n.name, n.reads.filter(r => by[r])]));
  const outs = Object.fromEntries(nodes.map(n => [n.name, nodes.filter(x => ins[x.name].includes(n.name)).map(x => x.name)]));
  const col = Object.fromEntries(nodes.map(n => [n.name, n.depth || 0]));
  for (let k = 0; k < 4; k++) [...nodes].reverse().forEach(n => { if (outs[n.name].length && n.type !== 'output') col[n.name] = Math.max(col[n.name], Math.min(...outs[n.name].map(o => col[o])) - 1); });
  const maxC = Math.max(...Object.values(col)); nodes.filter(n => n.type === 'output').forEach(n => col[n.name] = maxC);
  const cols = []; nodes.forEach(n => (cols[col[n.name]] ||= []).push(n)); const rowOf = {}; const idx = () => cols.forEach(c => c && c.forEach((n, i) => rowOf[n.name] = i)); idx();
  const bary = (n, nb) => { const v = nb[n.name].map(x => rowOf[x]).filter(x => x != null); return v.length ? v.reduce((a, b) => a + b, 0) / v.length : rowOf[n.name]; };
  for (let k = 0; k < 6; k++) { (k % 2 ? [...cols].reverse() : cols).forEach(c => { if (!c) return; c.sort((a, b) => bary(a, k % 2 ? outs : ins) - bary(b, k % 2 ? outs : ins)); }); idx(); }
  const colH = c => c ? c.reduce((a, n) => a + hOf(n) + GAP, -GAP) : 0, Hmax = Math.max(...cols.map(colH)); const pos = {};
  cols.forEach((c, ci) => { if (!c) return; let y = (Hmax - colH(c)) / 2; c.forEach(n => { pos[n.name] = [ci * CW, y]; y += hOf(n) + GAP; }); });
  const LO = (d.layout && d.layout.nodes) || {}; nodes.forEach(n => { const o = (S.gdrag && S.gdrag.pos && S.gdrag.pos[n.name]) || LO[n.name]; if (o) pos[n.name] = [o[0], o[1]]; });
  const W = Math.max(0, ...Object.values(pos).map(p => p[0])) + 320, H = Math.max(0, ...Object.values(pos).map(p => p[1])) + 220;
  // Wires: audio inputs solid, control (a field other than input / inputs / excite / a / b ...) dashed and dimmer;
  // each input gets its own slot on the reader's left edge; a port read (x.force) is teal and labelled.
  const AUDIO = new Set(['input', 'inputs', 'excite', 'a', 'b', 'carrier', 'modulator', 'source']), WORDS = new Set(['op', 'mode', 'shape', 'curve', 'color', 'material', 'floor', 'kind']);
  const sel = S.sel; const wires = nodes.flatMap(n => {
    const uniq = refPaths(n).map(r => ({ path: r.path, src: r.value.split('.')[0], port: r.value.includes('.') ? r.value.split('.')[1] : null, audio: AUDIO.has(r.path[0]) })).filter(r => pos[r.src]);
    return uniq.map((r, i) => { const [x0, y0] = pos[r.src], [x1, y1] = pos[n.name], a = [x0 + 150, y0 + hOf(by[r.src]) / 2], b = [x1, y1 + 18 + (uniq.length > 1 ? i * (hOf(n) - 30) / (uniq.length - 1) : (hOf(n) - 36) / 2)];
      const id = [n.name, ...r.path].join('|'), picked = S.wireSel === id, on = picked || sel === n.name || sel === r.src, dim = sel && !on, dx = Math.max(30, Math.abs(b[0] - a[0]) / 2), c = picked ? '#ec6d6d' : on ? '#6fa8ff' : r.port ? '#4fd1c5' : r.audio ? '#5a6273' : '#454c59', pd = `M${a[0]},${a[1]} C${a[0] + dx},${a[1]} ${b[0] - dx},${b[1]} ${b[0]},${b[1]}`;
      return `<path class="whit" data-wire="${esc(id)}" d="${pd}"><title>${esc(r.src + (r.port ? '.' + r.port : ''))} → ${esc(n.name)}.${esc(r.path.join('.'))}</title></path><path d="${pd}" stroke="${c}" stroke-width="${on ? 2 : r.audio ? 1.6 : 1.2}" fill="none" ${r.audio || r.port ? '' : 'stroke-dasharray="4 3"'} opacity="${dim ? .35 : 1}"/><circle cx="${b[0]}" cy="${b[1]}" r="2.5" fill="${c}" opacity="${dim ? .35 : 1}"/>${r.port ? `<text x="${a[0] + 6}" y="${a[1] - 5}" class="wport">.${esc(r.port)}</text>` : ''}`; }); }).join('');
  const hotNodes = S.hot ? macroTargets(S.hot) : [];
  host.innerHTML = `<div class="gtool"><button class="tbtn" data-gadd>${ic('plus')}Node <kbd>A</kbd></button>${Object.keys(LO).length ? '<button class="tbtn" data-gtidy>Tidy</button>' : ''}${S.multi && S.multi.size > 1 ? `<span class="hint">${S.multi.size} selected</span>` : ''}</div><div class="graph" style="width:${W}px;height:${H}px"><svg class="wires">${wires}</svg><svg class="gtemp" id="gtemp"></svg>${nodes.map(n => { const ph = isRigish(n), nb = ph ? rigModel(flat(n)).bodies.length : 0;
    return `<div class="gnode ${n.type === 'output' ? 'out' : ''} ${ph ? 'phys' : ''} ${S.sel === n.name ? 'sel' : ''} ${S.multi && S.multi.has(n.name) ? 'multi' : ''} ${ST.solo === n.name ? 'solo' : ''} ${hotNodes.includes(n.name) ? 'mac' : ''}" data-node="${esc(n.name)}" style="left:${pos[n.name][0]}px;top:${pos[n.name][1]}px;height:${hOf(n)}px"><div class="n">${ic(ICON[n.type] || 'wave')}${esc(n.name)}</div><button class="gsolo ${ST.solo === n.name ? 'on' : ''}" data-solo="${esc(n.name)}" aria-label="Solo ${esc(n.name)}" title="Hear this node">${ic('phones')}</button><div class="gbadges"><span class="t">${ph ? `${esc(n.type)} · ${nb} bod${nb === 1 ? 'y' : 'ies'}` : esc(n.type)}</span>${n.open_comments ? `<span class="pinc" title="open comments on this node">${n.open_comments}</span>` : ''}${n.unread ? `<span class="warnc" title="changed by an agent since you last looked (open the sound to mark it seen)">${ic('warn')}</span>` : ''}</div>${ph ? `<div class="rthumb" data-rigopen="${esc(n.name)}" title="Open">${rigMini(flat(n))}<span class="rexp">${ic('zoom')}</span></div>` : `<canvas data-gspark="${esc(n.name)}"></canvas>`}${n.type !== 'output' ? `<span class="gout" data-gout="${esc(n.name)}" title="Drag onto a node to connect"></span>` : ''}${ph ? `<span class="gout port" data-gout="${esc(n.name)}.force" data-label=".force" title="Drag to connect the force port"></span>` : ''}</div>`; }).join('')}</div>`;
  host.querySelectorAll('[data-gspark]').forEach(c => { const name = c.dataset.gspark, doc0 = ST.doc, e = getSpark(name); if (e.v) { drawSpark(c, e.v); return; } e.p.then(x => { if (c.isConnected && ST.doc === doc0) drawSpark(c, x); }).catch(() => {}); });
}

// ---------- physics: rig and impact nodes (bodies and joints, stepped together) ----------
// Inside a rig nothing flows one way: bodies are vertices, joints two-way couplings. The canvas lays bodies out by
// height (a joint's `a` is the upper body, `b` the lower or the ground) because gravity is part of the model.
const PHYS = new Set(['modal', 'contact', 'impact', 'rig', 'friction', 'click', 'step']);
const SHAPES = ['plate', 'bar', 'tube', 'membrane', 'string', 'shell'];
const MATERIALS = ['wood', 'metal', 'glass', 'stone', 'plastic'];
const flat = n => ({ type: n.type, name: n.name, ...n.fields });
const isRigish = n => n && (n.type === 'rig' || n.type === 'impact');
const isModal = b => !!(b.material || b.f0 || b.shape);
function rigModel(n) { if (n.type === 'impact') return { bodies: [{ name: 'obj', mass: n.object_mass, material: n.material || 'wood', shape: n.shape, f0: n.f0, loss: n.loss, label: 'object' }].concat(n.striker_mass ? [{ name: 'striker', mass: n.striker_mass }] : []),
    joints: [n.striker_mass ? { kind: 'contact', a: 'striker', b: 'obj', bounce: n.bounce } : { kind: 'contact', a: 'obj', b: 'ground', gap: n.height, bounce: n.bounce }] };
  return { bodies: n.bodies || [], joints: n.joints || [] }; }
function rigLayout(m) {
  // Rows from the bottom up: a body sits one row above the highest body it rests on, so a joint never skips a row.
  // Each body is placed over where it attaches below, ordered by that point, so edges come out near vertical.
  if (!m.bodies.length) return { pos: {}, wid: {}, groundY: null, minX: 40, maxX: 300, W: 420, H: 160 };
  const names = m.bodies.map(b => b.name), h = Object.fromEntries(names.map(n => [n, 0]));
  for (let k = 0; k < 16; k++) for (const j of m.joints) if (j.b !== 'ground' && h[j.a] != null && h[j.b] != null) h[j.a] = Math.max(h[j.a], h[j.b] + 1);
  const top = Math.max(0, ...Object.values(h)), RH = 130, GAPX = 34, BW = 170;
  const wid = {}; names.forEach(n => { const up = m.joints.filter(j => j.b === n && h[j.a] === h[n] + 1).length; wid[n] = up > 1 ? Math.max(BW, up * (BW + GAPX) - GAPX) : BW; });
  const pos = {}, at = (n, v) => pos[n][0] + 10 + (v ?? 0.3) * (wid[n] - 20);
  for (let r = 0; r <= top; r++) { const row = names.filter(n => h[n] === r);
    const want = Object.fromEntries(row.map(n => { const js = m.joints.filter(j => j.a === n && pos[j.b]); const c = js.length ? js.reduce((a, j) => a + at(j.b, j.at_b), 0) / js.length : null; return [n, c == null ? null : c - (10 + (js[0].at_a ?? 0.3) * (wid[n] - 20))]; }));
    const key = n => { const js = m.joints.filter(j => j.a === n && pos[j.b]); return js.length ? js.reduce((a, j) => a + at(j.b, j.at_b), 0) / js.length : 1e9; }; row.sort((a, b) => key(a) - key(b));
    let x = 0; const y = (top - r) * RH + 24;
    row.forEach(n => { const w = want[n] ?? x; x = Math.max(x, w); pos[n] = [x, y]; x += wid[n] + GAPX; });
    const off = row.reduce((a, n) => a + ((want[n] ?? pos[n][0]) - pos[n][0]), 0) / Math.max(1, row.length); if (off < 0) row.forEach(n => pos[n][0] += off); }
  const minX = Math.min(...names.map(n => pos[n][0])), maxX = Math.max(...names.map(n => pos[n][0] + wid[n])); names.forEach(n => pos[n][0] += 40 - minX);
  const groundY = m.joints.some(j => j.b === 'ground') ? (top + 1) * RH + 24 - 40 : null;
  return { pos, wid, groundY, minX: 40, maxX: maxX - minX + 40, W: maxX - minX + 80, H: (groundY ?? top * RH + 24 + 50) + 30 }; }
// a thumbnail of the rig for its card in the signal graph: heard bodies as bars, silent ones as rings, joints as lines
function rigMini(n) { const m = rigModel(n), L = rigLayout(m), sx = 126 / Math.max(L.W, 1), sy = 30 / Math.max(L.H, 1);
  const c = b => [4 + (L.pos[b][0] + L.wid[b] / 2) * sx, 3 + (L.pos[b][1] + 25) * sy]; let g = '';
  m.joints.forEach(j => { if (!L.pos[j.a]) return; const [x1, y1] = c(j.a), [x2, y2] = j.b === 'ground' ? [x1, 31] : L.pos[j.b] ? c(j.b) : [x1, y1]; g += `<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="${j.kind === 'spring' ? '#4fd1c5' : '#8a91a0'}" stroke-width="1.2" ${j.kind === 'spring' ? 'stroke-dasharray="2 1.5"' : ''}/>`; });
  if (L.groundY != null) g += `<line x1="4" y1="31.5" x2="130" y2="31.5" stroke="#5b6270" stroke-width="1.5"/>`;
  m.bodies.forEach(b => { const [x, y] = c(b.name), w = Math.max(6, L.wid[b.name] * sx); g += isModal(b) ? `<rect x="${x - w / 2}" y="${y - 2.5}" width="${w}" height="5" rx="2" fill="#4fd1c5"/>` : `<circle cx="${x}" cy="${y}" r="3" fill="none" stroke="#c4c9d4" stroke-width="1.2"/>`; });
  return `<svg class="rmini" viewBox="0 0 134 34">${g}</svg>`; }

// slider scales for body and joint fields; missing fields show the node's defaults (docs/nodes/rig.md)
const RF = { mass: [0.001, 10, 'log', 'kg'], f0: [40, 8000, 'log', 'Hz'], loss: [0.1, 4, 'lin', ''], noise: [0, 1, 'lin', ''], level_db: [-24, 12, 'lin', 'dB'], velocity: [-10, 10, 'lin', 'm/s'],
  bounce: [0, 1, 'lin', ''], hardness: [0, 1, 'lin', ''], gap: [0, 1, 'lin', 'm'], gap_mm: [0, 5, 'lin', 'mm'], stiffness: [1e3, 1e7, 'log', 'N/m'], damping: [0, 20, 'lin', 'N s/m'] };
const RDEF = { at_a: 0.3, at_b: 0.3, gap: 0, bounce: 0.4, hardness: 0.5, stiffness: 2e5, damping: 2, level_db: 0, loss: 1, noise: 0, velocity: 0, mass: 1, f0: 300 };
const RDOC = { mass: 'kg; heavier hits harder and rings the other body more', f0: 'lowest mode', loss: 'higher dies faster', noise: 'modes as noise bands: real wood 0.6, metal 0.4', level_db: 'how loud this body is in the output', velocity: 'starting speed, negative is down',
  bounce: 'share of the speed kept on each rebound', hardness: 'a point mass\'s hardness', gap: 'starting distance', gap_mm: 'free play before it pulls: a rattle', stiffness: 'spring rate', damping: 'how fast the spring settles' };
function rfPos(k, v) { const [a, b, sc] = RF[k]; return sc === 'log' ? Math.log(v / a) / Math.log(b / a) : (v - a) / (b - a); }
function rfVal(k, t) { const [a, b, sc] = RF[k]; const v = sc === 'log' ? a * (b / a) ** t : a + (b - a) * t; return Math.abs(v) >= 100 ? Math.round(v) : +v.toFixed(v < 0.01 && v > 0 ? 5 : 3); }
function rfFmt(k, v) { const u = RF[k][3]; return (Math.abs(v) >= 1000 ? (v / 1000).toFixed(v >= 1e5 ? 0 : 1) + 'k' : v) + (u ? ' ' + u : ''); }

function renderRigCanvas(host, d, n0) { const n = flat(n0), m = rigModel(n), L = rigLayout(m), BH = 50;
  { const LO = (d.layout && d.layout.rigs && d.layout.rigs[n.name]) || {}, bd = S.bdrag && S.bdrag.rig === n.name && S.bdrag.pos ? S.bdrag : null;
    m.bodies.forEach(b => { const o = bd && bd.name === b.name ? bd.pos : LO[b.name]; if (o && L.pos[b.name]) L.pos[b.name] = [o[0], o[1]]; });
    const ps = m.bodies.map(b => L.pos[b.name]).filter(Boolean);
    if (ps.length) { const maxR = Math.max(...m.bodies.map(b => L.pos[b.name][0] + L.wid[b.name])), maxB = Math.max(...ps.map(p => p[1])) + BH, gnd = m.joints.some(j => j.b === 'ground') || !!JD;
      L.groundY = gnd ? Math.max(L.groundY || 0, maxB + 40) : null; L.minX = Math.min(...ps.map(p => p[0])); L.maxX = maxR; L.W = maxR + 80; L.H = (L.groundY ?? maxB) + 40; } }
  const ax = (name, at) => L.pos[name][0] + 10 + (at ?? 0.3) * (L.wid[name] - 20);
  let svg = '', html = '';
  m.joints.forEach((j, ji) => { if (!L.pos[j.a] || (j.b !== 'ground' && !L.pos[j.b])) return; const gnd = j.b === 'ground', flip = !gnd && L.pos[j.a][1] > L.pos[j.b][1]; const x1 = ax(j.a, j.at_a), y1 = L.pos[j.a][1] + (flip ? 0 : BH), x2 = gnd ? x1 : ax(j.b, j.at_b), y2 = gnd ? L.groundY : L.pos[j.b][1] + (flip ? BH : 0);
    const sel = S.rigSel === 'j' + ji, col = j.kind === 'spring' ? '#4fd1c5' : sel ? '#e3e6ec' : '#9aa1b0', mx = (x1 + x2) / 2, my = (y1 + y2) / 2;
    const len = Math.hypot(x2 - x1, y2 - y1) || 1, ux = (x2 - x1) / len, uy = (y2 - y1) / len, px = -uy, py = ux;
    let d_ = `M${x1} ${y1} L${x2} ${y2}`, cap = '';
    if (j.kind === 'contact') { const g = 4, w = 9, P = (t, o) => `${mx + ux * t + px * o} ${my + uy * t + py * o}`;
      d_ = `M${x1} ${y1} L${P(-g, 0)} M${P(g, 0)} L${x2} ${y2}`; cap = `<path d="M${P(-g, -w)} Q${P(-g - 3, 0)} ${P(-g, w)} M${P(g, -w)} Q${P(g + 3, 0)} ${P(g, w)}" stroke="${col}" stroke-width="2" fill="none" stroke-linecap="round"/>`; }
    if (j.kind === 'spring') { const seg = 7, t0 = .3, t1 = .7; d_ = `M${x1} ${y1} L${x1 + (x2 - x1) * t0} ${y1 + (y2 - y1) * t0} ` + Array.from({ length: seg }, (_, q) => { const t = t0 + (t1 - t0) * (q + .5) / seg, o = q % 2 ? 5 : -5; return `L${x1 + (x2 - x1) * t + px * o} ${y1 + (y2 - y1) * t + py * o}`; }).join(' ') + ` L${x1 + (x2 - x1) * t1} ${y1 + (y2 - y1) * t1} L${x2} ${y2}`; }
    const drag = n.type === 'rig';
    svg += `${sel ? `<path d="${d_}" stroke="${col}" stroke-opacity=".25" stroke-width="8" fill="none" stroke-linejoin="round"/>` : ''}<path d="${d_}" stroke="${col}" stroke-width="${sel ? 2.4 : 1.6}" fill="none"/>${cap}<path class="hit" data-rsel="j${ji}" d="M${x1} ${y1} L${x2} ${y2}"/>`
      + `<circle ${drag ? `class="ah" data-ah="${ji}:a"` : ''} cx="${x1}" cy="${y1}" r="${sel ? 5 : 3.5}" fill="${col}">${drag ? `<title>drag along ${esc(j.a)} to move where it attaches</title>` : ''}</circle>`
      + (gnd ? `<circle cx="${x2}" cy="${y2}" r="3.5" fill="${col}"/>` : `<circle ${drag ? `class="ah" data-ah="${ji}:b"` : ''} cx="${x2}" cy="${y2}" r="${sel ? 5 : 3.5}" fill="${col}">${drag ? `<title>drag along ${esc(j.b)} to move where it attaches</title>` : ''}</circle>`);
    const extra = [j.kind === 'spring' && j.gap ? `${+(j.gap * 1000).toFixed(2)} mm play` : '', j.kind === 'contact' && j.gap ? `${j.gap} m drop` : '', j.slide ? `slide ← ${j.slide}` : ''].filter(Boolean).join(' · ');
    html += `<div class="rjoint ${j.kind} ${sel ? 'sel' : ''}" data-rsel="j${ji}" style="left:${mx + 16}px;top:${my}px">${esc(j.kind)}${extra ? `<span>${esc(extra)}</span>` : ''}</div>`; });
  m.bodies.forEach(b => { const [x, y] = L.pos[b.name], modal = isModal(b), sel = S.rigSel === 'b:' + b.name;
    const kind = modal ? `${b.material || 'wood'} ${b.shape || 'plate'}${b.f0 ? ' · ' + fmtHz(b.f0) + ' Hz' : ''}` : `${b.mass ?? '?'} kg mass`;
    const tags = [b.fixed ? '<span class="tag">fixed</span>' : '', b.velocity != null ? `<span class="tag start">${b.velocity < 0 ? '↓' : '↑'} ${Math.abs(b.velocity)} m/s</span>` : '', b.drive ? `<span class="tag in">drive ← ${esc(b.drive)}</span>` : '', modal ? `<span class="tag out" title="heard in the audio output">heard${b.level_db ? ' ' + b.level_db + ' dB' : ''}</span>` : '<span class="tag">silent</span>'].join('');
    html += `<div class="rbody ${modal ? '' : 'silent'} ${sel ? 'sel' : ''}" data-rsel="b:${esc(b.name)}" style="left:${x}px;top:${y}px;height:${BH}px;width:${L.wid[b.name]}px"><div class="n">${esc(b.label || b.name)}<span class="tags">${tags}</span></div><div class="t">${esc(kind)}</div>${n.type === 'rig' ? `<span class="jedge top" data-jedge="${esc(b.name)}" title="Drag to another body to join them"></span><span class="jedge bot" data-jedge="${esc(b.name)}" title="Drag to another body or the ground"></span>` : ''}</div>`; });
  if (L.groundY != null) html += `<div class="rground ${JD ? 'drop' : ''}" style="left:${L.minX - 20}px;width:${L.maxX - L.minX + 40}px;top:${L.groundY}px"><span>ground${n.floor ? ' · ' + esc(n.floor) : ''}</span></div>`;
  const nb = m.bodies.length, nj = m.joints.length;
  host.innerHTML = `<div class="rigbar"><button data-rigclose>‹ ${esc(ST.doc)}</button><span>/</span><b>${esc(n.name)}</b><span class="badge">${esc(n.type)}</span>${n.type === 'rig' ? `<span class="rtools"><button class="tbtn" data-radd="mass">${ic('plus')}Mass</button><button class="tbtn" data-radd="modal">${ic('plus')}Modal</button>${Object.keys((d.layout && d.layout.rigs && d.layout.rigs[n.name]) || {}).length ? '<button class="tbtn" data-gtidy>Tidy</button>' : ''}</span>` : ''}<span class="hint">${nb} ${nb === 1 ? 'body' : 'bodies'} · ${nj} ${nj === 1 ? 'joint' : 'joints'}</span></div>
    <div class="rig" style="width:${L.W + (S.rigSel ? 240 : 0)}px;height:${Math.max(L.H, S.rigSel ? 300 : 0)}px"><svg class="rw" width="${L.W}" height="${L.H}">${svg}</svg><svg class="rtemp" id="rtemp" width="${L.W}" height="${L.H}"></svg>${html}${nb ? '' : '<div class="hint" style="position:absolute;left:40px;top:40px">No bodies</div>'}${rigPop(n, L)}</div>`; }
// the edit popover for a body or joint; a field a knob drives shows the knob instead of a slider
function rigPop(n, L) { const m = rigModel(n); if (!S.rigSel || n.type !== 'rig') return '';
  const driver = (id, k) => { const f = k === 'gap_mm' ? 'gap' : k; const field = id.startsWith('b:') ? 'bodies' : 'joints', index = id.startsWith('b:') ? m.bodies.findIndex(q => 'b:' + q.name === id) : +id.slice(1);
    return doc().macros.find(mc => mc.map.some(x => x.node === n.name && x.field === field && x.index === index && x.key === f)); };
  const lab = k => k === 'gap_mm' ? 'play' : k.replace('_', ' ');
  const row = (id, k, v, set) => { const mc = driver(id, k); if (mc) return `<div class="field"><label>${lab(k)}</label><div class="link"><span class="macchip" data-kfocus="${esc(mc.name)}" title="Turn the ${esc(mc.name)} knob">◎ ${esc(mc.name)}</span></div></div>`;
    const shown = k === 'gap_mm' ? v * 1000 : v; return `<div class="field" title="${RDOC[k] || ''}"><label>${lab(k)}</label><input type="range" min="0" max="1" step="0.001" value="${rfPos(k, shown)}" data-rp="${esc(id)}:${k}"${set ? '' : ' class="dflt"'}><span class="val" data-rpv="${esc(id)}:${k}">${rfFmt(k, shown)}</span></div>`; };
  const en = (id, k, opts, v) => `<div class="field"><label>${k}</label><div class="opts">${opts.map(o => `<button data-rpe="${esc(id)}:${k}" data-v="${o}" aria-pressed="${o === v}">${o}</button>`).join('')}</div></div>`;
  let head = '', body = '', x = 0, y = 0;
  if (S.rigSel.startsWith('b:')) { const b = m.bodies.find(q => 'b:' + q.name === S.rigSel); if (!b) return ''; const id = S.rigSel, modal = isModal(b); [x, y] = [L.pos[b.name][0] + L.wid[b.name] + 12, L.pos[b.name][1]];
    head = `<input class="rbname" data-rbrename="${esc(b.name)}" value="${esc(b.name)}" spellcheck="false"><span class="hint">${modal ? 'modal body' : 'point mass'}</span>`;
    const ks = modal ? ['f0', 'loss', 'noise', 'mass', 'level_db', 'velocity'] : ['mass', 'velocity'];
    body = (modal ? en(id, 'material', MATERIALS, b.material || 'wood') + en(id, 'shape', SHAPES, b.shape || 'plate') : '') + ks.map(k => row(id, k, b[k] ?? RDEF[k], b[k] != null)).join('') +
      `<div class="field"><label>fixed</label><div class="opts"><button data-rpe="${esc(id)}:fixed" data-v="true" aria-pressed="${!!b.fixed}">held</button><button data-rpe="${esc(id)}:fixed" data-v="false" aria-pressed="${!b.fixed}">free</button></div></div>`; }
  else { const ji = +S.rigSel.slice(1), j = m.joints[ji]; if (!j || !L.pos[j.a]) return ''; const id = S.rigSel, a = L.pos[j.a], b = j.b === 'ground' ? [a[0], L.groundY - 50] : L.pos[j.b]; [x, y] = [Math.max(a[0], b[0]) + 180, (a[1] + b[1]) / 2];
    head = `<b style="color:${j.kind === 'spring' ? 'var(--a3)' : 'var(--fg)'}">${esc(j.kind)}</b><span class="hint">${esc(j.a)} ↔ ${esc(j.b)}</span>`;
    const ks = j.kind === 'spring' ? ['gap_mm', 'stiffness', 'damping'] : ['bounce', 'hardness', 'gap'];
    body = ks.map(k => { const f = k === 'gap_mm' ? 'gap' : k; return row(id, k, j[f] ?? RDEF[f], j[f] != null); }).join('') + (j.slide ? `<div class="hint" style="margin-top:4px">slides ← ${esc(j.slide)}</div>` : ''); }
  return `<div class="rpop" style="left:${x}px;top:${y}px"><div class="rph">${head}<button data-rpclose title="Close (Esc)">${ic('x')}</button></div>${body}<div class="rpf"><button class="tbtn danger" data-rpdel>${ic('trash')}Delete</button></div></div>`; }
// a body / joint edit as the whole bodies / joints list, which set_field upserts as one field
function rigPatch(n, id, k, v) { if (id.startsWith('b:')) { const name = id.slice(2); return ['bodies', n.bodies.map(b => b.name === name ? { ...b, [k]: v } : b)]; }
  const ji = +id.slice(1); if (k === 'gap_mm') { k = 'gap'; v = +(v / 1000).toFixed(5); } return ['joints', n.joints.map((j, i) => i === ji ? { ...j, [k]: v } : j)]; }
function physSection(n0) { const n = flat(n0), m = rigModel(n);
  const li = (id, main, sub, c) => `<button class="rli ${S.rigSel === id ? 'on' : ''}" data-rigpick="${esc(id)}"><span style="color:${c}">${esc(main)}</span><span class="hint">${esc(sub)}</span></button>`;
  const rows = n.type === 'rig' ? m.bodies.map(b => li('b:' + b.name, b.name, isModal(b) ? `${b.material || 'wood'} ${b.shape || 'plate'}` : `${b.mass ?? '?'} kg · silent`, 'var(--fg)')).join('') + m.joints.map((j, ji) => li('j' + ji, j.kind, `${j.a} ↔ ${j.b}`, j.kind === 'spring' ? 'var(--a3)' : 'var(--dim)')).join('') : '';
  return `<div class="sect"><div class="sh">Physics <span class="hint" style="text-transform:none;letter-spacing:0">${m.bodies.length} bodies · ${m.joints.length} joints</span></div>
    <button class="tbtn" data-rigopen="${esc(n.name)}" style="margin-bottom:8px">${ic('grid')}Open the rig</button><div class="rlist">${rows}</div></div>`; }
function openRig(name, pick) { S.centre = 'graph'; S.openRig = S.sel = name; S.rigSel = pick || null; S.tab = 'node'; renderCentre(); renderRight(); }
function closeRig() { S.openRig = null; S.rigSel = null; renderGraph(); renderRight(); }
// rig edits: change the local copy at once (so the canvas follows the pointer), send one set_field on release
function rigLocal(id, k, v) { const n0 = nodeOf(S.openRig); if (!n0) return null; const [field, value] = rigPatch(flat(n0), id, k, v); n0.fields[field] = value; return [field, value]; }
function rigSend(field, value) { act('set_field', { node: S.openRig, field, value }); }
$('#graphhost').addEventListener('mouseover', e => { const h = e.target.closest('.hit'); document.querySelectorAll('.rjoint').forEach(c => c.classList.toggle('hover', !!h && c.dataset.rsel === h.dataset.rsel)); });
let AH = null;
$('#graphhost').addEventListener('pointerdown', e => { const h = e.target.closest('.ah'); if (!h) return; e.preventDefault(); e.stopPropagation(); const [ji, end] = h.dataset.ah.split(':'); AH = { ji: +ji, end, moved: false }; S.rigSel = 'j' + ji; });
addEventListener('pointermove', e => { if (!AH) return; const n0 = nodeOf(S.openRig); if (!n0 || !n0.fields.joints) return; const j = n0.fields.joints[AH.ji], body = AH.end === 'a' ? j.a : j.b;
  const L = rigLayout(rigModel(flat(n0))), rig = $('#graphhost .rig'); if (!rig || !L.pos[body]) return; const r = rig.getBoundingClientRect();
  const at = Math.max(0, Math.min(1, +((e.clientX - r.left - L.pos[body][0] - 10) / (L.wid[body] - 20)).toFixed(2)));
  n0.fields.joints = n0.fields.joints.map((q, i) => i === AH.ji ? { ...q, ['at_' + AH.end]: at } : q); AH.moved = true; renderGraph(); });
addEventListener('pointerup', () => { if (!AH) return; const a = AH; AH = null; if (a.moved) rigSend('joints', nodeOf(S.openRig).fields.joints); else { renderGraph(); renderRight(); } });
$('#graphhost').addEventListener('input', e => { const t = e.target.closest('[data-rp]'); if (!t) return; const [a, b, k] = t.dataset.rp.split(':'), id = a === 'b' ? a + ':' + b : a, key = a === 'b' ? k : b;
  const v = rfVal(key, +t.value); rigLocal(id, key, v); t.classList.remove('dflt'); const o = document.querySelector(`[data-rpv="${CSS.escape(t.dataset.rp)}"]`); if (o) o.textContent = rfFmt(key, v); });
$('#graphhost').addEventListener('change', e => { const t = e.target.closest('[data-rp]'); if (!t) return; const [a, b, k] = t.dataset.rp.split(':'), id = a === 'b' ? a + ':' + b : a, key = a === 'b' ? k : b;
  const f = key === 'gap_mm' || id.startsWith('j') ? 'joints' : 'bodies'; rigSend(f, nodeOf(S.openRig).fields[f]); });
$('#graphhost').addEventListener('dblclick', e => { const c = e.target.closest('.gnode'); if (!c) return; const n = nodeOf(c.dataset.node); if (isRigish(n)) { e.stopPropagation(); openRig(n.name); } });

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
  return `<div class="knob ${S.hot === m.name ? 'hot' : ''} ${S.mapArm && S.mapArm.name === m.name ? 'armed' : ''}" data-knob="${esc(m.name)}"><div class="kn" title="Edit ${esc(m.name)}">${esc(m.name)}${m.game ? '<span class="game" title="exported: the game sets this at runtime">GAME</span>' : ''}</div>
    <svg class="dial" viewBox="0 0 66 60" tabindex="0" role="slider" aria-label="${esc(m.name)}" aria-valuemin="0" aria-valuemax="1" aria-valuenow="${m.value.toFixed(2)}" data-dial="${esc(m.name)}">${dial(m)}</svg>
    <div class="kval"><span data-kv="${esc(m.name)}">${m.value.toFixed(2)}</span></div></div>`;
}
function renderDock() {
  const d = doc(), dk = $('#dock'); dk.hidden = S.view === 'grid'; if (KD) return;
  const moved = d.macros.filter(m => Math.abs(m.value - m.default) > 0.005).length;
  const html = `<div class="dockgrip" data-dockgrip title="Drag to resize the macro pane"></div><div class="dockh"><span class="t">Macros</span><span class="grow"></span>${moved ? `<button class="tbtn" data-kreset title="Back to the agent's settings">Reset</button>` : ''}</div>
    <div class="knobs">${d.macros.map(knobHTML).join('')}<div class="knob add" data-kadd tabindex="0" title="A new knob">${ic('plus')}<span>Knob</span></div></div>`;
  // Rebuilt on every state change: skip when nothing changed, and keep the grid's scroll position otherwise.
  if (dk.dataset.html === html) return;
  const keep = dk.querySelector('.knobs'), top = keep ? keep.scrollTop : 0;
  dk.dataset.html = html; dk.innerHTML = html;
  const row = dk.querySelector('.knobs'); if (row && top) row.scrollTop = top;
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
    try { do { k.dirty = false; const m = macroOf(name); if (!m) break; mergeSound(await sounder.call('move_knob', { name, value: m.value }), name); PLOTBUST++; renderHud(); if (S.tab === 'sound') renderRight(); refreshPlot(); } while (k.dirty); }
    catch (e) { toast(errText(e)); } finally { k.run = null; }
  })();
  return k.run;
}
// merge a move_knob reply (macros, moves, seconds, peak_db, lufs) into state.sound
function mergeSound(r, keep) {
  const d = doc(); if (!r) return;
  for (const k of ['seconds', 'peak_db', 'lufs', 'moves', 'hash']) if (r[k] !== undefined) d[k] = r[k];
  if (r.macros) d.macros = r.macros.map(m => { const old = d.macros.find(x => x.name === m.name); return (keep === m.name || (KD && KD.name === m.name)) && old ? { ...m, value: old.value } : m; });
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
    if (!n) body = `<div class="sect hint">No node selected.</div>`;
    else { const readBy = d.nodes.filter(x => x.reads.includes(n.name)).map(x => x.name); const link = x => `<a href="#" data-goto="${esc(x)}" style="color:var(--accent)">${esc(x)}</a>`;
      body = `<div class="sect"><div class="nodehead">${ic(ICON[n.type] || 'wave')}<input class="nmedit" data-rename="${esc(n.name)}" value="${esc(n.name)}" spellcheck="false"><span class="badge">${esc(n.type)}</span>${PHYS.has(n.type) ? '<span class="physbadge">physical</span>' : ''}<button class="tbtn" data-solo="${esc(n.name)}" aria-pressed="${ST.solo === n.name}" title="Hear this node">${ic('phones')}Solo</button>${n.type !== 'output' ? `<button class="tbtn icon" data-ndup title="Duplicate (⌘D)">${ic('copy')}</button><button class="tbtn icon danger" data-ndel title="Delete (⌫)">${ic('trash')}</button>` : ''}</div><div class="blurb">${esc(BLURB[n.type] || '')}</div></div>
      ${isRigish(n) ? physSection(n) : ''}
      <div class="sect"><div class="sh">Fields</div>${fieldRows(n)}</div>
      <div class="sect"><div class="sh">Wiring</div><dl class="kv"><dt>reads</dt><dd>${n.reads.filter(x => d.nodes.some(y => y.name === x)).map(link).join(', ') || '—'}</dd><dt>read by</dt><dd>${readBy.map(link).join(', ') || (n.type === 'output' ? 'the sound' : '<span style="color:var(--warn)">nothing</span>')}</dd><dt>peak</dt><dd data-nodepeak="${esc(n.name)}">…</dd></dl></div>
      <div class="sect"><div class="sh">JSON</div><pre class="mono" style="margin:0;white-space:pre-wrap;color:var(--dim);font-size:11px">${esc(JSON.stringify({ type: n.type, name: n.name, ...n.fields }))}</pre></div>`;
      const name = n.name, doc0 = ST.doc; getSpark(name).p.then(x => { const el = document.querySelector(`[data-nodepeak="${name}"]`); if (el && ST.doc === doc0) { let pk = 0; for (const v of x) pk = Math.max(pk, Math.abs(v)); el.textContent = `${(20 * Math.log10(Math.max(pk, 1e-10))).toFixed(1)} dBFS`; } }).catch(() => {}); }
  } else if (S.tab === 'sound') {
    const ds = d.descriptors || {}, bmax = -6, bmin = -72;
    const cell = v => { const t = Math.max(0, Math.min(1, (v - bmin) / (bmax - bmin))); const k = Math.round((t * 0.92 + 0.04) * 255) * 4; return `<span style="background:rgb(${CMAP[k]},${CMAP[k + 1]},${CMAP[k + 2]})" title="${v.toFixed(1)} dB">${v < -99 ? '' : Math.round(v)}</span>`; };
    const checks = (d.checks || []).map(c => [c.name, c.pass, esc(c.detail).replace(/^(-?[\d.]+)/, '<b>$1</b>')]);
    if (!checks.some(c => c[0].startsWith('macro'))) d.macros.forEach(m => { if (m.result) checks.push([`macro ${m.name}`, m.result.pass, esc(m.result.detail)]); });
    const bands = ds.bands || [], by = (ST.sounds.find(s => s.name === ST.doc) || {}).by || '';
    const params = Object.entries(d.params || {});
    const out = d.nodes.find(x => x.type === 'output'), of = out ? out.fields : {}, tgt = of.lufs != null ? 'lufs' : of.peak_db != null ? 'peak' : 'none';
    body = `<div class="sect"><div class="sh">Sound</div>
      <div class="field"><label>kind</label><div class="opts">${['oneshot', 'loop'].map(k => `<button data-skind="${k}" aria-pressed="${d.kind === k}">${k}</button>`).join('')}</div></div>
      <div class="field"><label>duration</label><div class="link"><label class="chk2"><input type="checkbox" data-sdurauto ${d.duration ? '' : 'checked'}>auto</label>${d.duration ? `<input class="fnum" type="number" step="0.1" min="0.01" data-sdur value="${d.duration}"><span class="hint">s</span>` : ''}</div></div>
      <div class="field"><label>seed</label><div class="link"><input class="fnum" type="number" step="1" min="1" data-sseed value="${d.seed}"></div></div>
      ${out ? `<div class="field"><label>level</label><div class="opts">${[['none', 'none'], ['lufs', 'LUFS'], ['peak', 'peak']].map(([k, l]) => `<button data-starget="${k}" aria-pressed="${tgt === k}">${l}</button>`).join('')}</div></div>${tgt !== 'none' ? `<div class="field"><label>${tgt === 'lufs' ? 'LUFS' : 'peak dBFS'}</label><div class="link"><input class="fnum" type="number" step="0.5" data-stval value="${tgt === 'lufs' ? of.lufs : of.peak_db}"></div></div>` : ''}
      <div class="field"><label>fades ms</label><div class="link"><input class="fnum" type="number" step="0.5" min="0" data-sfade="in" value="${+((of.fade_in ?? 0.002) * 1000).toFixed(2)}" title="fade in"><input class="fnum" type="number" step="1" min="0" data-sfade="out" value="${+((of.fade_out ?? 0.005) * 1000).toFixed(2)}" title="fade out"></div></div>
      <div class="field"><label>ceiling dB</label><div class="link"><input class="fnum" type="number" step="0.5" max="0" data-sceil value="${of.ceiling_db ?? -1}"></div></div>` : ''}
      <dl class="kv" style="margin-top:8px"><dt>sample rate</dt><dd>${d.sample_rate}</dd><dt>causal</dt><dd>${d.causal ? 'yes' : 'no'}</dd></dl>
      <div class="sbtns"><button class="tbtn" data-dlg="save">Save to catalog…</button><button class="tbtn" data-dlg="export">Export…</button></div></div>
      ${d.macros.length ? `<div class="sect"><div class="sh">Macros <span class="hint" style="text-transform:none;letter-spacing:0">by ${esc(by)}</span></div>${d.macros.map(m => `<div style="margin:0 0 9px"><div style="display:flex;gap:6px;align-items:center"><span class="macchip" data-kfocus="${esc(m.name)}">◎ ${esc(m.name)}</span>${m.game ? '<span class="game">GAME</span>' : ''}<span class="grow"></span><span class="mono">${m.value.toFixed(2)}</span><span class="hint">default ${m.default.toFixed(2)}</span></div><div class="hint" style="margin:3px 0">${esc(m.doc)}</div><div class="fb">${m.map.map(x => mapText(x).map(t => `<span class="mono" style="font-size:11px">${esc(t)}</span>`).join('')).join('')}</div></div>`).join('')}</div>` : ''}
      ${(d.moves || []).length ? `<div class="sect"><div class="sh">Your moves</div><div class="fb">${d.moves.map(f => `<span>${esc(f.name)} <span class="mono">${f.from.toFixed(2)} → ${f.to.toFixed(2)}</span></span><span>now</span>`).join('')}</div></div>` : ''}
      <div class="sect"><div class="sh">Params</div>${params.map(([k, v]) => `<div class="field"><label>$${esc(k)}</label><div class="link"><input class="fnum" type="number" step="any" data-pval="${esc(k)}" value="${esc(v)}"></div></div>`).join('')}<div class="field"><label></label><div class="link"><input class="fnum" data-pnew placeholder="name" spellcheck="false"><button data-padd>+ param</button></div></div></div>
      <div class="sect"><div class="sh">Checks <span class="badge ${checks.every(c => c[1]) ? 'ok' : 'bad'}">${checks.filter(c => c[1]).length}/${checks.length}</span></div>${checks.map(([k, p, t]) => `<div class="chk"><span class="p ${p ? 'ok' : 'bad'}">${p ? 'PASS' : 'FAIL'}</span><span class="d">${esc(k)}: ${t}</span></div>`).join('')}${(d.warnings || []).map(w => `<div class="wrn">${ic('warn')}${esc(w)}</div>`).join('')}</div>
      ${bands.length ? `<div class="sect"><div class="sh">Bands (dB) × time</div><div class="bt"><span class="lab"></span>${['0', '20', '40', '60', '80%'].map(h => `<span class="hd">${h}</span>`).join('')}${bands.map((row, b) => `<span class="lab">${BLAB[b] || ''}</span>${row.map(cell).join('')}`).reverse().join('')}</div></div>` : ''}
      <div class="sect"><div class="sh">Descriptors</div><dl class="kv"><dt>length</dt><dd>${(d.seconds ?? 0).toFixed(3)} s</dd><dt>peak</dt><dd>${d.peak_db == null ? '—' : d.peak_db.toFixed(1) + ' dBFS'}</dd><dt>loudness</dt><dd>${d.lufs == null ? '—' : d.lufs.toFixed(1) + ' LUFS'}</dd>${ds.rms_db != null ? `<dt>RMS</dt><dd>${ds.rms_db.toFixed(1)} dBFS</dd>` : ''}${ds.centroid_hz != null ? `<dt>centroid</dt><dd>${fmtHz(sme(ds.centroid_hz))} Hz</dd>` : ''}</dl></div>`;
  } else {
    body = d.comments.length ? d.comments.map(c => { const rep = c.reply && (typeof c.reply === 'string' ? { by: 'agent', text: c.reply } : c.reply);
      return `<div class="cmt ${c.resolved ? 'done' : ''}" data-cmt="${c.id}"><span class="num">${c.id}</span><div><div class="hd"><b>${esc(c.by)}</b>${c.ago ? `<span>${esc(c.ago)}</span>` : ''}<span class="on">${esc(pinText(onOf(c)))}</span>${c.resolved ? '<span style="color:var(--ok)">resolved</span>' : ''}</div>${esc(c.text)}${rep ? `<div class="reply"><div class="hd"><b>${esc(rep.by || 'agent')}</b><span>agent</span></div>${esc(rep.text)}</div>` : ''}</div></div>`; }).join('') : `<div class="sect hint">No comments.</div>`;
    const pin = S.pendingPin || (S.sel ? { node: S.sel } : {});
    body += `<div class="grow"></div><div class="compose"><div class="pinto">${ic('pin')} on <span class="on mono" style="border:1px solid var(--line);border-radius:4px;padding:0 5px">${esc(pinText(pin))}</span>${S.pendingPin ? '<button class="hint" data-unpin>(whole sound)</button>' : ''}</div><div class="row2"><input id="cin" placeholder="What do you hear?" autocomplete="off"><button class="primary" id="csend">Post</button></div></div>`;
  }
  const keep = $('#cin') ? $('#cin').value : null;
  r.innerHTML = tabs + body; if (keep && $('#cin')) $('#cin').value = keep;
}

// ---------- grid of all sounds ----------
const THUMBS = new Map(); let THUMB_IO = null;
function renderOverview() {
  $('#overview').hidden = S.view !== 'grid'; if (S.view !== 'grid') return;
  const sounds = ST.sounds;
  const gridHTML = `<div class="ovsec">${esc(ST.project || 'friendSLOP')} · ${sounds.length} open${(ST.recent || []).length ? ` · ${ST.recent.length} saved` : ''}</div>` + sounds.map(d => `<div class="tile" data-doc="${esc(d.name)}" tabindex="0"><div class="thumb"><canvas data-th="${esc(d.name)}"></canvas>${d.working ? `<span class="chip working" style="font-size:11px"><i class="dot live" style="background:${agentColor(d.by)}"></i>${esc(d.by)} editing</span>` : ''}<button class="tp" data-audition="${esc(d.name)}" aria-label="Play ${esc(d.name)}">${ic('play')}</button></div><div class="info"><span class="n">${esc(d.name)}</span><span class="badge ${d.kind === 'loop' ? 'loop' : ''}">${esc(d.kind)}</span></div><div class="sub"><span class="grow">${d.seconds != null ? `${d.seconds.toFixed(2)} s${d.lufs != null ? ` · ${d.lufs.toFixed(1)} LUFS` : ''}` : ''}</span><span>${esc(d.by)} · ${esc(d.ago)}${d.saved ? '' : ' · <span style="color:var(--warn)">unsaved</span>'}</span></div></div>`).join('')
    + (ST.recent || []).map(d => `<div class="tile" data-load="${esc(d.name)}" tabindex="0" title="Saved sound: click to open"><div class="thumb"></div><div class="info"><span class="n">${esc(d.name)}</span><span class="badge ${d.kind === 'loop' ? 'loop' : ''}">${esc(d.kind)}</span></div><div class="sub"><span class="grow">${Number.isFinite(d.seconds) ? `${d.seconds.toFixed(2)} s` : ''}${Number.isFinite(d.lufs) && d.lufs > -90 ? ` · ${d.lufs.toFixed(1)} LUFS` : ''}</span><span>saved · ${esc(d.ago)}</span></div></div>`).join('');
  const og = $('#ovgrid'); if (og.dataset.html !== gridHTML) { og.dataset.html = gridHTML; og.innerHTML = gridHTML; }
  $('#feed').innerHTML = (ST.feed || []).map(f => { const i = f.what.indexOf(' · '); const what = i > 0 ? `<b>${esc(f.what.slice(0, i))}</b>${esc(f.what.slice(i))}` : esc(f.what);
    return `<div class="ev" data-doc="${esc(f.doc)}"><i class="dot" style="background:${agentColor(f.agent)}"></i><span class="what"><span class="mono">${esc(f.tool)}</span> ${what}</span><time>${esc(f.ago)}</time></div>`; }).join('');
  const hashes = new Map(sounds.map(d => [d.name, d.hash]));
  const drawTh = c => {
    if (!CMAP || !c.clientWidth || !c.clientHeight) return false; const name = c.dataset.th, W = c.width = Math.max(8, Math.round(c.clientWidth * DPR)), H = c.height = Math.max(8, Math.round(c.clientHeight * DPR));
    const k = docKey(hashes.get(name)), e = cached(THUMBS, `${name}|${k}|${W}x${H}`, 120, () => bulk(`render/${name}/spectrogram`, { rev: k, w: W, h: H }).then(b => specCanvas(parseSpec(b))));
    const draw = sc => { if (c.isConnected) c.getContext('2d').drawImage(sc, 0, 0, W, H); };
    if (e.v) draw(e.v); else e.p.then(draw).catch(() => {}); return true;
  };
  THUMB_IO?.disconnect(); THUMB_IO = new IntersectionObserver(es => es.forEach(x => { if (x.isIntersecting && drawTh(x.target)) THUMB_IO.unobserve(x.target); }), { rootMargin: '200px' });
  document.querySelectorAll('[data-th]').forEach(c => THUMB_IO.observe(c));
}

// ---------- toast, render all ----------
function toast(t) { const e = $('#toast'); e.innerHTML = `<span class="chip"><b>${esc(t)}</b></span>`; e.hidden = false; clearTimeout(toast.h); toast.h = setTimeout(() => e.hidden = true, 2600); }
function renderCentre() { renderHud(); renderGraph(); requestAnimationFrame(() => { renderPlot(); showRegbar(); refreshPlot(); }); }
function renderAll() { renderTop(); renderOverview(); renderCentre(); renderDock(); renderRight(); renderOpslog(); }

// =====================================================================
// Wiring (every action is a call; the page keeps no copy of the sound)
// =====================================================================
$('#gridbtn').onclick = () => { if (P.playing) act('stop', {}); S.view = 'grid'; renderAll(); };
$('#docbtn').onclick = () => { if (S.view === 'grid') { S.view = 'doc'; renderAll(); return; } const p = $('#docpop'); p.hidden = !p.hidden; if (!p.hidden) { renderDocList(); $('#docq').focus(); loadCatalog(); } };
let CATALOG = [];
async function loadCatalog() { try { const r = await sounder.call('catalog_list', {}); CATALOG = (Array.isArray(r) ? r : (r && r.sounds) || []).map(x => typeof x === 'string' ? { name: x } : x); renderDocList(); } catch (e) { /* the saved list is optional */ } }
// The human's verdict by ear on a saved sound: agents search for approved parts.
function apTag(name) {
  const c = CATALOG.find(x => x.name === name); if (!c) return '';
  return `<span class="ap ${c.approved ? 'on' : ''}" data-approve="${esc(name)}" data-on="${c.approved ? 1 : 0}" title="${c.approved ? 'You approved this by ear. Click to withdraw.' : 'Mark as approved by ear: agents prefer approved parts'}">${c.approved ? 'approved' : 'approve'}</span>`;
}
function renderDocList() {
  const q = $('#docq').value.toLowerCase();
  const open = ST.sounds.filter(d => d.name.includes(q)).map(d => {
    return `<div class="docrow ${d.name === ST.doc ? 'on' : ''}" data-doc="${esc(d.name)}">${ic(d.kind === 'loop' ? 'loop' : 'wave')}<span style="display:flex;gap:6px;align-items:center">${esc(d.name)}${d.open_comments ? `<span class="pinc" style="font:600 10px var(--sans);background:var(--warn);color:#1b1600;border-radius:99px;padding:0 5px" title="${d.open_comments} open comments">${d.open_comments}</span>` : ''}${d.working ? `<i class="dot live" style="background:${agentColor(d.by)}" title="${esc(d.by)} is editing"></i>` : ''}${d.saved ? '' : '<span style="color:var(--warn)" title="unsaved">●</span>'}${d.saved ? apTag(d.name) : ''}</span><span class="r">${esc(d.by)} · ${esc(d.ago)}</span></div>`; }).join('');
  const openNames = new Set(ST.sounds.map(d => d.name));
  const saved = CATALOG.filter(c => !openNames.has(c.name) && c.name.includes(q));
  $('#doclist').innerHTML = open + (saved.length ? `<div class="h" style="font-size:11px;letter-spacing:.06em;text-transform:uppercase;color:var(--faint);padding:8px 8px 4px">Saved</div>` + saved.map(c => `<div class="docrow" data-load="${esc(c.name)}">${ic(c.kind === 'loop' ? 'loop' : 'wave')}<span style="display:flex;gap:6px;align-items:center">${esc(c.name)}${c.role ? `<span style="color:var(--faint);font-size:11px">${esc(c.role)}</span>` : ''}${apTag(c.name)}</span><span class="r">open</span></div>`).join('') : '');
}
$('#docq').oninput = renderDocList;
async function openSound(name) { S.view = 'doc'; try { await sounder.call('select_sound', { doc: name }); } catch (e) { toast(errText(e)); } return pullState(name); }
$('#doclist').onclick = async e => {
  const ap = e.target.closest('[data-approve]'); if (ap) { e.stopPropagation(); try { const r = await sounder.call('catalog_approve', { name: ap.dataset.approve, approved: ap.dataset.on !== '1' }); if (Array.isArray(r)) CATALOG = r; renderDocList(); } catch (er) { toast(errText(er)); } return; }
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
  if (e.target.closest('[data-rigclose]')) { closeRig(); return; }
  const ro = e.target.closest('[data-rigopen]'); if (ro) { e.stopPropagation(); openRig(ro.dataset.rigopen); return; }
  if (S.openRig) {
    if (e.target.closest('[data-rpclose]')) { S.rigSel = null; renderGraph(); renderRight(); return; }
    const pe = e.target.closest('[data-rpe]'); if (pe) { const [a, b, k] = pe.dataset.rpe.split(':'), v = pe.dataset.v === 'true' ? true : pe.dataset.v === 'false' ? false : pe.dataset.v; const r = rigLocal(a + ':' + b, k, v); if (r) rigSend(...r); return; }
    const kf = e.target.closest('[data-kfocus]'); if (kf) { const el = document.querySelector(`[data-dial="${CSS.escape(kf.dataset.kfocus)}"]`); if (el) el.focus(); return; }
    if (e.target.closest('.rpop')) return;
    const rs = e.target.closest('[data-rsel]'); if (rs) { S.rigSel = S.rigSel === rs.dataset.rsel ? null : rs.dataset.rsel; renderGraph(); renderRight(); return; }
    if (S.rigSel) { S.rigSel = null; renderGraph(); renderRight(); } return; }
  const so = e.target.closest('[data-solo]'); if (so) { e.stopPropagation(); toggleSolo(so.dataset.solo); return; }
  const n = e.target.closest('[data-node]'); if (n) { S.sel = n.dataset.node; S.tab = 'node'; renderGraph(); renderRight(); } };

// knobs
$('#dock').addEventListener('pointerdown', e => { const k = e.target.closest('[data-dial]'); if (!k) return; e.preventDefault(); k.setPointerCapture(e.pointerId); const m = macroOf(k.dataset.dial); KD = { name: m.name, y: e.clientY, v: m.value, from: m.value }; $('#kpop').hidden = true; });
$('#dock').addEventListener('pointermove', e => { if (!KD) return; setMacro(KD.name, KD.v + (KD.y - e.clientY) / (e.shiftKey ? 600 : 150)); });
$('#dock').addEventListener('pointerup', () => { if (!KD) return; const k = KD; KD = null; finishMacro(k.name, k.from); });
$('#dock').addEventListener('dblclick', e => { const k = e.target.closest('[data-dial]'); if (!k) return; const m = macroOf(k.dataset.dial); const from = m.value; setMacro(m.name, m.default); finishMacro(m.name, from); });
$('#dock').addEventListener('wheel', e => {
  const k = e.target.closest('[data-dial]');
  // The wheel scrolls the macro grid; it turns a dial only once that dial has been clicked (focused).
  if (k && document.activeElement !== k) return;
  if (!k) return; e.preventDefault(); const m = macroOf(k.dataset.dial); if (!KD) KD = { name: m.name, from: m.value, wheel: true }; setMacro(m.name, m.value - e.deltaY * 0.0015); clearTimeout(KD.t); KD.t = setTimeout(() => { const kk = KD; KD = null; finishMacro(kk.name, kk.from); }, 350); }, { passive: false });
$('#dock').addEventListener('keydown', e => { const k = e.target.closest('[data-dial]'); if (!k || !['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(e.key)) return; e.preventDefault(); e.stopPropagation(); const m = macroOf(k.dataset.dial); const from = m.value; setMacro(m.name, m.value + (/Up|Right/.test(e.key) ? 1 : -1) * (e.shiftKey ? 0.01 : 0.05)); finishMacro(m.name, from).then(() => requestAnimationFrame(() => document.querySelector(`[data-dial="${m.name}"]`)?.focus())); });
$('#dock').addEventListener('mouseover', e => { const k = e.target.closest('[data-knob]'); if (!k || KD) return; if (S.hot !== k.dataset.knob) { S.hot = k.dataset.knob; document.querySelectorAll('.knob').forEach(x => x.classList.toggle('hot', x.dataset.knob === S.hot)); renderGraph(); } showKpop(k.dataset.knob, k); });
$('#dock').addEventListener('mouseleave', () => { if (KD) return; S.hot = null; $('#kpop').hidden = true; document.querySelectorAll('.knob').forEach(x => x.classList.remove('hot')); renderGraph(); });
$('#dock').onclick = async e => { if (e.target.closest('[data-kreset]')) { await act('reset_knobs', {}); toast('Knobs back at the agent\'s defaults'); } };

// right pane
$('#right').onclick = e => {
  const t = e.target.closest('[data-tab]'); if (t) { S.tab = t.dataset.tab; renderRight(); return; }
  const rp = e.target.closest('[data-rigpick]'); if (rp) { openRig(S.sel, rp.dataset.rigpick); return; }
  const ro = e.target.closest('[data-rigopen]'); if (ro) { openRig(ro.dataset.rigopen); return; }
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
  const ld = e.target.closest('[data-load]'); if (ld) { try { await sounder.call('catalog_load', { name: ld.dataset.load }); await openSound(ld.dataset.load); } catch (er) { toast(errText(er)); } return; }
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
  else if (e.key === 'Escape') { $('#docpop').hidden = true; $('#bpop').hidden = true; if (S.openRig && S.rigSel && S.centre === 'graph') { S.rigSel = null; renderGraph(); renderRight(); } else if (S.openRig && S.centre === 'graph') closeRig(); else if (S.region) { S.region = null; showRegbar(); } else if (S.zoom) { S.zoom = null; renderCentre(); } else if (ST.solo) solo(null); }
});
document.addEventListener('mousedown', e => { if (!e.target.closest('#docpop, #docbtn')) $('#docpop').hidden = true; if (!e.target.closest('#bpop, [data-hist]')) $('#bpop').hidden = true; });

// ---------- boot ----------
async function loadFonts() {
  const faces = [['IBM Plex Sans', 400, 'IBMPlexSans-Regular.ttf'], ['IBM Plex Sans', 500, 'IBMPlexSans-Medium.ttf'], ['IBM Plex Sans', 600, 'IBMPlexSans-SemiBold.ttf'], ['IBM Plex Mono', 400, 'IBMPlexMono-Regular.ttf']];
  await Promise.all(faces.map(async ([fam, w, file]) => { try { const f = new FontFace(fam, `url(${FONT_BASE}${file})`, { weight: String(w) }); await f.load(); document.fonts.add(f); } catch (e) { /* falls back to system-ui */ } }));
}
function loadScript(src) { return new Promise((res, rej) => { const s = document.createElement('script'); s.src = src; s.onload = res; s.onerror = () => rej(new Error('could not load ' + src)); document.head.append(s); }); }
// =====================================================================
// Editing: everything a person can do without an agent
// =====================================================================
// A person's edits are agents' calls: sound_ops (and new_sound, catalog, export) through the `tool` window call,
// so the server parses and checks them and each is one history step. Placements go through `layout`. The strip
// at the bottom left shows the last call as an agent would send it.
let TYPES = null;                                   // node_types: [{type, blurb, example, schema}]
const TYPE = t => (TYPES || []).find(x => x.type === t);
const NAME_RE = /^[a-z_][a-z0-9_]*$/i;
const RESERVED = new Set(['mono', 'stereo']);
const freeName = (base, taken) => { base = base.replace(/[^a-z0-9_]/gi, '_'); if (!taken.has(base)) return base; let i = 2; while (taken.has(base + '_' + i)) i++; return base + '_' + i; };
const nodeNames = () => new Set(doc().nodes.map(n => n.name));
const opOf = n => ({ type: n.type, name: n.name, ...clone(n.fields) });
const plainMacro = m => { const { result, ...r } = clone(m); r.map = r.map.map(e => Object.fromEntries(Object.entries(e).filter(([, v]) => v != null))); if (r.claim == null) delete r.claim; return r; };
let LASTCALL = null;
function renderOpslog() { const el = $('#opslog'); if (!el) return; el.hidden = !LASTCALL || S.view === 'grid'; if (!LASTCALL) return;
  el.innerHTML = `<b>${esc(LASTCALL.tool)}</b>${esc(el.classList.contains('open') ? JSON.stringify(LASTCALL.args, null, 1) : JSON.stringify(LASTCALL.args))}`; }
async function tool(name, args) { LASTCALL = { tool: name, args }; renderOpslog(); return act('tool', { name, args }); }
const ops = args => tool('sound_ops', { doc: ST.doc, ...args });
async function placeAt(layout) { LASTCALL = { tool: 'layout', args: layout }; renderOpslog(); return act('layout', { layout }); }

// ---------- schemas: what kind of value a field takes ----------
function fieldSchema(type, k) { const t = TYPE(type); if (!t || !t.schema) return null; const S0 = t.schema, defs = S0.$defs || S0.definitions || {};
  const deref = s => { let g = 0; while (s && s.$ref && g++ < 8) s = defs[s.$ref.split('/').pop()]; return s; };
  let p = (S0.properties || {})[k]; if (!p) return null; const desc = p.description || '';
  if (p.$ref && /\/In$/.test(p.$ref)) return { kind: 'in', desc };
  if (p.anyOf) { const alts = p.anyOf.filter(a => a.type !== 'null'); if (alts.length === 1) { if (alts[0].$ref && /\/In$/.test(alts[0].$ref)) return { kind: 'in', desc, optional: true }; p = { ...alts[0], description: desc }; } }
  const r = deref(p) || {};
  if (r.enum) return { kind: 'enum', options: r.enum, desc };
  if (r.oneOf && r.oneOf.every(o => o.enum || o.const)) return { kind: 'enum', options: r.oneOf.flatMap(o => o.enum || [o.const]), desc };
  const ty = Array.isArray(r.type) ? r.type.filter(x => x !== 'null')[0] : r.type;
  if (ty === 'number' || ty === 'integer') return { kind: 'num', int: ty === 'integer', desc };
  if (ty === 'boolean') return { kind: 'bool', desc };
  if (ty === 'string') return { kind: 'str', desc };
  return { kind: 'json', desc }; }
const inFields = type => { const t = TYPE(type); return t && t.schema ? Object.keys(t.schema.properties || {}).filter(k => (fieldSchema(type, k) || {}).kind === 'in') : []; };

// ---------- references between nodes, as paths into a node's fields ----------
const ENUMK = new Set(['type', 'op', 'mode', 'shape', 'curve', 'color', 'material', 'floor', 'kind', 'to', 'surface', 'call', 'layer', 'preset']);
function refPaths(n) { const names = nodeNames(), out = [];
  const isRef = s => typeof s === 'string' && !/^[$=]/.test(s) && !/\s/.test(s) && names.has(s.split('.')[0]) && s.split('.')[0] !== n.name;
  const walk = (v, path) => { const k = path[path.length - 1];
    if (typeof v === 'string') { if (isRef(v) && !ENUMK.has(path[0]) && !(path[0] === 'bodies' && k === 'name') && !(path[0] === 'joints' && (k === 'a' || k === 'b'))) out.push({ path, value: v }); }
    else if (Array.isArray(v)) v.forEach((x, i) => walk(x, path.concat(i)));
    else if (v && typeof v === 'object') Object.entries(v).forEach(([kk, x]) => walk(x, path.concat(kk))); };
  Object.entries(n.fields).forEach(([k, v]) => walk(v, [k])); return out; }
function setPath(o, path, v) { let p = o; for (const k of path.slice(0, -1)) p = p[k]; const last = path[path.length - 1];
  if (v != null) { p[last] = v; return; }
  if (Array.isArray(p)) p[last] = undefined; else delete p[last]; }
const tidyArrays = o => { for (const [k, v] of Object.entries(o)) if (Array.isArray(v) && v.includes(undefined)) o[k] = v.filter(x => x !== undefined); return o; };
// a disconnected input becomes 0 (silence), so the node stays valid (`mul` without `b` is an error)
function unref(op, path) { const top = path[0];
  if (path.length === 1 && (fieldSchema(op.type, top) || {}).kind === 'in') op[top] = 0;
  else if (path.length === 2 && top === 'inputs') setPath(op, path, null);
  else if (path.length >= 2 && typeof path[path.length - 2] === 'number' && path[path.length - 1] === 'input') setPath(op, path.slice(0, -1), null);
  else setPath(op, path, null);
  return tidyArrays(op); }

// ---------- signal graph ----------
function defaultsFor(type) { const t = TYPE(type), ex = clone((t && t.example) || {}); delete ex.type; delete ex.name; const names = nodeNames();
  for (const [k, v] of Object.entries(ex)) { const fs = fieldSchema(type, k) || {}; if (fs.kind === 'in' && typeof v === 'string' && !/^[$=]/.test(v) && !names.has(v.split('.')[0])) ex[k] = 0;
    else if (k === 'inputs' && Array.isArray(v)) ex[k] = []; else if (Array.isArray(v) && v.some(x => typeof x === 'string' && names.has(x) === false && /^[a-z_]+$/i.test(x) && fs.kind !== 'json')) delete ex[k]; }
  for (const k of (t?.schema?.required || [])) if (!(k in ex)) { const fs = fieldSchema(type, k) || {}; ex[k] = fs.kind === 'in' ? 0 : fs.kind === 'enum' ? fs.options[0] : fs.kind === 'num' ? 0 : fs.kind === 'bool' ? false : k === 'inputs' ? [] : null; if (ex[k] == null) delete ex[k]; }
  return ex; }
async function addNode(type, at) { const name = freeName(type, nodeNames()), op = { type, name, ...defaultsFor(type) };
  // a sound needs an output: the first node of an empty sound comes with one, reading it
  const extra = type !== 'output' && !doc().nodes.some(n => n.type === 'output') ? [{ type: 'output', name: freeName('out', new Set([...nodeNames(), name])), input: name }] : [];
  const r = await ops({ ops: [op, ...extra] }); if (r === undefined) return; S.sel = name; S.tab = 'node'; if (at) await placeAt({ nodes: { [name]: at.map(Math.round) } }); }
function openNodePalette(x, y, at) { if (!TYPES) return toast('loading node types…');
  openPal({ x, y, title: 'Add a node', search: true, items: TYPES.filter(t => t.type !== 'output').map(t => ({ label: t.type, hint: (t.blurb || '').split(/[;:(]/)[0], act: () => addNode(t.type, at) })) }); }
function connect(src, tname, cx, cy) { const t = nodeOf(tname); if (!t || tname === src.split('.')[0]) return; const op = opOf(t);
  if (t.type === 'mix') return ops({ ops: [{ ...op, inputs: [...(op.inputs || []), src] }] });
  let items;
  if (t.type === 'rig') items = (op.bodies || []).map((b, i) => ({ label: `${b.name} drive`, hint: b.drive || '', act: () => { op.bodies[i].drive = src; ops({ ops: [op] }); } }))
    .concat((op.joints || []).map((j, i) => j.kind === 'contact' ? { label: `${j.a}↔${j.b} slide`, hint: j.slide || '', act: () => { op.joints[i].slide = src; ops({ ops: [op] }); } } : null).filter(Boolean));
  else items = inFields(t.type).map(k => ({ label: k, hint: op[k] == null ? 'empty' : String(op[k]), act: () => ops({ ops: [{ ...op, [k]: src }] }) }));
  if (!items.length) return toast(`${tname} takes no signal`);
  if (items.length === 1) return items[0].act();
  openPal({ x: cx, y: cy, title: `${src} → ${tname}`, items }); }
function disconnect(id) { const [node, ...p] = id.split('|'), n = nodeOf(node); if (!n) return; const path = p.map(x => /^\d+$/.test(x) ? +x : x); S.wireSel = null; ops({ ops: [unref(opOf(n), path)] }); }
function deleteNodes(names) { const d = doc(); names = names.filter(nm => nodeOf(nm) && nodeOf(nm).type !== 'output'); if (!names.length) return toast('the output stays: a sound has exactly one');
  const gone = new Set(names), list = names.map(name => ({ type: 'delete', name }));
  d.nodes.filter(n => !gone.has(n.name)).forEach(n => { const rs = refPaths(n).filter(r => gone.has(r.value.split('.')[0])); if (!rs.length) return; let op = opOf(n); rs.sort((a, b) => String(b.path).localeCompare(String(a.path))).forEach(r => { op = unref(op, r.path); }); list.push(op); });
  const macros = d.macros.filter(m => m.map.some(e => gone.has(e.node))).map(m => ({ ...plainMacro(m), map: plainMacro(m).map.filter(e => !gone.has(e.node)) }));
  S.multi = null; ops({ ops: list, ...(macros.length ? { macros } : {}) }); }
async function duplicateNodes(names) { const taken = nodeNames(), list = [], lay = {};
  names.forEach(nm => { const n = nodeOf(nm); if (!n || n.type === 'output') return; const op = opOf(n); op.name = freeName(nm, taken); taken.add(op.name); list.push(op); const p = cardPos(nm); if (p) lay[op.name] = [p[0] + 30, p[1] + 30]; });
  if (!list.length) return; if (await ops({ ops: list }) === undefined) return; S.sel = list[0].name; if (Object.keys(lay).length) placeAt({ nodes: lay }); }
async function renameNode(old, nu) { nu = nu.trim(); if (old === nu) return true; if (!NAME_RE.test(nu) || RESERVED.has(nu)) { toast(`"${nu}" is not a valid node name`); return false; } if (nodeNames().has(nu)) { toast(`a node is already named ${nu}`); return false; }
  const d = doc(), list = [{ type: 'delete', name: old }];
  d.nodes.forEach(n => { const op = opOf(n); let ch = n.name === old; if (ch) op.name = nu; refPaths(n).forEach(r => { if (r.value.split('.')[0] === old) { ch = true; setPath(op, r.path, nu + r.value.slice(old.length)); } }); if (ch) list.push(op); });
  const macros = d.macros.filter(m => m.map.some(e => e.node === old)).map(m => { const pm = plainMacro(m); pm.map = pm.map.map(e => e.node === old ? { ...e, node: nu } : e); return pm; });
  if (await ops({ ops: list, ...(macros.length ? { macros } : {}) }) === undefined) return false;
  const lo = d.layout || {}; if (lo.nodes?.[old] || lo.rigs?.[old]) placeAt({ nodes: { [old]: null, ...(lo.nodes?.[old] ? { [nu]: lo.nodes[old] } : {}) }, rigs: { [old]: null, ...(lo.rigs?.[old] ? { [nu]: lo.rigs[old] } : {}) } });
  if (S.sel === old) S.sel = nu; if (S.openRig === old) S.openRig = nu; return true; }
function cardPos(name) { const el = document.querySelector(`#graphhost .gnode[data-node="${CSS.escape(name)}"]`); return el ? [parseFloat(el.style.left), parseFloat(el.style.top)] : (doc().layout?.nodes?.[name] || null); }
function graphXY(e) { const g = $('#graphhost .graph') || $('#graphhost .rig'); const r = g.getBoundingClientRect(); return [e.clientX - r.left, e.clientY - r.top]; }
function openAddAtCentre() { if (S.view === 'grid') return; if (S.centre !== 'graph') { S.centre = 'graph'; renderCentre(); }
  const h = $('#graphhost').getBoundingClientRect();
  if (S.openRig) return openPal({ x: h.left + h.width / 2 - 140, y: h.top + 60, title: 'Add a body', items: [{ label: 'point mass', act: () => addBody('mass') }, { label: 'modal object', act: () => addBody('modal') }] });
  const cards = [...document.querySelectorAll('#graphhost .gnode')], y = cards.length ? Math.max(...cards.map(c => parseFloat(c.style.top) + c.offsetHeight)) + 30 : 20;
  openNodePalette(h.left + h.width / 2 - 140, h.top + 60, [0, y]); }

// ---------- rig: bodies and joints ----------
const rigOp = () => { const n = nodeOf(S.openRig); return n && n.type === 'rig' ? opOf(n) : null; };
async function addBody(kind) { const op = rigOp(); if (!op) return; op.bodies ||= []; op.joints ||= []; const nm = freeName(kind === 'mass' ? 'mass' : 'body', new Set(op.bodies.map(b => b.name)));
  op.bodies.push(kind === 'mass' ? { name: nm, mass: 0.1 } : { name: nm, mass: 0.5, material: 'wood', shape: 'plate', f0: 300, loss: 1.2, noise: 0.6 });
  if (await ops({ ops: [op] }) !== undefined) S.rigSel = 'b:' + nm; }
async function addJoint(kind, a, b, at_a, at_b) { const op = rigOp(); if (!op) return; op.joints ||= [];
  const j = { kind, a, b, at_a: +at_a.toFixed(2), ...(b === 'ground' ? {} : { at_b: +at_b.toFixed(2) }) }; Object.assign(j, kind === 'contact' ? { bounce: 0.4, ...(b === 'ground' ? { gap: 0.3 } : {}) } : { stiffness: 2e5, damping: 2 });
  op.joints.push(j); if (await ops({ ops: [op] }) !== undefined) S.rigSel = 'j' + (op.joints.length - 1); }
function deleteRigPart(id) { const op = rigOp(); if (!op) return; const rig = op.name; let macros = doc().macros.map(plainMacro);
  const reindex = (field, removed) => { macros = macros.map(m => ({ ...m, map: m.map.filter(e => !(e.node === rig && e.field === field && removed.includes(e.index))).map(e => e.node === rig && e.field === field && e.index != null ? { ...e, index: e.index - removed.filter(r => r < e.index).length } : e) })); };
  const before = JSON.stringify(macros);
  if (id.startsWith('b:')) { const name = id.slice(2), bi = op.bodies.findIndex(b => b.name === name), jr = op.joints.map((j, i) => j.a === name || j.b === name ? i : -1).filter(i => i >= 0);
    op.bodies.splice(bi, 1); op.joints = op.joints.filter((j, i) => !jr.includes(i)); reindex('bodies', [bi]); reindex('joints', jr); }
  else { const ji = +id.slice(1); op.joints.splice(ji, 1); reindex('joints', [ji]); }
  const orig = doc().macros.map(plainMacro), changed = macros.filter((m, i) => JSON.stringify(m) !== JSON.stringify(orig[i]));
  S.rigSel = null; ops({ ops: [op], ...(JSON.stringify(macros) !== before && changed.length ? { macros: changed } : {}) }); }
function renameBody(old, nu) { nu = nu.trim(); if (old === nu) return; const op = rigOp(); if (!op) return; if (!NAME_RE.test(nu) || nu === 'ground') return toast(`"${nu}" is not a valid body name`); if (op.bodies.some(b => b.name === nu)) return toast(`a body is already named ${nu}`);
  op.bodies.forEach(b => { if (b.name === old) b.name = nu; }); op.joints.forEach(j => { if (j.a === old) j.a = nu; if (j.b === old) j.b = nu; });
  ops({ ops: [op] }).then(r => { if (r === undefined) return; S.rigSel = 'b:' + nu; const lo = doc().layout?.rigs?.[op.name]?.[old]; if (lo) placeAt({ rigs: { [op.name]: { [old]: null, [nu]: lo } } }); }); }

// ---------- knobs: create, edit, map ----------
function knobEntryTarget(el) {
  if (el.dataset.num) { const n = nodeOf(S.sel), k = el.dataset.num, s = slider(k); return n && { ent: { node: n.name, field: k }, cur: n.fields[k], val: t => { const v = s.to ? s.to(t) : t; return Math.abs(v) >= 100 ? Math.round(v) : +v.toFixed(4); } }; }
  if (el.dataset.rp) { const [a, b, k] = el.dataset.rp.split(':'), id = a === 'b' ? a + ':' + b : a, key = a === 'b' ? k : b, n = nodeOf(S.openRig); if (!n) return null; const f = n.fields;
    const field = id.startsWith('b:') ? 'bodies' : 'joints', index = id.startsWith('b:') ? f.bodies.findIndex(q => 'b:' + q.name === id) : +id.slice(1), fk = key === 'gap_mm' ? 'gap' : key;
    return { ent: { node: n.name, field, index, key: fk }, cur: f[field][index][fk] ?? RDEF[fk], val: t => key === 'gap_mm' ? +(rfVal(key, t) / 1000).toFixed(5) : rfVal(key, t) }; }
  return null; }
function armMap(name) { S.mapArm = { name, entry: null }; document.body.classList.add('mapping'); const b = $('#mapbar'); b.hidden = false; b.innerHTML = `<span>Mapping <b>◎ ${esc(name)}</b></span><span class="hint">drag a slider to the knob's top value</span><button data-mapdone>Done</button>`; if (S.tab !== 'node') { S.tab = 'node'; renderRight(); } renderDock(); }
function disarmMap() { S.mapArm = null; document.body.classList.remove('mapping'); $('#mapbar').hidden = true; renderDock(); }
const saveMacro = (m, oldName) => ops({ ...(oldName && oldName !== m.name ? { delete_macros: [oldName] } : {}), macros: [m] });
async function newKnob() { const name = freeName('knob', new Set(doc().macros.map(m => m.name)));
  if (await saveMacro({ name, value: 0.5, default: 0.5, doc: '', game: false, map: [] }) === undefined) return; KEDIT = name; armMap(name); setTimeout(() => openKedit(name), 60); }
let KEDIT = null;
function openKedit(name) { const m = doc().macros.find(x => x.name === name), el = $('#kedit'); if (!m) { el.hidden = true; KEDIT = null; return; } KEDIT = name;
  const k = document.querySelector(`.knob[data-knob="${CSS.escape(name)}"]`), r = k ? k.getBoundingClientRect() : { left: 20, top: innerHeight - 200 };
  el.innerHTML = `<div class="keh"><input data-kname value="${esc(m.name)}" spellcheck="false"><button data-kclose>${ic('x')}</button></div>
    <input class="kdoc" data-kdoc value="${esc(m.doc || '')}" placeholder="what it does">
    <div class="krow"><span>claim</span><select data-kclaim><option value="">none</option>${Object.keys(CLAIMS).concat(['inharmonicity', 'roughness', 'density', 'attack', 'spacing', 'ring']).map(c => `<option ${m.claim && m.claim[0] === c ? 'selected' : ''}>${c}</option>`).join('')}</select>${m.claim ? `<div class="opts"><button data-kdir="1" aria-pressed="${m.claim[1] > 0}">rises</button><button data-kdir="-1" aria-pressed="${m.claim[1] < 0}">falls</button></div>` : ''}</div>
    <label class="krow"><span>game</span><input type="checkbox" data-kgame ${m.game ? 'checked' : ''}></label>
    <div class="kmap">${m.map.length ? m.map.map((e, i) => `<div class="kme"><span class="mono" title="${esc(mapText(e)[0])}">${esc(mapText(e)[0])}</span><input type="number" step="any" data-kfrom="${i}" value="${+(+e.from).toFixed(5)}"><span>→</span><input type="number" step="any" data-kto="${i}" value="${+(+e.to).toFixed(5)}"><select data-kcurve="${i}"><option ${e.curve !== 'exp' ? 'selected' : ''}>linear</option><option ${e.curve === 'exp' ? 'selected' : ''}>exp</option></select><button data-kdel="${i}" title="Remove">${ic('x')}</button></div>`).join('') : '<div class="hint">moves nothing yet</div>'}</div>
    <div class="kfoot"><button class="tbtn" data-karm aria-pressed="${S.mapArm?.name === name}">${ic('pin')}Map a slider</button><button class="tbtn" data-kdef title="Make where it sits the default">Default here</button><span class="grow"></span><button class="tbtn danger" data-kremove>Delete</button></div>`;
  el.hidden = false; el.style.left = Math.max(8, Math.min(innerWidth - 400, r.left)) + 'px'; el.style.top = Math.max(8, r.top - el.offsetHeight - 10) + 'px'; }
$('#kedit').addEventListener('change', async e => { const name = KEDIT, m0 = doc().macros.find(x => x.name === name); if (!m0) return; const m = plainMacro(m0), t = e.target;
  if (t.matches('[data-kname]')) { const nu = t.value.trim(); if (!NAME_RE.test(nu)) { toast(`"${nu}" is not a valid knob name`); t.value = name; return; } if (await saveMacro({ ...m, name: nu }, name) === undefined) { t.value = name; return; } if (S.mapArm?.name === name) S.mapArm.name = nu; KEDIT = nu; return; }
  if (t.matches('[data-kdoc]')) m.doc = t.value;
  else if (t.matches('[data-kclaim]')) { if (t.value) m.claim = [t.value, m.claim ? m.claim[1] : 1]; else delete m.claim; }
  else if (t.matches('[data-kgame]')) m.game = t.checked;
  else if (t.dataset.kfrom != null) m.map[+t.dataset.kfrom].from = +t.value;
  else if (t.dataset.kto != null) m.map[+t.dataset.kto].to = +t.value;
  else if (t.dataset.kcurve != null) m.map[+t.dataset.kcurve].curve = t.value;
  else return;
  saveMacro(m); });
$('#kedit').addEventListener('click', e => { const name = KEDIT, m0 = doc().macros.find(x => x.name === name); if (!m0) return; const m = plainMacro(m0), t = e.target;
  if (t.closest('[data-kclose]')) { $('#kedit').hidden = true; KEDIT = null; disarmMap(); return; }
  if (t.closest('[data-kdir]')) { m.claim = [m.claim[0], +t.closest('[data-kdir]').dataset.kdir]; saveMacro(m); return; }
  if (t.closest('[data-kdel]')) { m.map.splice(+t.closest('[data-kdel]').dataset.kdel, 1); saveMacro(m); return; }
  if (t.closest('[data-karm]')) { S.mapArm?.name === name ? disarmMap() : armMap(name); openKedit(name); return; }
  if (t.closest('[data-kdef]')) { m.default = m.value; saveMacro(m); return; }
  if (t.closest('[data-kremove]')) { disarmMap(); $('#kedit').hidden = true; KEDIT = null; ops({ delete_macros: [name] }); } });
$('#kedit').addEventListener('keydown', e => { if (e.key === 'Enter' && e.target.matches('input')) e.target.blur(); });
$('#mapbar').addEventListener('click', e => { if (e.target.closest('[data-mapdone]')) { disarmMap(); if (KEDIT) openKedit(KEDIT); } });
// map mode: a slider press starts a map entry and the drag sets its `to`; the node itself is not edited
document.addEventListener('pointerdown', e => { if (!S.mapArm) return; const el = e.target.closest('input[type=range][data-num], input[type=range][data-rp]'); if (!el) return;
  const tg = knobEntryTarget(el); if (!tg || typeof tg.cur !== 'number') { toast('that field takes a number'); return; } S.mapArm.entry = { ...tg.ent, from: tg.cur, to: tg.cur }; S.mapArm.val = tg.val; S.mapArm.el = el; }, true);
document.addEventListener('input', e => { const a = S.mapArm; if (!a || !a.entry || e.target !== a.el) return; e.stopImmediatePropagation(); a.entry.to = a.val(+e.target.value);
  const out = e.target.nextElementSibling; if (out) out.textContent = `${+(+a.entry.from).toFixed(4)} → ${+(+a.entry.to).toFixed(4)}`; }, true);
document.addEventListener('change', e => { const a = S.mapArm; if (!a || !a.entry || e.target !== a.el) return; e.stopImmediatePropagation(); dragRight = false; const m0 = doc().macros.find(x => x.name === a.name); if (!m0) return; const m = plainMacro(m0), ent = a.entry; a.entry = null;
  const same = x => x.node === ent.node && x.field === ent.field && (x.index ?? null) === (ent.index ?? null) && (x.key ?? null) === (ent.key ?? null);
  const exp = ent.from > 0 && ent.to > 0 && (ent.to / ent.from > 4 || ent.from / ent.to > 4);
  m.map = m.map.filter(x => !same(x)).concat([{ ...ent, curve: exp ? 'exp' : 'linear' }]);
  saveMacro(m).then(r => { if (r !== undefined) toast(`${a.name} now moves ${mapText(ent)[0]}`); }); }, true);

// ---------- sounds: new, duplicate, template, save, export ----------
function showEmpty() { if ($('#empty')) return; const el = document.createElement('div'); el.id = 'empty'; el.className = 'empty';
  el.innerHTML = `<div class="emptybox"><b>No sounds open</b><div class="sbtns"><button class="primary" data-enew>${ic('plus')}New sound</button><button class="tbtn" data-etpl>From template</button></div></div>`;
  el.onclick = e => { if (e.target.closest('[data-enew]')) newSound(); else if (e.target.closest('[data-etpl]')) newSound(null, 'door_slam'); }; document.body.appendChild(el); }
async function newSound(from, template) { const ids = new Set(((ST && ST.sounds) || []).map(s => s.name)); const name = freeName(template || (from ? from + '_copy' : 'sound'), ids);
  if (await tool('new_sound', { name, ...(template ? { template } : {}) }) === undefined) return;
  if (from) { const d = doc(); await tool('sound_ops', { doc: name, ops: d.nodes.map(opOf), macros: d.macros.map(plainMacro), params: d.params, kind: d.kind, ...(d.duration ? { duration: d.duration } : {}), seed: d.seed }); }
  $('#docpop').hidden = true; await openSound(name); S.centre = 'graph'; if (ST) renderCentre(); }
function saveDialog() { openDlg(`Save ${ST.doc} to the catalog`, [{ label: 'role', key: 'role', kind: 'select', options: ['', 'pre', 'crack', 'body', 'weight', 'after', 'bed', 'texture', 'whole'] }, { label: 'material', key: 'material', kind: 'select', options: ['', 'wood', 'metal', 'glass', 'stone', 'plastic', 'cloth', 'water', 'any'] }, { label: 'tags', key: 'tags' }, { label: 'description', key: 'description', kind: 'area' }], 'Save',
  v => tool('catalog', { action: 'save', doc: ST.doc, ...(v.role ? { role: v.role } : {}), ...(v.material ? { material: v.material } : {}), ...(v.tags ? { tags: v.tags.split(/[,\s]+/).filter(Boolean) } : {}), ...(v.description ? { description: v.description } : {}) }).then(r => { if (r) toast(r.text.split('\n')[0]); })); }
function exportDialog() { openDlg(`Export ${ST.doc}`, [{ label: 'format', key: 'format', kind: 'select', options: ['ogg', 'wav'], value: 'ogg' }, { label: 'ogg quality', key: 'quality', kind: 'number', value: 0.5, step: 0.1 }, { label: 'wav bits', key: 'bits', kind: 'select', options: ['16', '24', '32'], value: '24' }, { label: 'variations', key: 'variations', kind: 'number', value: 1, step: 1 }], 'Export',
  v => tool('export', { doc: ST.doc, ...(v.format === 'wav' ? { format: 'wav', bits: +v.bits } : { quality: v.quality }), ...(v.variations > 1 ? { variations: v.variations } : {}) }).then(r => { if (r) toast(r.text.split('\n')[0]); })); }
function stepHistory(dir) { const h = doc().history, i = h.findIndex(x => x.current), j = i + dir; if (i < 0 || j < 0 || j >= h.length) { toast(dir < 0 ? 'nothing to undo' : 'nothing to redo'); return; } act('jump', { id: h[j].id }); }

// ---------- small popovers: a menu / palette and a dialog ----------
let PAL = null;
function openPal({ x, y, title, items, search }) { closePal(); const el = document.createElement('div'); el.className = 'pal'; document.body.appendChild(el); PAL = { el, items, q: '', i: 0 };
  el.innerHTML = `${title ? `<div class="palh">${esc(title)}</div>` : ''}${search ? '<input placeholder="Search" autocomplete="off" spellcheck="false">' : ''}<div class="list"></div>`;
  const draw = () => { const q = PAL.q.toLowerCase(), score = t => !q ? 0 : t.label.toLowerCase().startsWith(q) ? 0 : t.label.toLowerCase().includes(q) ? 1 : 2;
    let its = PAL.items.filter(t => !q || (t.label + ' ' + (t.hint || '')).toLowerCase().includes(q)); if (q) its = its.map((t, i) => [t, i]).sort((a, b) => score(a[0]) - score(b[0]) || a[1] - b[1]).map(x => x[0]);
    PAL.vis = its; PAL.i = Math.min(PAL.i, Math.max(0, its.length - 1));
    el.querySelector('.list').innerHTML = its.slice(0, 200).map((t, i) => `<button data-pi="${i}" class="${i === PAL.i ? 'on' : ''}"><span>${esc(t.label)}</span>${t.hint ? `<span class="hint">${esc(t.hint)}</span>` : ''}</button>`).join('') || '<div class="hint" style="padding:6px 8px">nothing matches</div>'; };
  draw(); PAL.draw = draw;
  el.style.left = Math.max(8, Math.min(innerWidth - 300, x)) + 'px'; el.style.top = Math.max(8, Math.min(innerHeight - Math.min(400, el.offsetHeight) - 8, y)) + 'px';
  const inp = el.querySelector('input'); if (inp) { inp.focus(); inp.oninput = () => { PAL.q = inp.value; PAL.i = 0; draw(); }; } else { el.tabIndex = -1; el.focus(); }
  el.onkeydown = e => { if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); PAL.i = Math.max(0, Math.min(PAL.vis.length - 1, PAL.i + (e.key === 'ArrowDown' ? 1 : -1))); draw(); el.querySelector('.on')?.scrollIntoView({ block: 'nearest' }); }
    else if (e.key === 'Enter') { e.preventDefault(); const t = PAL.vis[PAL.i]; if (t) { closePal(); t.act(); } } };
  // a menu opened by a drop appears under the cursor: the drop's own click must not pick an item
  const born = performance.now(); el.onclick = e => { if (performance.now() - born < 300) return; const b = e.target.closest('[data-pi]'); if (!b) return; const t = PAL.vis[+b.dataset.pi]; closePal(); t.act(); }; }
function closePal() { if (PAL) { PAL.el.remove(); PAL = null; } }
document.addEventListener('mousedown', e => { if (PAL && !e.target.closest('.pal')) closePal(); if (!e.target.closest('#kedit, .knob, #mapbar, .pal') && !$('#kedit').hidden && !S.mapArm) { $('#kedit').hidden = true; KEDIT = null; } });
function openDlg(title, fields, primary, onOk) { const el = $('#dlg');
  el.innerHTML = `<div class="dlgbox"><div class="dlgh">${esc(title)}<button data-dlgx>${ic('x')}</button></div>${fields.map(f => `<label class="dfield"><span>${esc(f.label)}</span>${f.kind === 'select' ? `<select data-dk="${f.key}">${f.options.map(o => `<option ${o === f.value ? 'selected' : ''}>${o}</option>`).join('')}</select>` : f.kind === 'area' ? `<textarea data-dk="${f.key}" rows="3"></textarea>` : `<input data-dk="${f.key}" type="${f.kind || 'text'}" value="${f.value ?? ''}" ${f.step ? `step="${f.step}"` : ''}>`}</label>`).join('')}<div class="dlgf"><button class="tbtn" data-dlgx>Cancel</button><button class="primary" data-dlgok>${esc(primary)}</button></div></div>`;
  el.hidden = false; el.onclick = e => { if (e.target === el || e.target.closest('[data-dlgx]')) { el.hidden = true; return; } if (e.target.closest('[data-dlgok]')) { const v = Object.fromEntries([...el.querySelectorAll('[data-dk]')].map(i => [i.dataset.dk, i.type === 'number' ? +i.value : i.value])); el.hidden = true; onOk(v); } };
  el.querySelector('[data-dk]')?.focus(); }

// ---------- inspector field editors, from each type's schema ----------
function fieldEd(n, k, v) { const d = doc();
  if (d.macros.some(m => m.map.some(x => x.node === n.name && x.field === k && x.index == null))) return fieldRow(n, k, v);
  const fs = fieldSchema(n.type, k) || { kind: typeof v === 'number' ? 'num' : 'json' }, L = `<label title="${esc(fs.desc || k)}">${esc(k)}</label>`, opt = !(TYPE(n.type)?.schema?.required || []).includes(k);
  const x = opt ? `<button class="x" data-funset="${esc(k)}" title="Unset (back to the default)">×</button>` : '';
  if (fs.kind === 'enum') return `<div class="field">${L}<div class="opts">${fs.options.map(o => `<button data-enum="${esc(k)}" data-v="${esc(o)}" aria-pressed="${o === v}">${esc(o)}</button>`).join('')}</div></div>`;
  if (fs.kind === 'in') {
    if (typeof v === 'number') { const s = slider(k), pos = s.from ? s.from(Math.max(v, 1e-6)) : v; return `<div class="field">${L}<input type="range" data-num="${esc(k)}" min="${s.min}" max="${s.max}" step="${s.step}" value="${pos}"><span class="val"><span data-numv="${esc(k)}">${s.fmt(v)}</span><button class="x" data-fsig="${esc(k)}" title="From a node or an expression">⇄</button></span></div>`; }
    if (typeof v === 'string' && nodeNames().has(v.split('.')[0])) return `<div class="field">${L}<div class="link"><button data-goto="${esc(v)}">← ${esc(v)}</button><button class="x" data-fclear="${esc(k)}" title="Disconnect">×</button></div></div>`;
    return `<div class="field">${L}<input class="fexpr" data-ftxt="${esc(k)}" value="${esc(typeof v === 'string' ? v : JSON.stringify(v))}" spellcheck="false"><button class="x" data-fsig="${esc(k)}" title="From a node or a number">⇄</button></div>`; }
  if (fs.kind === 'num') return `<div class="field">${L}<div class="link"><input class="fnum" type="number" step="any" data-fbox="${esc(k)}" value="${v}">${x}</div></div>`;
  if (fs.kind === 'bool') return `<div class="field">${L}<div class="link"><input type="checkbox" data-fbool="${esc(k)}" ${v ? 'checked' : ''}>${x}</div></div>`;
  if (fs.kind === 'str') return `<div class="field">${L}<div class="link"><input class="fexpr" data-fstr="${esc(k)}" value="${esc(v)}" spellcheck="false">${x}</div></div>`;
  return `<div class="field">${L}<textarea class="fjson" data-fjson="${esc(k)}" rows="1" spellcheck="false">${esc(JSON.stringify(v))}</textarea></div>`; }
function fieldRows(n) { const skip = k => isRigish(n) && (k === 'bodies' || k === 'joints'); const set = Object.keys(n.fields).filter(k => !skip(k));
  const props = Object.keys(TYPE(n.type)?.schema?.properties || {}).filter(k => !(k in n.fields) && !skip(k));
  return set.map(k => fieldEd(n, k, n.fields[k])).join('') + (props.length ? `<div class="field"><label></label><div class="link"><select class="faddsel" data-fadd><option value="">+ field</option>${props.map(p => `<option title="${esc(fieldSchema(n.type, p)?.desc || '')}">${esc(p)}</option>`).join('')}</select></div></div>` : ''); }
function sourceMenu(e, name, k) { const n = nodeOf(name), r = e.target.getBoundingClientRect();
  const items = doc().nodes.filter(x => x.name !== name && x.type !== 'output').map(x => ({ label: x.name, hint: x.type, act: () => act('set_field', { node: name, field: k, value: x.name }) }));
  doc().nodes.filter(x => isRigish(x) && x.name !== name).forEach(x => items.push({ label: x.name + '.force', hint: 'port', act: () => act('set_field', { node: name, field: k, value: x.name + '.force' }) }));
  items.unshift({ label: 'a number', act: () => act('set_field', { node: name, field: k, value: typeof n.fields[k] === 'number' ? n.fields[k] : 1 }) }, { label: 'an expression', hint: '=… or $param', act: () => act('set_field', { node: name, field: k, value: '=1' }) });
  openPal({ x: r.left - 120, y: r.bottom + 4, title: `${name}.${k}`, items, search: items.length > 8 }); }
const fieldDefault = (type, k) => { const ex = TYPE(type)?.example, fs = fieldSchema(type, k) || {}; if (ex && k in ex && !(fs.kind === 'in' && typeof ex[k] === 'string')) return clone(ex[k]); return fs.kind === 'enum' ? fs.options[0] : fs.kind === 'bool' ? true : fs.kind === 'str' ? '' : fs.kind === 'json' ? [] : fs.kind === 'in' ? 1 : 0; };

// ---------- editing gestures ----------
let GD = null, WD = null, MQ = null, BD = null, JD = null, SUPPRESS = false;
const editable = el => !!(el && el.closest && el.closest('input, textarea, select, [contenteditable]'));
$('#graphhost').addEventListener('pointerdown', e => { if (e.button !== 0) return; const t = e.target;
  if (S.openRig) { const rig = nodeOf(S.openRig); if (!rig || rig.type !== 'rig') return;
    const je = t.closest('[data-jedge]'); if (je) { e.preventDefault(); e.stopPropagation(); const [x, y] = graphXY(e), bx = je.closest('.rbody').getBoundingClientRect(); JD = { from: je.dataset.jedge, edge: je.classList.contains('top') ? 'top' : 'bot', x0: x, y0: y, at: Math.max(0, Math.min(1, (e.clientX - bx.left - 10) / (bx.width - 20))) }; renderGraph(); return; }
    const rb = t.closest('.rbody'); if (rb && !t.closest('.ah')) { const [x, y] = graphXY(e); BD = { rig: rig.name, name: rb.dataset.rsel.slice(2), x0: x, y0: y, orig: [parseFloat(rb.style.left), parseFloat(rb.style.top)], pos: null }; }
    return; }
  const out = t.closest('[data-gout]'); if (out) { e.preventDefault(); e.stopPropagation(); const r = out.getBoundingClientRect(), g = $('#graphhost .graph').getBoundingClientRect(); WD = { src: out.dataset.gout, x0: r.left + r.width / 2 - g.left, y0: r.top + r.height / 2 - g.top }; out.classList.add('on'); return; }
  const card = t.closest('.gnode'); if (card && !t.closest('button, .rthumb, input')) { const nm = card.dataset.node, names = S.multi && S.multi.has(nm) ? [...S.multi] : [nm]; const [x, y] = graphXY(e); GD = { names, x0: x, y0: y, orig: Object.fromEntries(names.map(n => [n, cardPos(n)])), pos: null }; return; }
  if (!t.closest('.gnode, .whit, .gtool') && $('#graphhost .graph')) { const [x, y] = graphXY(e); MQ = { x0: x, y0: y, x, y, add: e.shiftKey }; } });
addEventListener('pointermove', e => {
  if (GD) { const [x, y] = graphXY(e), dx = x - GD.x0, dy = y - GD.y0; if (!GD.pos && Math.hypot(dx, dy) < 4) return; GD.pos = Object.fromEntries(GD.names.map(n => [n, [Math.max(0, GD.orig[n][0] + dx), Math.max(0, GD.orig[n][1] + dy)]])); S.gdrag = GD; renderGraph(); return; }
  if (WD) { const [x, y] = graphXY(e), tmp = $('#gtemp'); if (tmp) tmp.innerHTML = `<path d="M${WD.x0},${WD.y0} C${WD.x0 + 60},${WD.y0} ${x - 60},${y} ${x},${y}" stroke="#6fa8ff" stroke-width="2" fill="none" stroke-dasharray="5 4"/>`;
    document.querySelectorAll('.gnode.droptarget').forEach(c => c.classList.remove('droptarget')); const over = document.elementFromPoint(e.clientX, e.clientY)?.closest('.gnode'); if (over && over.dataset.node !== WD.src.split('.')[0]) over.classList.add('droptarget'); return; }
  if (MQ) { const [x, y] = graphXY(e); MQ.x = x; MQ.y = y; let m = $('#graphhost .marquee'); if (!m) { m = document.createElement('div'); m.className = 'marquee'; $('#graphhost .graph').appendChild(m); } Object.assign(m.style, { left: Math.min(x, MQ.x0) + 'px', top: Math.min(y, MQ.y0) + 'px', width: Math.abs(x - MQ.x0) + 'px', height: Math.abs(y - MQ.y0) + 'px' }); return; }
  if (BD) { const [x, y] = graphXY(e), dx = x - BD.x0, dy = y - BD.y0; if (!BD.pos && Math.hypot(dx, dy) < 4) return; BD.pos = [Math.max(0, BD.orig[0] + dx), Math.max(0, BD.orig[1] + dy)]; S.bdrag = BD; renderGraph(); return; }
  if (JD) { const [x, y] = graphXY(e), tmp = $('#rtemp'); if (tmp) tmp.innerHTML = `<line x1="${JD.x0}" y1="${JD.y0}" x2="${x}" y2="${y}" stroke="#6fa8ff" stroke-width="2" stroke-dasharray="5 4"/><circle cx="${x}" cy="${y}" r="4" fill="#6fa8ff"/>`;
    document.querySelectorAll('.rbody.droptarget, .rground.droptarget').forEach(c => c.classList.remove('droptarget')); const over = document.elementFromPoint(e.clientX, e.clientY)?.closest('.rbody, .rground'); if (over && over.dataset.rsel !== 'b:' + JD.from) over.classList.add('droptarget'); } });
addEventListener('pointerup', e => {
  if (GD) { const g = GD; GD = null; S.gdrag = null; if (g.pos) { SUPPRESS = true; placeAt({ nodes: Object.fromEntries(Object.entries(g.pos).map(([k, v]) => [k, v.map(Math.round)])) }); } return; }
  if (WD) { const w = WD; WD = null; document.querySelectorAll('.gnode.droptarget, .gout.on').forEach(c => c.classList.remove('droptarget', 'on')); const tmp = $('#gtemp'); if (tmp) tmp.innerHTML = '';
    const over = document.elementFromPoint(e.clientX, e.clientY)?.closest('.gnode'); if (over) connect(w.src, over.dataset.node, e.clientX, e.clientY); return; }
  if (MQ) { const q = MQ; MQ = null; $('#graphhost .marquee')?.remove(); const x0 = Math.min(q.x0, q.x), x1 = Math.max(q.x0, q.x), y0 = Math.min(q.y0, q.y), y1 = Math.max(q.y0, q.y);
    if (x1 - x0 < 4 && y1 - y0 < 4) { if (!q.add && (S.multi || S.wireSel)) { S.multi = null; S.wireSel = null; renderGraph(); } return; }
    const hit = [...document.querySelectorAll('#graphhost .gnode')].filter(c => { const l = parseFloat(c.style.left), t = parseFloat(c.style.top); return l < x1 && l + 150 > x0 && t < y1 && t + c.offsetHeight > y0; }).map(c => c.dataset.node);
    S.multi = new Set([...(q.add && S.multi ? S.multi : []), ...hit]); if (!S.multi.size) S.multi = null; else { S.sel = hit[0] || S.sel; S.tab = 'node'; } SUPPRESS = true; renderGraph(); renderRight(); return; }
  if (BD) { const b = BD; BD = null; S.bdrag = null; if (b.pos) { SUPPRESS = true; placeAt({ rigs: { [b.rig]: { [b.name]: b.pos.map(Math.round) } } }); } return; }
  if (JD) { const j = JD; JD = null; const over = document.elementFromPoint(e.clientX, e.clientY)?.closest('.rbody, .rground'); renderGraph(); if (!over) return; SUPPRESS = true;
    if (over.classList.contains('rground')) return openPal({ x: e.clientX, y: e.clientY, title: `${j.from} → ground`, items: [{ label: 'contact', act: () => addJoint('contact', j.from, 'ground', j.at, 0) }] });
    const to = over.dataset.rsel.slice(2); if (to === j.from) return; const br = over.getBoundingClientRect(), atTo = Math.max(0, Math.min(1, (e.clientX - br.left - 10) / (br.width - 20))), upper = j.edge === 'bot';
    const mk = kind => () => upper ? addJoint(kind, j.from, to, j.at, atTo) : addJoint(kind, to, j.from, atTo, j.at);
    openPal({ x: e.clientX, y: e.clientY, title: `${j.from} ↔ ${to}`, items: [{ label: 'contact', act: mk('contact') }, { label: 'spring', act: mk('spring') }] }); } });
$('#graphhost').addEventListener('click', e => { if (SUPPRESS) { SUPPRESS = false; e.stopPropagation(); return; } const t = e.target;
  if (t.closest('[data-gadd]')) { e.stopPropagation(); openAddAtCentre(); return; }
  if (t.closest('[data-gtidy]')) { e.stopPropagation(); placeAt(S.openRig ? { rigs: { [S.openRig]: null } } : { nodes: null }); return; }
  const ra = t.closest('[data-radd]'); if (ra) { e.stopPropagation(); addBody(ra.dataset.radd); return; }
  if (t.closest('[data-rpdel]')) { e.stopPropagation(); deleteRigPart(S.rigSel); return; }
  const w = t.closest('.whit'); if (w) { e.stopPropagation(); S.wireSel = S.wireSel === w.dataset.wire ? null : w.dataset.wire; renderGraph(); return; }
  const card = t.closest('.gnode'); if (card && e.shiftKey) { e.stopPropagation(); const nm = card.dataset.node; S.multi ||= new Set(S.sel ? [S.sel] : []); S.multi.has(nm) ? S.multi.delete(nm) : S.multi.add(nm); if (!S.multi.size) S.multi = null; S.sel = nm; renderGraph(); renderRight(); return; }
  if (card) { S.multi = null; S.wireSel = null; } }, true);
$('#graphhost').addEventListener('dblclick', e => { const t = e.target;
  if (S.openRig) { if (!t.closest('.rbody, .rpop, .rjoint, .rigbar') && nodeOf(S.openRig)?.type === 'rig') { e.stopPropagation(); openPal({ x: e.clientX, y: e.clientY, title: 'Add a body', items: [{ label: 'point mass', act: () => addBody('mass') }, { label: 'modal object', act: () => addBody('modal') }] }); } return; }
  if (t.closest('.gnode .n')) { e.stopPropagation(); S.sel = t.closest('.gnode').dataset.node; S.tab = 'node'; renderGraph(); renderRight(); const i = $('[data-rename]'); if (i) { i.focus(); i.select(); } return; }
  if (!t.closest('.gnode, .gtool') && $('#graphhost .graph')) { e.stopPropagation(); openNodePalette(e.clientX, e.clientY, graphXY(e)); } }, true);
$('#graphhost').addEventListener('change', e => { if (e.target.matches('[data-rbrename]')) renameBody(e.target.dataset.rbrename, e.target.value); });
$('#graphhost').addEventListener('keydown', e => { if (e.target.matches('[data-rbrename]') && e.key === 'Enter') e.target.blur(); });

// inspector
$('#right').addEventListener('change', async e => { const t = e.target, n = nodeOf(S.sel); let k;
  if (t.matches('[data-rename]')) { if (!await renameNode(t.dataset.rename, t.value)) t.value = t.dataset.rename; return; }
  if ((k = t.dataset.fbox)) return act('set_field', { node: S.sel, field: k, value: +t.value });
  if ((k = t.dataset.fbool)) return act('set_field', { node: S.sel, field: k, value: t.checked });
  if ((k = t.dataset.fstr)) return act('set_field', { node: S.sel, field: k, value: t.value });
  if ((k = t.dataset.fjson)) { let v; try { v = JSON.parse(t.value); } catch (er) { toast(`${k}: not valid JSON`); return; } return act('set_field', { node: S.sel, field: k, value: v }); }
  if ((k = t.dataset.ftxt)) { const s = t.value.trim(), v = s !== '' && !isNaN(+s) ? +s : s; return act('set_field', { node: S.sel, field: k, value: v }); }
  if (t.matches('[data-fadd]') && t.value) { const f = t.value; return act('set_field', { node: S.sel, field: f, value: fieldDefault(n.type, f) }); }
  if (t.matches('[data-pval]')) return ops({ params: { [t.dataset.pval]: +t.value } });
  if (t.matches('[data-sseed]')) return ops({ seed: Math.max(1, Math.round(+t.value)) });
  if (t.matches('[data-sdur]')) return ops({ duration: +t.value || 0 });
  const out = doc().nodes.find(x => x.type === 'output');
  if (t.matches('[data-stval]')) return act('set_field', { node: out.name, field: out.fields.lufs != null ? 'lufs' : 'peak_db', value: +t.value });
  if (t.matches('[data-sfade]')) return act('set_field', { node: out.name, field: 'fade_' + t.dataset.sfade, value: +t.value / 1000 });
  if (t.matches('[data-sceil]')) return act('set_field', { node: out.name, field: 'ceiling_db', value: +t.value }); });
$('#right').addEventListener('keydown', e => { if (e.key === 'Enter' && e.target.matches('[data-rename], [data-pnew], input.fnum, input.fexpr')) { e.target.blur(); if (e.target.matches('[data-pnew]')) $('[data-padd]')?.click(); } });
$('#right').addEventListener('click', e => { const t = e.target; let b;
  if ((b = t.closest('[data-fclear]'))) return act('set_field', { node: S.sel, field: b.dataset.fclear, value: 0 });
  if ((b = t.closest('[data-funset]'))) return act('set_field', { node: S.sel, field: b.dataset.funset, value: null });
  if ((b = t.closest('[data-fsig]'))) return sourceMenu(e, S.sel, b.dataset.fsig);
  if (t.closest('[data-ndel]')) return deleteNodes(S.multi ? [...S.multi] : [S.sel]);
  if (t.closest('[data-ndup]')) return duplicateNodes(S.multi ? [...S.multi] : [S.sel]);
  if ((b = t.closest('[data-skind]'))) { const k = b.dataset.skind; return ops(k === 'loop' && !doc().duration ? { kind: k, duration: 2 } : { kind: k }); }
  if (t.matches('[data-sdurauto]')) return ops({ duration: t.checked ? 0 : Math.max(0.1, +(doc().seconds || 1).toFixed(2)) });
  if ((b = t.closest('[data-starget]'))) { const o = doc().nodes.find(x => x.type === 'output'), k = b.dataset.starget, op = opOf(o); delete op.lufs; delete op.peak_db; if (k === 'lufs') op.lufs = -16; if (k === 'peak') op.peak_db = -1; return ops({ ops: [op] }); }
  if ((b = t.closest('[data-pdel]'))) { const p = clone(doc().params); delete p[b.dataset.pdel]; return toast('removing a param: delete its uses first, then it can go (sound_ops has no param delete)'); }
  if (t.closest('[data-padd]')) { const i = $('[data-pnew]'), nm = i.value.trim().replace(/^\$/, ''); if (!NAME_RE.test(nm)) return toast('a param name is a word'); return ops({ params: { [nm]: 1 } }); }
  if (t.closest('[data-dlg="save"]')) return saveDialog();
  if (t.closest('[data-dlg="export"]')) return exportDialog(); });

// dock
$('#dock').addEventListener('click', e => { if (e.target.closest('[data-kadd]')) { newKnob(); return; } const kn = e.target.closest('.knob .kn'); if (kn) { $('#kpop').hidden = true; openKedit(kn.closest('[data-knob]').dataset.knob); } });
// sound switcher: new, duplicate, template
$('#docpop').addEventListener('click', e => { if (e.target.closest('[data-newsound]')) newSound(); else if (e.target.closest('[data-dupsound]')) newSound(ST.doc);
  else if (e.target.closest('[data-tplsound]')) { const r = e.target.getBoundingClientRect(); openPal({ x: r.left, y: r.bottom + 4, title: 'From a template', items: [{ label: 'door_slam', hint: 'the approved layered slam', act: () => newSound(null, 'door_slam') }] }); } });
$('#opslog').addEventListener('click', () => { $('#opslog').classList.toggle('open'); renderOpslog(); });

// keys
addEventListener('keydown', e => { if (!ST) return; const mod = e.metaKey || e.ctrlKey, k = e.key.toLowerCase();
  if (e.key === 'Escape') { if (PAL) { closePal(); e.stopImmediatePropagation(); return; } if (!$('#dlg').hidden) { $('#dlg').hidden = true; e.stopImmediatePropagation(); return; } if (S.mapArm) { disarmMap(); e.stopImmediatePropagation(); return; } if (!$('#kedit').hidden) { $('#kedit').hidden = true; KEDIT = null; e.stopImmediatePropagation(); return; } if (S.wireSel || S.multi) { S.wireSel = null; S.multi = null; renderGraph(); e.stopImmediatePropagation(); return; } return; }
  if (editable(e.target) || S.view === 'grid') return;
  if (mod && k === 'z') { e.preventDefault(); e.stopImmediatePropagation(); stepHistory(e.shiftKey ? 1 : -1); return; }
  if (mod && k === 'd') { e.preventDefault(); e.stopImmediatePropagation(); if (S.sel) duplicateNodes(S.multi ? [...S.multi] : [S.sel]); return; }
  if (mod) return;
  if (k === 'a') { e.preventDefault(); e.stopImmediatePropagation(); openAddAtCentre(); return; }
  if (e.key === 'Delete' || e.key === 'Backspace') { e.preventDefault(); e.stopImmediatePropagation();
    if (S.openRig && S.centre === 'graph') { if (S.rigSel && nodeOf(S.openRig)?.type === 'rig') deleteRigPart(S.rigSel); return; }
    if (S.wireSel) return disconnect(S.wireSel); if (S.multi) return deleteNodes([...S.multi]); if (S.sel && S.centre === 'graph') deleteNodes([S.sel]); } }, true);

async function boot() {
  try {
    if (!IN_WINDOW && !BRIDGE) await loadScript('fixture.js');   // plain browser: fixture.js answers sounder.call and the bulk GETs
    await loadFonts();
    CMAP = new Uint8Array(await bulk('render/colormap', {}));
    sounder.call('node_types', {}).then(t => { TYPES = t; if (ST) renderRight(); }).catch(() => {});
    applyState(await sounder.call('state', {}));
    requestAnimationFrame(overlay);
    if (BRIDGE) (async function poll(since) { // the session's `rev` moved: the page pulls `state` (as sounder.changed does)
      try { const r = await (await fetch(`${BRIDGE}/poll?since=${since}`)).json(); if (r.rev > since) onChanged(r.rev); setTimeout(() => poll(Math.max(since, r.rev)), 50); }
      catch (e) { setTimeout(() => poll(since), 1000); }
    })(0);
  } catch (e) { document.body.insertAdjacentHTML('beforeend', `<div class="toast"><span class="chip warn">${esc(errText(e))}</span></div>`); }
}
boot();

// Macro pane height: drag the grip on its top edge; remembered per browser (optional).
(function () {
  const dk = $('#dock'); let y0 = 0, h0 = 0, on = false;
  try { const h = localStorage.getItem('sounder.dockh'); if (h) dk.style.setProperty('--dockh', h + 'px'); } catch (e) { /* storage is optional */ }
  dk.addEventListener('pointerdown', e => { if (!e.target.closest('[data-dockgrip]')) return; on = true; y0 = e.clientY; h0 = dk.getBoundingClientRect().height; dk.setPointerCapture(e.pointerId); e.preventDefault(); });
  dk.addEventListener('pointermove', e => { if (!on) return; const h = Math.max(90, Math.min(innerHeight * 0.8, h0 + (y0 - e.clientY))); dk.style.setProperty('--dockh', h + 'px'); });
  const end = () => { if (!on) return; on = false; try { localStorage.setItem('sounder.dockh', String(Math.round(dk.getBoundingClientRect().height))); } catch (e) { /* ignore */ } };
  dk.addEventListener('pointerup', end); dk.addEventListener('pointercancel', end);
})();
