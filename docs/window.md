# The window

`sounder` (no `--headless`, default `window` feature) opens a system webview
(`wry` on a `tao` event loop, main thread) beside the MCP server: the human
listens and comments there; the agent keeps using the tools. `--headless`
never opens it; `cargo build --no-default-features` has no wry / tao / cpal.

```sh
cargo run                         # window + MCP on stdio
cargo run -- --project DIR        # same, in a project
```

Data flow (the contract is `design/window-api.md`):

- The page (`ui/`, embedded at build time by `build.rs`; a built-in raw-state
  page stands in when `ui/` is absent) calls `sounder.call(name, args)` ->
  `window.ipc` -> a `Cmd::Exec` on the session thread -> `window/api.rs` as
  client `window` -> the reply returns as `sounder._reply(...)`. The window's
  edits are credited to it: `set_field` is one history step `~node` `by window`;
  comments are `by you`; knob moves are recorded as the human's (the agent's
  `summary` shows and clears them; the window's `state` shows them without
  clearing).
- `Session::rev()` bumps on every visible change from any client; after each
  command that moved it the session thread wakes the loop and the page gets
  `sounder.changed(rev)`, then asks for `state` (`src/session/window_state.rs`).
  `state` never waits for the default checks or the macro sweeps: they are
  empty / null until a follow-up command (400 ms after changes pause) runs
  `Session::check` and bumps `rev`.
- Bulk data comes over `sounder://render/<doc>/{spectrogram,wave,loudness}`
  and `.../node/<name>/spark` from `sfx_analysis::view` (the same STFT window,
  dB scale and loudness window rule as `listen`'s image), `?rev=` caches.
- Playback goes through `PlayerControl` (`window/player_iface.rs`, a re-export
  of `src/player.rs`); player events become `sounder.played({...})`.

What the window cannot do yet: no Edit menu (so no Cmd-C / Cmd-V in text
fields), no `catalog_save` call, no resolving comments (the agent does that).

## Driving the real page from a browser (dev only)

`sounder --headless --dev-ui 8767` (window build) also serves the page and the
window's calls / bulk data over plain HTTP on 127.0.0.1 (`window/dev.rs`: no
auth, std only). Open `http://127.0.0.1:8767/app/index.html?bridge=http://127.0.0.1:8767`
in any browser (an agent's included): the page runs against the REAL session
exactly as the webview does (`?bridge=` switches `ui/app.js` from `window.ipc`
to `POST /call`, `GET /render/...`, and a `GET /poll` loop that stands in for
`sounder.changed`). With the driver daemon: `SOUNDER_EXTRA_ARGS="--dev-ui 8767"
python3 mcp_driver.py --daemon ...`, then talk to the same session as an agent
with `--connect`. This is how the plan's window checks were run: a knob drag
and a typed comment show up in the agent's `summary`; an agent's `sound_ops`
redraws the open page without a reload. Caveat for browser tooling: a page
without focus gets no `requestAnimationFrame`, so the first paint can wait for
a screenshot or click.

## Checking the native webview without a screenshot

`SOUNDER_PROBE=/path/out.json sounder` (window build) makes the window measure
its own rendered page 6 s after it opens and write the result: user agent,
loaded fonts, JS errors, rect / font / colour of the layout elements, canvas
sizes and how many sampled pixels are lit (did the spectrogram paint). The
driver's `--daemon --windowed` starts the window. Last run (WKWebView, dpr 2):
no errors, four Plex faces loaded, layout rects equal to the mock's
(top 48, hud 40, dock 162, right 300), spectrogram canvas painted.
