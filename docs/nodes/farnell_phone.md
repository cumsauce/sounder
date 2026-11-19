# Telephone tones and two-tone alarms (Farnell)

Two nodes from Andy Farnell's *Designing Sound* patches
(`/Users/doomy/Downloads/BOOK-PUREDATA/`). Code: `crates/sfx_graph/src/nodes/farnell_phone.rs`;
tests: `crates/sfx_analysis/tests/it/farnell_phone.rs`. The patches were read as
netlists (Pd is not installed here), so there is no render-against-Pd distance
yet; the numbers below are measured on the node. Both need a `duration` (a
steady tone never decays).

## `phone_tone`

`{"kind": "dial|busy|ringback|dtmf|pulse_dial", "digits": "5551234", "line": 0..1, "tempo": 0.25..4}`

| kind | patch | what it makes |
|---|---|---|
| `dial` | PHONETONES/dialtone1.pd (350 + 440 Hz); dialtone2.pd uses 450 | steady two-sine tone |
| `busy` | busy-signal.pd | 480 + 620 Hz, `osc~ 2` x 10000 clipped 0..1 as a gate: 0.25 s on, 0.25 s off |
| `ringback` | ringingtone.pd | 480 + 440 Hz, `metro 1000` counting mod 6, `> 2`, `lop~ 100`: 3 s on, 3 s off |
| `dtmf` | DTMF/dtmf1.pd | per key a column and a row sine (the patch's tone list, 697-941 / 1209-1633, A-D too), `line~` 1 ms up, 200 ms, 1 ms down, then `hip~ 100` |
| `pulse_dial` | pulsedial.pd | a `metro 100` toggling a signal to 1 and (after `del 40`) to 0: 10 pulses a second, 40 ms each; a digit is that many pulses (0 is ten) |

`line` is telephone-line.pd (PHONETONES/telephone-line.pd, the same file as
ALARMS/telephone-line.pd): `clip~ -0.9 0.9`, `bp~ 2000 12`, `*~ 1.2`,
`bp~ 400 7`, `hip~ 90` twice. Pd's `bp~`, `hip~` and `lop~` are copied
exactly (`farnell.rs`). `line` crossfades the clean tone and that path, and
the input is scaled by `1 + line` (2 at line 1: the patches sum two unit
sines, which clip at 0.9). Defaults are what the patches do: 1 for `dial`,
`ringback`, `pulse_dial` (they all run through the line); 0 for `busy` and
`dtmf` (raw in the patch).

Measured (48 kHz):

- `dial`, `line` 0: peaks 350.0 and 440.0 Hz, equal level. `line` 1: centroid
  396 Hz, the energy sits at 250-500 Hz (the 400 Hz resonance), partials at
  171 / 260 / 350 / 440 / 530 Hz from the clipping, 1.5-2.6 kHz 54 dB under
  the 250-500 Hz band.
- `busy`: tones at 480 / 620 Hz, off periods below -100 dBFS, repeating every
  0.5 s. `ringback`: 440 / 480 Hz, on 0-3 s and 6-9 s.
- `dtmf` "5551234": 7 keys, 0.3 s each, key 1 reads 697 + 1209 Hz.
- `pulse_dial` "52": 7 pulses in the first 0.46 s then 0.7 s of gap; each
  pulse gives a click at make and one at break (the line's two resonances
  ring on each step); crest 21 dB.

Deviations:

- Peak-matched. The line takes 20-40 dB off a tone, so the node scales the
  line path to the clean path's peak: `line` changes the character, not the
  level.
- `ringback` starts with the ring (the patch's counter starts at 0, so it is
  silent for its first 3 s). The 3 s / 3 s cadence is the patch's (the US
  cadence is 2 / 4).
- `busy` gate edges are smoothed over about 1.5 ms (the patch's hard gate
  clicks).
- `pulse_dial`: the patch leaves the pulses as a bare 0/1 signal into the
  line. The dry path (`line` 0) is the same steps through `hip~ 90`, so a
  click rather than DC. The 0.7 s gap between digits and the 20 ms lead-in are
  mine; `tempo` scales all pulse and gap times.
- `dial` uses dialtone1.pd's 440 Hz, not dialtone2.pd's 450; the `tline` inside
  dialtone2.pd, pulsedial.pd and ringingtone.pd differs slightly from the
  standalone telephone-line.pd (a parallel `clip` x 0.15 branch and `bp~ 400 3`
  in dialtone2); the standalone one is used for all.
- `digits` is read as text: `params.rs` leaves the `digits` field of a
  `phone_tone` alone (a string of digits, or `1-2`, would otherwise be
  evaluated as a number). Separators (space, `-`, `,`, `.`) are ignored.
- DTMF level is that of two sines at 0.5 (the patch's 0.25 scale and the
  `output` node's normalisation make the absolute level moot).

## `two_tone`

`{"low": 600, "high": 800, "rate": 2, "shape": "switch|gated|smooth"}`

ALARMS/2tone-5.pd is `osc~ 2` x 1e6 clipped 0..1 into `lop~ 70` as g, and
`osc~ 800` x g + `osc~ 600` x (1 - g), x 0.2 (the `sig~ 1`, `-~` is the
complement). `switch` is that. `gated` is 2tone-3.pd's gated 800 Hz beep
(osc~ 2 gate through `lop~ 70`) taken for both tones, each on for 40 % of its
half with silence between. `smooth` is an equal-power cosine crossfade, the
`cos~` window idea of 2tone-10.pd. 2tone-10, -12, -15, -20 and -25 alternate
three or more tones (723 / 932 / 1012 Hz, a `phasor~` or `metro 500` stepping
mod 3, a timebase with `line~`, a sequencer table); they are not ported, so
`two_tone` is only the two-tone members of the family. The oscillators run
continuously, so a tone keeps its phase while it is off, as in the patch.

Measured: default is 800 Hz for 0.25 s then 600 Hz for 0.25 s (YIN pitch by
tenth 800 600 605 800 600 ...), peak 0.2, centroid by tenth 800 / 653 / 699;
`gated` has an 88 dB-deep gap between the tones; `smooth` holds the level
within 4 dB through the crossfade.
