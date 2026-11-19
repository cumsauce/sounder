# Labels batch 9: the foley ports and the retuned rain

The first grouped page (`tools/labels/grouped_page.py`): each box held 2-8
takes of one sound, and the listener picked the best take or "all bad".
A pick means "the best of these", not a rating.

| Box | Pick | Note |
|---|---|---|
| rain (stone, glass, metal roof, water; retuned) | stone, dressed (s10) | |
| leaves (dry, grass, trees) | trees, dressed (s09) | |
| surf (sand, shingle, storm) | shingle, dry (s33) | |
| creature roar | roar size 0.9, dressed (s38) | |
| creature growl | growl huge, dry (s37) | |
| creature snarl | dressed (s03) | |
| creature whine | dressed (s16) | |
| lava | default size, dressed (s15) | |
| underwater | dry (s19) | |
| cave | all bad | "i think you implemented this incorrectly" |
| frog | all bad | "probably same pitch when they croak, also sounds too vocalized" |

- `creature` replaces `roar`: no box went "all bad", so the roar is no longer heard as a cow.
- The retuned stone rain beat glass, metal and water.
- `cave` and `frog` need work. The frog's calls need pitch spread and a less vowel-like resonator.

## The rebuilds (batch 10, `tools/labels/gen10.py`)

- `cave`: foley's five fixed-pitch sine voices dripped up to 25 times a second, a beeping texture. Now 0.4-4 drips a second, mostly into pools: Minnaert bubbles of 1.5-5 mm with a rising pitch and a tick. The rest tick on wet rock. All of it runs through a dark FDN echo of 1.5-4 s set by `depth`, pre-rolled 2 s so the echo is full at the start.
- `frog`: a croak of its own replaces foley's `frog`. Each call is pitched ±12 %, each note ±6 %, falling 4-15 % through the note. The pulses are rasping noise clicks, and the sac is broad (Q 3, was 8).
