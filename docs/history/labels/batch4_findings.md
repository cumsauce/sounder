# Batch 4 labels (metal and stone by synthesis), 2026-11-18

23 blind cues from `tools/labels/gen4.py` (key: `tools/labels/batch4.json`),
labelled on the "Metal and Stone Labels" page. Every earlier metal build had
been heard as glass, ceramic or plastic; no stone had read as stone.

## Metal: B wins on every object

| Object | A: clean resonators, split beating pairs (dry) | B: `modal` metal, noise 0.4, noise_width 0.1, low loss (dry / room) |
|---|---|---|
| steel pipe, f0 165, loss 0.3 | digital bell (ok) | large pipe drop (ok) / large pipe drop (good) |
| trash can, tube f0 118, loss 0.5 | fm bell (bad) | very large metal container hit / hitting large piece of metal (good, good) |
| pot lid, plate f0 310, loss 0.4 | digital bell (ok) | medium metal container hit / medium metal plate or gong hit (good, good) |
| wrench, bar f0 620, loss 0.6 | glassware (ok) | hitting a metal bell (good) / same (ok) |
| coin, plate f0 2900, loss 0.5 | ring of a small hand bell (good) | metal being dropped into a small glass (good, good) |

- Clean sine modes read as synthesis ("digital", "fm") even with split
  pairs beating and decays falling with frequency.
- Noise-band modes rung for seconds read as metal, and as the right object
  for the pipe, the bin and the lid. The earlier metal tries rang 0.1-0.4 s:
  the length of the ring is what moved them from glass to metal.
- The `heard_as` hint agreed on B for the pipe, lid and bin ("hitting a metal
  object", "a metal pipe dropped on the floor") and called every A a UI or
  error sound.

## Stone: the heavy one reads as rock, the small ones as clicks

| Design | Heard as |
|---|---|
| heavy slab (brown-noise thud 140 Hz, gritty crack, chips) | hitting a rock (ok) / hitting something and a small rock falls (ok) |
| rock on concrete, noise bands | hitting a keyboard button (good) / mouse click (ok) |
| rock on concrete, `modal` stone f0 1500 | mouse click (good, good) |
| two pebbles, `modal` stone f0 3400 | tapping small object (good, good) |

- Short, bright, dead hits read as plastic UI clicks. Weight under the hit
  (the slab) is what made one read as rock. Next: a larger rock with the
  slab's weight and grit and a little of the modal body.
- The hint called every stone "a digital glitch": it cannot judge stone.

## Kept as parts (named for what was heard)

`large_pipe_drop`, `large_metal_container_hit`, `metal_plate_gong_hit`,
`metal_bell_hit`, `metal_into_small_glass`, `small_object_taps`,
`mouse_click_tap`, `keyboard_button_hit`. Knobs: pitch and ring on the
metal, pitch or brightness on the taps; each knob's middle is the rated sound.
