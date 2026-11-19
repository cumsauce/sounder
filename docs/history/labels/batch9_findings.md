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
