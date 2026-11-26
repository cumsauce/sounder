# `contacts`: events from a physics log, and per-event detail

A physics-driven scene already knows when and how hard everything hit.
`contacts` reads that log (CSV or JSON, written by any engine or script) as
an event stream, so the sound follows the simulation instead of a
statistical clock. Every event can also carry detail (size, material, kind,
body, position), which `voices` patches read and `debris` uses.

## `contacts`

`{file, [columns], [fps], [reference], [min_strength], [kinds],
[materials], [listener [0, 0, 0]], [at 0]}`. Ports: `out` (every kept
contact), `impact`, `slide`, `roll`.

- `file`: relative paths resolve as for `sample` (the project, then the
  sources folders). `.json` is read as JSON, anything else as CSV. Editing
  the file re-renders the node (it keys on the file's mtime).
- Events are sorted by time; contacts before 0 or past the sound's end are
  dropped. At most 200,000 contacts.

### The log schema (engine-neutral)

One row (CSV) or object (JSON) per contact. Only a time and a strength are
required.

| Field | Default column names (first present wins, case folded) | Meaning |
|---|---|---|
| `time` | `time t seconds timestamp` | seconds |
| `frame` | `frame step tick` | used with `fps` when there is no time column |
| `strength` | `strength impulse speed velocity force` | any unit; the absolute value is used |
| `body` | `body body_id object id` | an integer body id (`$body`) |
| `size` | `size radius scale` | metres (`$size`) |
| `material` | `material mat surface` | a name (`$material` is its id) |
| `kind` | `kind type event` | `impact` (also hit, collision, begin, start, contact, bounce), `slide` (sliding, scrape, friction), `roll` (rolling); missing = impact |
| `position` | `position pos` | JSON only: `[x, y, z]` metres |
| `x` `y` `z` | `x px pos_x`, ... | CSV position columns (y, z default 0) |

`columns: {"time": "sim_t", "strength": "impulse_ns"}` names any column
instead; an unknown field name errors with the list. A missing time or
strength column errors with the columns the log has.

CSV: a header row, comma-separated, no commas inside fields (quotes around
a field are stripped); blank lines and lines starting with `#` are skipped.
JSON: an array of objects, or an object holding exactly one array
(`{"contacts": [...]}`).

```csv
time,impulse,body,size,material,kind,x,y,z
0.10,8.0,3,0.1,metal,impact,-1,0,0
0.30,1.0,3,0.1,metal,slide,0,0,5
```

### Value, filters, materials, position

- `value` = |strength| / `reference`, clamped 0..1. Without `reference` the
  strongest kept contact is 1 (so a log's loudness is relative to itself:
  give `reference` to compare several logs).
- `min_strength` (log units), `kinds` and `materials` drop contacts.
  `materials` also fixes the ids (`$material` = index in that list); without
  it every material is numbered in sorted, case-folded order.
- Positions are taken relative to `listener` (x right, y up, z forward) and
  kept on the event for a later spatializer. `pan` = x over the horizontal
  distance (-1..1), `distance` = the length of the position.

## Per-event detail (`Event.extra`)

Every field is optional; the plain event nodes leave them empty.

| Detail | `contacts` | `collapse` | `voices` param |
|---|---|---|---|
| size, m | `size` column | `brick` | `$size` |
| material id | `material` column | - | `$material` |
| kind | `kind` column | - | `$kind` (0 impact, 1 slide, 2 roll) |
| body | `body` column | - | `$body` |
| pan -1..1 | from position | the column, x `pan_width` (default 1) | `$pan` |
| distance, m | from position | with `distance`: from [x along the wall, 0, distance] | `$distance` |

- `voices` sets each param only when the event carries it, so a sound param
  of the same name (`$size`) stays visible for plain events. The `map`
  expressions read them too.
- `debris`: an event with a distance plays at `distance_ref` / d (default
  1 m; at most +12 dB, closer than a quarter of it) and, past
  `distance_ref`, its contact pulse lengthens by half per doubling of
  distance: a gentle high cut. `stereo: true` pans each hit (constant power)
  when the events carry a pan or position, ringing every bank twice; the
  floor and the bed sit in the middle. Mono is the default, and events
  without detail render bit-identical to before.

## Measured (tests: `crates/sfx_analysis/tests/it/modal.rs`, mods `collapse`, `contacts`)

- A 4 x 4 collapse at `distance` 8 against 1: more than 12 dB lower RMS and
  a lower spectral centroid (peak-normalized).
- A crumble from the left column with `stereo: true`: the left channel's
  energy centre comes before the right's.
- A voices patch reading `$size $material $kind $distance $pan` from a
  one-row CSV rings at the frequency those values give.

Not heard yet.
