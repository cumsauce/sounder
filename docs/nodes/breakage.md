# `fracture_on` and `dust`: objects that break when hit hard

Two nodes for the moment a hit is hard enough to break the thing it hits.

## `fracture_on` (events in, events out)

`{events, [threshold 0.4], [sharpness 1], [chips 12], [spread 0.15], [exponent 2], [seed]}`

For each input event with `value` v over `threshold`, the strength is
s = (v - threshold) / (1 - threshold). The hit breaks with probability
s^`sharpness` (1 = linear; 3 = only the hardest break; 0.3 = most over the
threshold break). A broken hit emits 1 + round(`chips` x s) events:

- the first at the hit itself, size 1 (the crack),
- the rest after it, crowded early (as `shatter`) over
  `spread` x (0.3 + 0.7 s) seconds, so a harder hit scatters wider,
- each `value` = a power-law chip size in 1/100..1 (`exponent`, as `shatter`)
  times v, so a harder hit makes louder chips.

Output is only the chips, sorted. At most 200,000. Feed it to `voices` (a
small `modal` per chip), `grains` or `debris`; mix the original hits in
separately. Values at or under `threshold` never break (tested).

## `dust`

`{events, [material masonry | wood | glass], [hz], [q], [density], [settle 0.8], [rise 0.03], [amount 1], [gain_db -20], [seed]}`, mono.

A fine granular bed: random-sign impulses with a heavy-tailed size
(u^3), band-passed. Each event adds value^2 x `amount` to an energy
that falls 60 dB over `settle` (capped at 1.5); the energy smoothed over
`rise` sets the grain level, and its square root the grain probability (up to `density` per second) and the
grain level. Silent before the first event (tested).

| material | centre | q | density at full energy |
|---|---|---|---|
| masonry | 3 kHz | 0.8 | 6000/s |
| wood | 1.8 kHz | 0.8 | 3000/s |
| glass | 6.5 kHz | 1.5 | 5000/s |

`hz`, `q`, `density` override the material. Level is not normalized: balance
with `gain_db`. Not yet heard.

## Example

```json
[
  {"type": "collapse", "name": "wall", "rows": 8, "cols": 8, "at": 0.05},
  {"type": "fracture_on", "name": "chips", "events": "wall", "threshold": 0.5, "sharpness": 1.5, "chips": 10},
  {"type": "dust", "name": "grit", "events": "wall", "material": "masonry", "settle": 1.2, "gain_db": -24}
]
```
Chips go through `voices` of a small stone `modal`; `grit` goes into the mix.
