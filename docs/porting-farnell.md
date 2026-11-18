# Porting Farnell's Designing Sound patches

Several nodes (`spark`, `splash`, `engine`, `motor`, `fan`, `clock`, `gunshot`,
the `explosion` fireball) started from Andy Farnell's *Designing Sound* (MIT
Press). Its companion Pure Data patches are the ground truth, not photos of
the book's diagrams: every photo-based port had a misreading. Lessons:

- **Read the `.pd` file, then play it.** A patch is plain text; dump it as a
  netlist (each box with its inputs) rather than reading the diagram. Then
  render it in real Pd and compare: `pd -nogui -batch -noaudio -r 48000
  -compatibility 0.41 -open patch.pd`, with the top-level `dac~` swapped for
  `writesf~ 2`, a `loadbang` sending `; pd dsp 1` and `open file.wav, start`,
  and a delayed `stop` then `; pd quit` (quitting at once loses the file).
  Patches that wait for a button or slider need a message sent to that box;
  sliders saved with init send their values on load. Compare with `listen
  reference` (STFT distance and the biggest band x time differences).
- **The book's Pd is not today's.** The patches date from 2008-2010
  (Pd-extended, Pd 0.39-0.43):
  - `pow~` did not exist in vanilla before 0.42 (2009); the patches used
    Pd-extended's cyclone `pow~`, which is Max-style: **right inlet ^ left
    inlet** (motorenv.pd: `sig~ 6` on the left and the text says
    "complement^6"). Vanilla 0.42+ is left ^ right. So guns1.pd's chirp is
    64^(1.05 x) in the book's Pd. The listener heard that as "a raygun"
    and preferred (1.05 x)^64, so `gunshot` keeps the latter on purpose.
  - `wrap~` before 0.48 output 1 instead of 0 at whole numbers; `osc~`,
    `cos~`, `vcf~` used a 512-point table before 0.55 (`-compatibility`
    restores both).
- **Other gotchas:** `cos~` takes cycles, not radians; `lop~` / `hip~` are
  one-pole; `bp~` takes centre and Q; a `vline~` message `1 1 0, 0 $1 1`
  is a 1 ms rise then a $1 ms fall; fan-in to a signal inlet sums;
  abstractions take creation arguments as `$1` (spark6formant 1.8 scales
  every band by 1.8).
- **Faithful is not always better by ear.** The listener rated the closer
  ports of `spark` (humandsparks.pd) and `fan` (fan5.pd) worse than the
  adaptations, so those nodes keep the adaptations; the engine, motor,
  clock, gunshot noise and fireball follow their patches. A port is a
  starting point; the listener decides.
