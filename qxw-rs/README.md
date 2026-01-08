# qxw-rs (incremental Rust port)

This folder contains an incremental Rust port of the Qxw crossword tool.

Current scope (first slice):
- Load `.qxw` files (`#QXW2` and legacy header)
- Recompute clue numbering
- Export/print the answer list (text or HTML)
- Write `.qxw` files in `#QXW2` format (round-trippable)

Non-goals:
- The CLI/library should remain usable without GTK dependencies

UI status:
- A GTK4 UI binary exists as `qxwgtk` behind the `ui-gtk4` feature flag.

## Build

From this directory:

- `cargo build`

### GTK4 UI

The GTK UI is optional and feature-gated.

- Build:
  - `cargo build --features ui-gtk4 --bin qxwgtk`

- Run (optionally with a file to open):
  - `cargo run --features ui-gtk4 --bin qxwgtk`
  - `cargo run --features ui-gtk4 --bin qxwgtk -- path/to/puzzle.qxw`

## Usage

- Print basic info:
  - `cargo run --bin qxwtool -- info ../examples/bar.qxw`

- Dump a human-friendly summary (+ ASCII grid for rectangular puzzles):
  - `cargo run --bin qxwtool -- dump ../examples/bar.qxw`

- Dump with clue numbers (gtype=0 only):
  - `cargo run --bin qxwtool -- dump ../examples/bar.qxw --numbers`

- Generate a new 5x5 mini-style crossword and save as `.qxw`:
  - `cargo run --bin qxwtool -- generate-mini out-mini.qxw`
  - With a specific wordlist: `cargo run --bin qxwtool -- generate-mini --dict /usr/share/dict/words out-mini.qxw`
  - Reproducible: `cargo run --bin qxwtool -- generate-mini --seed 1 out-mini.qxw`
  - NYT-like blocked mini (rotationally symmetric black squares):
    - `cargo run --bin qxwtool -- generate-mini --blocks 6 out-mini-blocked.qxw`
    - With constraints: `cargo run --bin qxwtool -- generate-mini --blocks 6 --min-word-len 3 --seed 1 out-mini-blocked.qxw`
    - Auto-pick a reasonable even block count: `cargo run --bin qxwtool -- generate-mini --blocks auto --seed 1 out-mini-blocked.qxw`

### Dictionary quality (important)

The generator will only be as good as the dictionary you give it.

On macOS/Linux, the default system word lists (e.g. `/usr/share/dict/words`) often include archaic/obscure words and abbreviations. That can yield fills that don’t feel “NYT mini”-like.

One easy way to get a more modern/common word list is to generate one using the `wordfreq` Python package:

- Install and build a dictionary:
  - Using `uv` (recommended):
    - `uv run --with wordfreq -- python scripts/build_wordfreq_dict.py --out dict-wordfreq.txt --min-zipf 3.2 --min-zipf-short 3.8`
  - Or using `pip`:
    - `python3 -m pip install wordfreq`
    - `python3 scripts/build_wordfreq_dict.py --out dict-wordfreq.txt --min-zipf 3.2 --min-zipf-short 3.8`

- Use it for generation:
  - `cargo run --bin qxwtool -- generate-mini --blocks auto --dict dict-wordfreq.txt mini.qxw`

Notes:
- You can tweak `--min-zipf` upward (e.g. `3.6`) to get more common words, at the cost of fewer options.
- For sizes > 5, generation can take longer; use `--time-limit-ms` to cap it (default is 5000ms, `0` disables the limit).

#### No-Python option: `qxwtool build-dict`

If you don’t want to use Python, `qxwtool` includes a basic dictionary cleaner.

This does not know true word frequency; it applies simple heuristics (letters only, length bounds, vowel/consonant balance, etc.) to remove a lot of “junk” entries.

- Build a cleaned dictionary from a system word list:
  - `cargo run --bin qxwtool -- build-dict /usr/share/dict/words dict-clean.txt --min-len 3 --max-len 15`

- For mini-style fill, you can tighten the filter a bit (example):
  - `cargo run --bin qxwtool -- build-dict /usr/share/dict/words dict-mini.txt --min-len 3 --max-len 7 --min-consonants 2`

- Then generate with it:
  - `cargo run --bin qxwtool -- generate-mini --blocks auto --dict dict-mini.txt mini.qxw`

- Print answers:
  - `cargo run --bin qxwtool -- answers path/to/puzzle.qxw`

- Write answers as HTML:
  - `cargo run --bin qxwtool -- answers path/to/puzzle.qxw --format html --out answers.html`

- Round-trip save back to `#QXW2`:
  - `cargo run --bin qxwtool -- save ../examples/bar.qxw out.qxw`

### Visual inspection: printable SVG export

Export one or more `.qxw` files as printable SVGs (completed grid with letters + clue numbers):

- `cargo run --bin qxwtool -- export-svg out-svgs path/to/puzzle.qxw`
- Batch (shell glob): `cargo run --bin qxwtool -- export-svg out-svgs out-year/*.qxw`

Options:
- Change size: `--cell 48`
- Blank grid (no letters): `--blank`
- Hide clue numbers: `--no-numbers`
- Overwrite: `--overwrite`

### Batch generation: one puzzle per day

Generate a full year of dated puzzles (one `.qxw` per day) into a directory, using `dict-wordfreq.txt`:

- `cargo run --bin qxwtool -- generate-year out-year --year 2026 --dict dict-wordfreq.txt`

Notes:
- Output files are named like `YYYY-MM-DD.qxw`.
- The generator varies layout across days (open / blocked fixed / blocked auto) and retries to ensure puzzles are unique within the batch.
- Use `--overwrite` if re-running into an existing directory.
