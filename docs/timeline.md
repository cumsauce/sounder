# Working to a video timeline

A sound can be laid out in video frames and exported to line up with a clip.
Nothing changes for a sound without `fps`.

## Frame rate and frame times

- `fps` is a document field (optional; 24, 25, 29.97, 30, 60, any rate in
  0..1000). Set it with `new_sound {fps}` or `sound_ops {fps}`; `fps: 0`
  in `sound_ops` removes it. `summary` shows it on the first line.
- With an `fps`, any time may be written in frames: `"f:123"` or `"123f"`
  (fractions allowed, `"12.5f"`). This works in every node field (`at`,
  `start`, impulse `at` lists, `pattern` `every`, a `voices` patch, ...) and
  in the `duration` argument of `new_sound` / `sound_ops`.
- Frames are resolved to seconds (`frames / fps`) in one place,
  `params::resolve_frames`, before `$param` / `=expr` resolution, both when an
  edit is validated and when the graph is rendered. The document keeps the
  frame strings, so changing `fps` re-times every frame field.
- The converter does not know which fields are times: a frame string in a
  non-time field (`freq: "12f"`) becomes seconds there too. Use it only for
  times.
- A frame string without an `fps` is an error naming the field ("no fps; set
  fps ..."). Frames are not expressions: `"=12f + 1"` is not supported; use
  seconds or a param for arithmetic.
- A frame time lands on the nearest sample (`frames / fps * sample_rate`,
  as every time field does): at 29.97 fps frame 30 is 1.001 s.

## Export for a clip

`export` takes, besides its usual arguments:

| Argument | Meaning |
|---|---|
| `start_frame` | the clip frame the sound starts on (needs `fps`): that many frames of leading silence, so sample 0 of the file is frame 0 of the clip |
| `offset` | the same in seconds or frames (`"f:10"`); not with `start_frame` |
| `length_frames` | the clip's length in frames (needs `fps`): the file is padded with silence or trimmed to exactly this, offset included |
| `length` | the same in seconds or frames; not with `length_frames` |

- A trim fades the last 5 ms out so the cut does not click.
- LUFS and peak in the manifest are measured on the render, before the
  padding (silence does not change either).
- The manifest records `fps`, `offset` (seconds), `start_frame` and
  `length_frames` (null without an `fps` / a length). A loop's `loop.start`
  moves to the offset.
- Lengths are rounded to whole samples: 50 frames at 25 fps and 48 kHz is
  exactly 96000 samples; at 29.97 fps a frame is 1601.6 samples, so a clip
  length is rounded once, not per frame.

```json
{"name": "export", "args": {"path": "renders/wall.wav", "start_frame": 48, "length_frames": 240}}
```

Tests: `crates/sfx_graph/tests/graph.rs` ("video frames") and
`tests/args.rs` (`export_lines_up_with_a_video_clip`).
