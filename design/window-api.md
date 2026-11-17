# Window API: the contract between the Rust side and `ui/`

Two lanes build against this file in parallel: **p5-window-rs** (Rust: the
`wry` / `tao` window, `Session` glue, bulk data) and **p5-ui** (the page: the
mock in `design/window-concept.html` moved to `ui/`, with its DSP removed).
Neither may deviate silently: if a lane must change something here, it edits
this file in its own commit and says so in its report.

The page is the mock's look and interactions, unchanged. The mock's left nodes
pane is gone (nodes live in the Graph view). No A/B, no seeds view. Everything
the page shows comes from `Session`: no DSP in JS.

## Transport

- JS -> Rust: `sounder.call(name, args) -> Promise<result>`. Implemented in
  `ui/app.js` over `window.ipc.postMessage(JSON.stringify({id, name, args}))`.
  Rust replies by evaluating `sounder._reply(id, {ok: true, result} |
  {ok: false, error: "text"})`. Errors are the same plain-English strings the
  MCP tools return; the page shows them in its toast.
- Rust -> JS push: `sounder.changed(rev)` after any change to any sound, an
  agent's `sound_ops`, a knob move, an undo, a comment. The page then calls
  `state`. Also `sounder.played({playing, pos, loop, span, node})` on player
  start / stop / loop / seek (the page extrapolates the playhead with
  `requestAnimationFrame` and resyncs on each event).
- Bulk binary data over the custom protocol `sounder://` (GET only). Page and
  fonts are served from it too: `sounder://app/index.html`, `app.js`, `style.css`,
  `sounder://fonts/<file>`.
- Browser fallback: with no `window.ipc`, `app.js` loads `ui/fixture.json` (a
  `state` result) so the page opens in a plain browser for design work. Calls
  then no-op locally (knob moves change the fixture state in memory only).
- The window is a Session client named `window`: its edits are credited to it.

## `state` (call; no args or `{doc}` to switch the window's open sound)

```jsonc
{
  "rev": 42,                       // bumps on every change; also in sounder.changed
  "doc": "laser",                  // the open sound
  "sounds": [ { "name": "laser", "kind": "oneshot|loop", "by": "look",
                "ago": "2 min", "working": true, "saved": false,
                "open_comments": 1,
                "seconds": 0.404, "lufs": -18.2 } ],   // seconds, lufs: for the All-sounds tile (null if it does not render yet)
  "project": "friendSLOP",         // the top bar's label: the project folder, or "shared library"
  "agents": [ { "name": "look", "live": true } ],
  "feed": [ { "agent": "look", "tool": "sound_ops", "what": "laser · ~lp",
              "ago": "0:12", "doc": "laser" } ],        // newest first, <= 30
  "sound": {
    "name": "laser", "kind": "oneshot", "version": 4, "seed": 1,
    "sample_rate": 48000, "duration": null,               // null = automatic
    "seconds": 0.404, "peak_db": -1.4, "lufs": -18.2, "causal": false,
    "warnings": ["..."],
    "nodes": [ { "name": "lp", "type": "filter", "reads": ["zap", "cutoff"],
                 "fields": { "mode": "lp", "freq": "cutoff", "q": 1.5 },  // raw node JSON minus type / name
                 "effective": { "q": 1.6 },   // fields a macro drives: the value in use
                 "unread": false, "open_comments": 0, "depth": 4 } ],
    "params": { "base": 220 },
    "macros": [ { "name": "brightness", "value": 0.5, "default": 0.5,
                  "doc": "...", "claim": ["centroid", 1], "game": false,
                  "map": [ { "node": "lp", "field": "q", "from": 0.8, "to": 2.4,
                             "curve": "linear", "col": null, "index": null, "key": null } ],
                  "result": { "pass": true, "detail": "centroid rises over 0..1: 1.7k Hz -> 3.5k Hz" } } ],
                                            // result is null until verify (or the window) ran the sweep
    "moves": [ { "name": "brightness", "from": 0.5, "to": 0.8 } ],   // the human's, unread by the agent
    "comments": [ { "id": 1, "text": "...", "by": "you", "node": null,
                    "time": [0.0, 0.07], "hz": [2500, 9000],
                    "resolved": false, "reply": null } ],
                    // reply: null | "text" | {by, text}; an optional "ago" string is shown if present
    "history": [ { "id": 3, "label": "~lp", "by": "look", "version": 4, "current": true } ],
    "checks": [ { "name": "peak_db", "pass": true, "detail": "-1.00 dBFS (max -0.3)" } ],
    "descriptors": { /* sfx_analysis::Descriptors as JSON: lufs, bands [8][5], centroid_hz, modes, ... */ }
  },
  "solo": null,                      // node name or null: what the player hears
  "solo_stats": null,                // {seconds, peak_db, lufs} of the soloed node, else null
  "playing": false
}
```

`nodes` are in evaluation order (dependencies first). `descriptors`,
`checks` and macro `result`s are computed by the same Rust that serves agents;
the window never recomputes them.

## Calls

| name | args | effect / result |
|---|---|---|
| `state` | `{doc?}` | the object above |
| `select_sound` | `{doc}` | the window's open sound (does not change any agent's active sound) |
| `move_knob` | `{name, value}` | `Session::move_knob`; sets the macro value and records the move; result: new `state.sound` fields `macros, moves, seconds, peak_db, lufs` |
| `reset_knobs` | `{}` | every macro back to its default (one `move_knob` each) |
| `set_field` | `{node, field, value}` | upserts one field of one node through `Session::apply_ops` as client `window`: one history step labelled `~node`; error text on a bad value |
| `set_seed` | `{seed}` | `sound_ops seed` |
| `add_annotation` | `{text, node?, time?, hz?}` | a comment by `you` |
| `jump` | `{id}` | history jump (undo / redo are jumps) |
| `solo` | `{node: string|null}` | what the player plays; the data URLs below take `node` too |
| `play` | `{from?, span?, loop?}` | start playback of the (solo) render; `from` seconds, `span` `[t0, t1]` the region (the page's Play / Loop it): with `loop` false play the span once and stop at `t1`, with `loop` true loop it |
| `stop` / `seek` | `{}` / `{t}` | |
| `catalog_list` / `catalog_load` | `{}` / `{name}` | saved sounds (`[{name, kind}]`, bare names also accepted; the switcher shows the ones not open); load opens it, then the page calls `select_sound` |

The page has no call for params (`$name`: shown read-only in the Sound tab) and
auditioning a tile in the All-sounds grid is `select_sound {doc}` then `play`
(the window's open sound follows). `descriptors` keys the page reads:
`rms_db`, `centroid_hz` (a number or `{start, middle, end}`), `bands` ([8][5]).
Macro checks may be in `checks` (names starting `macro `); if not, the page
lists each macro's `result` there. `sounder.changed(rev)` while a knob is being
dragged only refreshes the picture; the state is pulled on release.

## Bulk data (GET)

All take `?rev=<n>` (cache-busting) and optional `node=<name>` (default: the
output). Little-endian, no compression.

- `sounder://render/<doc>/spectrogram?w=<px>&h=<px>&t0=&t1=&hz0=&hz1=`:
  16-byte header (`u32 w, u32 h, f32 t0, f32 t1`) then `w * h` `f32` dB values
  (-120..0), row 0 = highest frequency, log-spaced rows between `hz0..hz1`
  (default 20..20000 or Nyquist), columns evenly in time `t0..t1` (default the
  whole sound). The page maps dB to colour with the same map as `listen`
  (`sfx_analysis::raster::colormap`, -90 dB = black, 0 = pale): expose that map
  as `sounder://render/colormap` (256 RGBA bytes).
- `sounder://render/<doc>/wave?w=<px>&t0=&t1=`: `3 * w` `f32`: per column the
  min, then max, then RMS (three arrays back to back).
- `sounder://render/<doc>/loudness?w=<px>&t0=&t1=`: `w` `f32` LUFS (-90 floor),
  window `clamp(duration / 4, 0.02, 0.4)` s, as in the `listen` image.
- `sounder://render/<doc>/node/<name>/spark`: 92 `f32` peak envelope for the
  graph card's mini waveform.

## What each lane owns

- **p5-window-rs**: `src/bin/sounder/window/` (`mod.rs`, `api.rs`, `protocol.rs`),
  `Session::window_state()` and what it needs (revision counter, agents / feed
  from the call log), `sfx_analysis/src/view.rs` (dB grid, wave columns,
  loudness columns, sparks), main.rs wiring (`sounder` with the `window`
  feature opens the window; `--headless` never does), embedding `ui/` with
  `include_str!` / `include_bytes!`, the `Player` handle calls (a stub trait
  until p5-player merges). Tests with a fake `Player` and without a window:
  `api` calls over `Session`, the protocol handlers' bytes, `state` shape
  matching this file (a JSON-schema-ish test of key names).
- **p5-ui**: `ui/index.html`, `ui/app.js`, `ui/style.css`, `ui/fixture.json`,
  `ui/fonts/`, `ui/fixture.js` (the plain-browser stand-in; the window never
  loads it, no need to embed it) (copy the IBM Plex files modeler ships under
  `~/Code/modeler/src/bin/modeler/preview/fonts/` with their licence
  text; remove the Google Fonts link). Delete the mock's `evaluate`, `fft`,
  `stft`, `describe`, `lufs`, `macroCheck`, `applyMacros`, WebAudio playback,
  the in-page example data; draw from `state` and the bulk data. A tiny local
  static server is fine for design work (`.claude/launch.json` already has one
  for `design/`; add `ui/`).
- **p5-player** owns `src/player.rs` and the MCP `play` tool; see its brief.

## Rust-side notes (p5-window, where it settled a detail)

- `Session::window_state(doc) -> Result<Value, String>` (an unopen `doc` is an
  error the page toasts). `solo` / `playing` are filled in by the window layer.
- `checks` is `[]` and a macro's `result` is `null` until the window's
  follow-up command ran the default checks (~400 ms after changes pause); that
  command bumps `rev`, so the page gets `changed` and asks again.
- `unread` on a node: an agent's history step since the human last opened the
  sound (`select_sound`, `catalog_load`) touched it.
- `set_field` with `value: null` removes the field. `jump` forces over an
  agent's step (the human may undo it). `time` / `hz` of `add_annotation` take
  `[a, b]` or one number.
- Results: `move_knob` / `reset_knobs` return `{macros, moves, seconds, peak_db,
  lufs}`; `select_sound` / `catalog_load` return a full `state`; `set_field`
  `{label, changed, seconds, peak_db, lufs}`; `play` `{playing, node}`;
  `catalog_list` the catalog entries. Not built: `catalog_save`.
- Responses carry `Access-Control-Allow-Origin: *` (the page is `sounder://app`,
  the data `sounder://render`: different hosts); with `?rev=` they are cached
  forever, without it `no-store`.
