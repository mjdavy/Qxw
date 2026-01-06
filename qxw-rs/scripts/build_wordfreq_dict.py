#!/usr/bin/env python3
"""Build a crossword-friendly word list using the `wordfreq` package.

Why:
- The macOS/Linux system dictionaries often contain archaic/obscure words.
- `qxwtool` will happily use whatever words exist in the list.

This script produces a plain text dictionary (one word per line) suitable for
`qxwtool --dict <FILE>`.

Example:
  python3 scripts/build_wordfreq_dict.py --out dict-wordfreq.txt --min-zipf 3.2
"""

from __future__ import annotations

import argparse
import re
import sys


_WORD_RE = re.compile(r"^[A-Za-z]+$")


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True, help="Output path (one word per line)")
    ap.add_argument("--lang", default="en", help="wordfreq language code (default: en)")
    ap.add_argument("--top", type=int, default=200_000, help="How many candidates to consider")
    ap.add_argument("--min-len", type=int, default=3, help="Minimum word length")
    ap.add_argument("--max-len", type=int, default=15, help="Maximum word length")
    ap.add_argument(
        "--min-zipf",
        type=float,
        default=3.0,
        help="Minimum Zipf frequency (higher = more common). Typical: 3.0..4.0",
    )
    ap.add_argument(
        "--short-max-len",
        type=int,
        default=3,
        help="Words up to this length are treated as 'short' for extra filtering",
    )
    ap.add_argument(
        "--min-zipf-short",
        type=float,
        default=3.8,
        help="Minimum Zipf frequency for short words (helps drop abbreviations). Typical: 3.6..4.2",
    )
    ap.add_argument(
        "--allow-no-vowel",
        action="store_true",
        help="Allow words with no vowel (by default, require a vowel)",
    )
    ap.add_argument(
        "--vowels",
        default="AEIOU",
        help="Vowel set to require (default: AEIOU). Use AEIOUY to treat Y as a vowel.",
    )
    args = ap.parse_args(argv)

    vowels = set(args.vowels.upper())
    if not vowels:
        print("error: --vowels must not be empty", file=sys.stderr)
        return 2

    try:
        from wordfreq import top_n_list, zipf_frequency
    except Exception as e:
        print(
            "error: missing dependency 'wordfreq'. Install it with:\n"
            "  python3 -m pip install wordfreq\n\n"
            f"details: {e}",
            file=sys.stderr,
        )
        return 2

    words: set[str] = set()
    for w in top_n_list(args.lang, args.top):
        if not _WORD_RE.match(w):
            continue
        if not (args.min_len <= len(w) <= args.max_len):
            continue
        z = zipf_frequency(w, args.lang)
        if z < args.min_zipf:
            continue

        if len(w) <= args.short_max_len and z < args.min_zipf_short:
            continue

        if not args.allow_no_vowel:
            if not any((ch.upper() in vowels) for ch in w):
                continue
        words.add(w.upper())

    out_list = sorted(words)
    with open(args.out, "w", encoding="utf-8") as f:
        for w in out_list:
            f.write(w)
            f.write("\n")

    print(f"wrote {len(out_list)} words to {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
