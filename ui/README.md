# ui/: the sounder window's page

The page the `wry` window shows (`sounder://app/index.html`). It draws what the
Rust `Session` says and sends the human's actions back; there is no audio and
no DSP here. The contract is `design/window-api.md`; the look is
`design/window-concept.html` (the mock, read-only reference).

| file | what |
|---|---|
| `index.html`, `style.css` | the mock's markup and CSS, unchanged except the concept-notes card (mock only) is gone |
| `app.js` | the page: `sounder` shim, rendering, interactions |
| `fixture.js` | FIXTURE ONLY, plain-browser stand-in for the Rust side; never loaded in the window, no need to embed it |
| `fixture.json` | a `state` skeleton plus four sounds (laser with 3 macros, comments and history; pan_and_lid, a rig with bodies, joints and two knobs on joint / body fields; ui_click; wind_loop, a loop with a failing macro check) |
| `fonts/` | IBM Plex Sans / Mono (`IBMPlex-LICENSE.txt`), loaded with `FontFace` from `sounder://fonts/` in the window, `fonts/` in a browser |

## Design work in a browser

```sh
python3 -m http.server 8766 --directory ui    # or preview_start "ui" (.claude/launch.json)
```

Open `http://localhost:8766/index.html` (not `file://`: the fixture is fetched).
Without `window.ipc` the page loads `fixture.js`, which answers every call from
`fixture.json` and changes it in memory (knob moves, `set_field`, comments, seed,
history jumps, solo, play / stop / seek with a moving playhead, no sound). Its
spectrogram / wave / loudness / spark / colormap data is made by a crude
parametric model that follows the macros, so dragging a knob visibly changes the
picture. It is not a renderer.

## The `sounder` shim

`sounder.call(name, args)` posts `{id, name, args}` through `window.ipc.postMessage`
and resolves on `sounder._reply(id, {ok, result} | {ok:false, error})`; errors show
in the toast. Rust pushes `sounder.changed(rev)` (the page re-pulls `state`) and
`sounder.played({playing, pos, loop, span, node})` (the playhead resyncs, then is
extrapolated with `requestAnimationFrame`). Bulk data (`sounder://render/...`) is
fetched as `ArrayBuffer` and only drawn: dB grid -> colour through the colormap
bytes, wave and loudness columns, node sparks.

Knobs: dial and value follow the pointer locally; `move_knob` goes out at ~15 Hz
while dragging and on release, its reply is merged into `state.sound`, and the
picture is refetched (state is pulled on release). Node-tab sliders send
`set_field` on release.

Not backed by a call (so read-only or approximated, see `design/window-api.md`):
`$params` are listed, not edited; a tile's play button is `select_sound` + `play`.
