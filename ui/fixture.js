'use strict';
// =====================================================================
// FIXTURE ONLY. Loaded by app.js when there is no window.ipc (a plain
// browser, for design work and agent browser checks). It stands in for the
// Rust side: it answers sounder.call(...) from fixture.json and synthesises
// the bulk data (spectrogram / wave / loudness / spark / colormap) with a
// crude parametric model so the page has something plausible to draw.
// None of this is part of the real path: the window never loads this file,
// and nothing here is signal processing of an actual render.
// =====================================================================
window.FIX = (() => {
  let J = null, rev = 42, nextId = 100;
  const snaps = {};             // history id -> node fields, so `jump` restores something
  const PL = { playing: false, pos: 0, loop: false, span: null, t0: 0, timer: null, solo: null };
  const clone = o => JSON.parse(JSON.stringify(o));
  const ready = fetch('fixture.json').then(r => r.json()).then(j => { J = j; J.cur = J.state.doc; for (const s of Object.values(J.sound_data)) { recompute(s); s.history.forEach(h => snaps[h.id + s.name] = snapOf(s)); } });
  const snapOf = s => clone(s.nodes.map(n => [n.name, n.fields]));

  // ----- fixture "engine": macros write node fields, giving each node an effective value -----
  const mval = (m, x) => m.curve === 'exp' ? m.from * (m.to / m.from) ** x : m.from + (m.to - m.from) * x;
  function recompute(s) {
    for (const n of s.nodes) n.effective = {};
    for (const mac of s.macros) for (const e of mac.map) {
      const n = s.nodes.find(x => x.name === e.node); if (!n) continue; const v = mval(e, mac.value), base = n.effective[e.field] ?? clone(n.fields[e.field]);
      if (e.col != null) n.effective[e.field] = n.fields[e.field].map(row => row.map((c, j) => j === e.col ? +(c * v).toFixed(4) : c));
      else if (e.index != null) { const arr = clone(base); if (e.key) { const it = typeof arr[e.index] === 'string' ? { input: arr[e.index] } : arr[e.index]; it[e.key] = +v.toFixed(4); arr[e.index] = it; } else arr[e.index] = +v.toFixed(4); n.effective[e.field] = arr; }
      else n.effective[e.field] = +v.toFixed(4);
    }
    const eff = (node, f) => { const n = s.nodes.find(x => x.name === node); return n ? (n.effective[f] ?? n.fields[f]) : undefined; };
    if (s.name === 'laser') s.seconds = +(eff('amp', 'time') * 1.43).toFixed(3);
    const out = s.nodes.find(n => n.type === 'output'); if (out && typeof out.effective.lufs === 'number') s.lufs = +(out.effective.lufs - 0.0).toFixed(1);
  }
  const fv = (s, node, f) => { const n = s.nodes.find(x => x.name === node); return n ? (n.effective[f] ?? n.fields[f]) : undefined; };

  // ----- bulk data -----
  const hash = (a, b) => { const v = Math.sin(a * 12.9898 + b * 78.233) * 43758.5453; return v - Math.floor(v); };
  const db20 = x => 20 * Math.log10(Math.max(x, 1e-9));
  function model(s) {
    const L = s.seconds;
    if (s.name === 'laser') { const from = fv(s, 'sweep', 'from'), to = fv(s, 'sweep', 'to'), b = fv(s, 'cutoff', 'b'), q = fv(s, 'lp', 'q'), at = fv(s, 'amp', 'time');
      const amp = t => Math.exp(-6.9078 * t / at), f0 = t => from * (to / from) ** Math.min(1, t / 0.25);
      return { env: t => 0.9 * amp(t), partials: t => { const out = [], cut = f0(t) * b; for (let h = 1; h <= 16; h++) { const hz = f0(t) * h; if (hz > 22000) break; const o = Math.log2(hz / cut), g = 1 / Math.sqrt(1 + (hz / cut) ** 4) * (1 + (q - 0.7) * Math.exp(-o * o * 8)); out.push([hz, db20(amp(t) * g / h) - 12, 0.05]); } return out; }, bands: () => [], tonal: 0.7 }; }
    if (s.name === 'ui_click') { const modes = fv(s, 'body', 'modes'), fast = fv(s, 'fast', 'time'), hp = fv(s, 'hp', 'freq'), g = fv(s, 'm', 'inputs')[1].gain;
      return { env: t => 0.9 * Math.exp(-6.9078 * t / 0.06) + 0.05 * Math.exp(-6.9078 * t / fast), partials: t => modes.map(([hz, dec, gain]) => [hz, db20(gain * 0.6 * Math.exp(-6.9078 * t / dec)) - 8, 0.04]), bands: t => [[hp, 20000, db20(g * Math.exp(-6.9078 * t / fast)) - 34, 0.8]], tonal: 0.4 }; }
    const q = fv(s, 'howl', 'q'), rate = fv(s, 'gust', 'rate'), sw = fv(s, 'swell', 'range'), lufs = fv(s, 'out', 'lufs');
    const swell = t => sw[0] + (sw[1] - sw[0]) * (0.5 + 0.5 * Math.sin(2 * Math.PI * t * rate / 4 * 1.0 + 1.3)), cen = t => 800 + 450 * Math.sin(2 * Math.PI * t / 4 + 0.5) + 200 * Math.sin(2 * Math.PI * t / 4 * 2 * rate / 1.2);
    return { env: t => 0.5 * swell(t), partials: () => [], bands: t => [[cen(t), 0, db20(swell(t)) - 26 + (lufs + 25) * 0.5, 1.4 / Math.sqrt(q)]], tonal: 0.35, noisy: true };
  }
  function specGrid(s, w, h, t0, t1, hz0, hz1) {
    const m = model(s), out = new Float32Array(w * h), Lg = Math.log2(hz1 / hz0);
    for (let x = 0; x < w; x++) {
      const t = t0 + (x + 0.5) / w * (t1 - t0), env = m.env(t);
      for (let y = 0; y < h; y++) out[y * w + x] = -104 + hash(x, y) * 6 + (env > 0.01 ? 6 : 0) * hash(y, x);
      for (const [hz, db, wo] of m.partials(t)) { if (hz < hz0 / 1.2 || hz > hz1 * 1.2) continue; const r0 = h * Math.log2(hz1 / hz) / Lg, sg = Math.max(0.8, wo * h / Lg);
        for (let y = Math.max(0, Math.floor(r0 - 3 * sg)); y <= Math.min(h - 1, Math.ceil(r0 + 3 * sg)); y++) { const d = (y - r0) / sg, v = db - 12 * d * d + hash(x, y) * 3; if (v > out[y * w + x]) out[y * w + x] = v; } }
      for (const [lo, hi, db, bw] of m.bands(t)) for (let y = 0; y < h; y++) { const hz = hz1 * 2 ** (-(y + 0.5) / h * Lg); let d; if (hi) d = hz < lo ? Math.log2(lo / hz) : hz > hi ? Math.log2(hz / hi) : 0; else d = Math.log2(hz / lo) / bw; if (hi) d /= (bw || 1); const v = db - 9 * d * d + (hash(x, y * 3) - 0.5) * 9; if (v > out[y * w + x]) out[y * w + x] = v; }
    }
    return out;
  }
  const num = (q, k, d) => q[k] !== undefined && q[k] !== '' ? +q[k] : d;
  function bulkData(path, q) {
    if (path === 'render/colormap') { const C = [[0, 0, 4], [31, 12, 72], [85, 15, 109], [136, 34, 106], [186, 54, 85], [249, 142, 9], [252, 255, 164]], b = new Uint8Array(1024);
      for (let i = 0; i < 256; i++) { const u = i / 255 * 6, k = Math.min(5, Math.floor(u)), f = u - k; for (let j = 0; j < 3; j++) b[i * 4 + j] = Math.round(C[k][j] + (C[k + 1][j] - C[k][j]) * f); b[i * 4 + 3] = 255; } return b.buffer; }
    const mt = path.match(/^render\/([^/]+)\/(spectrogram|wave|loudness|node\/([^/]+)\/spark)$/); if (!mt) throw new Error('fixture: no such data ' + path);
    const s = J.sound_data[mt[1]]; if (!s) throw new Error(`fixture: no sound "${mt[1]}"`); const m = model(s), kind = mt[2].startsWith('node') ? 'spark' : mt[2];
    const t0 = num(q, 't0', 0), t1 = num(q, 't1', s.seconds), w = num(q, 'w', 256);
    if (kind === 'spectrogram') { const h = num(q, 'h', 128), hz0 = num(q, 'hz0', 20), hz1 = num(q, 'hz1', Math.min(20000, s.sample_rate / 2)), buf = new ArrayBuffer(16 + w * h * 4), dv = new DataView(buf);
      dv.setUint32(0, w, true); dv.setUint32(4, h, true); dv.setFloat32(8, t0, true); dv.setFloat32(12, t1, true); new Float32Array(buf, 16, w * h).set(specGrid(s, w, h, t0, t1, hz0, hz1)); return buf; }
    if (kind === 'wave') { const o = new Float32Array(3 * w); for (let i = 0; i < w; i++) { const t = t0 + (i + 0.5) / w * (t1 - t0), e = Math.min(1, m.env(t)), j = m.noisy ? (hash(i, 1) - 0.5) * 0.3 : 0; o[i] = -Math.min(1, e * (1 + j)); o[w + i] = Math.min(1, e * (1 - j)); o[2 * w + i] = e * m.tonal * (m.noisy ? 0.8 + 0.4 * hash(i, 2) : 1); } return o.buffer; }
    if (kind === 'loudness') { const o = new Float32Array(w); let pk = 1e-6; for (let i = 0; i < 64; i++) pk = Math.max(pk, m.env(i / 64 * s.seconds)); for (let i = 0; i < w; i++) { const t = t0 + (i + 0.5) / w * (t1 - t0); o[i] = Math.max(-90, s.lufs + 5 + db20(m.env(t) / pk)); } return o.buffer; }
    const n = s.nodes.find(x => x.name === mt[3]) || { type: 'osc' }, o = new Float32Array(92);
    for (let i = 0; i < 92; i++) { const t = i / 91 * s.seconds, e = Math.min(1, m.env(t)); o[i] = ['decay', 'env', 'gain', 'output', 'math', 'filter', 'resonators', 'mix'].includes(n.type) ? e : n.type === 'impulse' ? (i < 2 ? 1 : 0) : ['ramp', 'wander', 'lfo'].includes(n.type) ? 0.35 + 0.6 * (n.type === 'ramp' ? 1 - i / 91 : 0.5 + 0.5 * Math.sin(i / 9 + n.name.length)) : 0.7 * (0.8 + 0.4 * hash(i, 5)) * (n.type === 'noise' ? 1 : 0.9); }
    return o.buffer;
  }

  // ----- state and calls -----
  const cur = () => J.sound_data[J.cur];
  const bump = () => { rev++; setTimeout(() => window.sounder.changed(rev), 0); };
  const syncSounds = () => { for (const e of J.state.sounds) { const s = J.sound_data[e.name]; if (s) { e.open_comments = s.comments.filter(c => !c.resolved).length; e.seconds = s.seconds; e.lufs = s.lufs; } } };
  function state() { syncSounds(); return { ...clone(J.state), rev, doc: J.cur, sound: clone(cur()), solo: PL.solo, playing: PL.playing }; }
  function addStep(s, label) { s.history.forEach(h => h.current = false); const id = nextId++; s.version++; s.history.push({ id, label, by: 'window', version: s.version, current: true }); snaps[id + s.name] = snapOf(s); const e = J.state.sounds.find(x => x.name === s.name); if (e) { e.saved = false; e.by = 'you'; e.ago = 'now'; } }
  function emit(over) { Object.assign(PL, over); clearTimeout(PL.timer); const s = cur(); if (PL.playing && !PL.loop) PL.timer = setTimeout(() => emit({ playing: false, pos: 0 }), Math.max(0, (s.seconds - PL.pos)) * 1000); PL.t0 = performance.now(); window.sounder.played({ playing: PL.playing, pos: PL.pos, loop: PL.loop, span: PL.span, node: PL.solo }); }
  const tick = () => PL.playing ? PL.pos + (performance.now() - PL.t0) / 1000 : PL.pos;
  const calls = {
    state: a => { if (a.doc) { if (!J.sound_data[a.doc]) throw new Error(`no open sound named "${a.doc}"`); J.cur = a.doc; } return state(); },
    select_sound: a => { if (!J.sound_data[a.doc]) throw new Error(`no open sound named "${a.doc}"`); J.cur = a.doc; PL.solo = null; bump(); },
    move_knob: a => { const s = cur(), m = s.macros.find(x => x.name === a.name); if (!m) throw new Error(`no macro named "${a.name}"`); m.value = Math.max(0, Math.min(1, +a.value));
      const i = s.moves.findIndex(x => x.name === a.name); if (Math.abs(m.value - m.default) < 0.005) { if (i >= 0) s.moves.splice(i, 1); } else if (i >= 0) s.moves[i].to = +m.value.toFixed(3); else s.moves.push({ name: a.name, from: m.default, to: +m.value.toFixed(3) });
      recompute(s); const e = J.state.sounds.find(x => x.name === s.name); e.saved = false; bump(); return { macros: clone(s.macros), moves: clone(s.moves), seconds: s.seconds, peak_db: s.peak_db, lufs: s.lufs }; },
    reset_knobs: () => { const s = cur(); s.macros.forEach(m => m.value = m.default); s.moves = []; recompute(s); bump(); },
    set_field: a => { const s = cur(), n = s.nodes.find(x => x.name === a.node); if (!n) throw new Error(`no node named "${a.node}"`); if (!(a.field in n.fields)) throw new Error(`node #${s.nodes.indexOf(n)} (${n.type} "${n.name}"): field \`${a.field}\`: unknown field, expected one of ${Object.keys(n.fields).join(', ')}`);
      if (typeof a.value === 'number' && (!isFinite(a.value) || (a.value < 0 && !/db|lufs/.test(a.field)))) throw new Error(`node "${n.name}": field \`${a.field}\`: ${a.value} is below 0`);
      n.fields[a.field] = a.value; recompute(s); addStep(s, `~${a.node}`); bump(); },
    set_seed: a => { const s = cur(); s.seed = a.seed; addStep(s, `seed ${a.seed}`); bump(); },
    add_annotation: a => { const s = cur(); s.comments.push({ id: Math.max(0, ...s.comments.map(c => c.id)) + 1, text: a.text, by: 'you', node: a.node || null, time: a.time || null, hz: a.hz || null, resolved: false, reply: null, ago: 'now' }); bump(); },
    jump: a => { const s = cur(), h = s.history.find(x => x.id === a.id); if (!h) throw new Error('no such history step'); s.history.forEach(x => x.current = x === h); s.version = h.version; const sn = snaps[h.id + s.name]; if (sn) sn.forEach(([name, f]) => { const n = s.nodes.find(x => x.name === name); if (n) n.fields = clone(f); }); recompute(s); bump(); },
    solo: a => { PL.solo = a.node || null; bump(); if (PL.playing) emit({}); },
    play: a => { const s = cur(); emit({ playing: true, pos: a.from || 0, loop: !!a.loop || s.kind === 'loop', span: a.span || null }); },
    stop: () => emit({ playing: false, pos: 0 }),
    seek: a => emit({ pos: a.t }),
    catalog_list: () => clone(J.catalog),
    catalog_load: a => { throw new Error(`the fixture has no data for "${a.name}" (it only knows the three sounds in fixture.json)`); },
  };
  return {
    async call(name, args) { await ready; const f = calls[name]; if (!f) throw new Error(`unknown call ${name}`); const r = f(args || {}); return r === undefined ? null : clone(r); },
    async bulk(path, q) { await ready; return bulkData(path, q); },
  };
})();
