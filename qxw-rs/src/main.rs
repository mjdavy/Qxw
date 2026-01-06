use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use qxw::format::{load_qxw, save_qxw2};
use qxw::generator::{
    generate_mini_puzzle_blocked_auto_from_wordlist_size,
    generate_mini_puzzle_blocked_auto_from_wordlist_size_limited,
    generate_mini_puzzle_blocked_from_wordlist_size,
    generate_mini_puzzle_blocked_from_wordlist_size_limited,
    generate_mini_puzzle_from_wordlist_size,
    generate_mini_puzzle_from_wordlist_size_limited,
    load_wordlist_len,
    load_wordlist_lens,
};
use qxw::model::{DNAME, Puzzle, MXSZ};
use rand::rngs::StdRng;
use rand::SeedableRng;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Print basic info about a .qxw file
    Info {
        /// Input .qxw file
        input: PathBuf,
    },

    /// Load a .qxw file and write it back out in #QXW2 format
    Save {
        /// Input .qxw file
        input: PathBuf,

        /// Output path
        output: PathBuf,
    },

    /// Print a human-friendly summary (and ASCII grid for rectangular puzzles)
    Dump {
        /// Input .qxw file
        input: PathBuf,

        /// Render clue numbers inside cells (gtype=0 only)
        #[arg(long)]
        numbers: bool,
    },

    /// Print or export the answer list (one section per direction)
    Answers {
        /// Input .qxw file
        input: PathBuf,

        /// Output format (text or html)
        #[arg(long, default_value = "text")]
        format: String,

        /// Write output to file instead of stdout
        #[arg(long)]
        out: Option<PathBuf>,
    },

    /// Build a cleaned/filtered dictionary file from an input wordlist
    ///
    /// This is intended to turn system wordlists (which often contain punctuation,
    /// proper nouns, and obscure entries) into something more crossword-friendly.
    BuildDict {
        /// Input word list file (one word per line)
        input: PathBuf,

        /// Output dictionary path
        output: PathBuf,

        /// Minimum word length to keep
        #[arg(long, default_value_t = 3)]
        min_len: usize,

        /// Maximum word length to keep
        #[arg(long, default_value_t = 15)]
        max_len: usize,

        /// Allow words with no vowels (AEIOUY). By default, words must contain a vowel.
        #[arg(long)]
        allow_no_vowel: bool,

        /// Require at least this many consonants (letters not in AEIOUY)
        #[arg(long, default_value_t = 1)]
        min_consonants: usize,

        /// Require at least this many distinct letters
        #[arg(long, default_value_t = 2)]
        min_distinct: usize,

        /// Reject words with a repeated-letter run longer than this
        #[arg(long, default_value_t = 3)]
        max_run: usize,
    },

    /// Generate an NxN mini-style crossword and save as .qxw
    GenerateMini {
        /// Word list file (one word per line)
        #[arg(long)]
        dict: Option<PathBuf>,

        /// Output .qxw path
        out: PathBuf,

        /// Grid size (N for an NxN mini)
        #[arg(long, default_value_t = 5)]
        size: usize,

        /// Time limit in milliseconds (0 disables the limit)
        #[arg(long, default_value_t = 5000)]
        time_limit_ms: u64,

        /// Optional title
        #[arg(long)]
        title: Option<String>,

        /// Optional author
        #[arg(long)]
        author: Option<String>,

        /// RNG seed for reproducible generation
        #[arg(long)]
        seed: Option<u64>,

        /// Number of black squares (blocks). Use 0 for an open grid, or "auto" for a filled blocked grid.
        #[arg(long, default_value = "0")]
        blocks: String,

        /// Minimum across/down entry length in blocked grids
        #[arg(long, default_value_t = 3)]
        min_word_len: usize,
    },
}

#[derive(Debug, Clone, Copy)]
enum BlocksSpec {
    Fixed(usize),
    Auto,
}

fn parse_blocks_spec(s: &str) -> Result<BlocksSpec> {
    if s.eq_ignore_ascii_case("auto") {
        return Ok(BlocksSpec::Auto);
    }
    let n: usize = s
        .parse()
        .with_context(|| format!("invalid --blocks {s:?} (use 0, a number like 6, or 'auto')"))?;
    Ok(BlocksSpec::Fixed(n))
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.cmd {
        Command::Info { input } => {
            let mut puzzle = load_qxw(&input).with_context(|| format!("loading {}", input.display()))?;
            puzzle.compute_numbers();
            println!(
                "gtype={} width={} height={} symmr={} symmm={} symmd={} ndir={}",
                puzzle.gtype,
                puzzle.width,
                puzzle.height,
                puzzle.symmr,
                puzzle.symmm,
                puzzle.symmd,
                puzzle.ndir()
            );
            for d in 0..puzzle.ndir() {
                let mut count = 0usize;
                for (dir, _x, _y, _word, _number) in puzzle.iter_lights() {
                    if dir == d {
                        count += 1;
                    }
                }
                println!("{}: {} lights", DNAME[puzzle.gtype][d], count);
            }
        }

        Command::Save { input, output } => {
            let mut puzzle = load_qxw(&input).with_context(|| format!("loading {}", input.display()))?;
            puzzle.compute_numbers();
            save_qxw2(&puzzle, &output)?;
        }

        Command::Dump { input, numbers } => {
            let mut puzzle = load_qxw(&input).with_context(|| format!("loading {}", input.display()))?;
            puzzle.compute_numbers();
            print!("{}", render_dump(&puzzle, numbers));
        }

        Command::Answers { input, format, out } => {
            let mut puzzle = load_qxw(&input).with_context(|| format!("loading {}", input.display()))?;
            puzzle.compute_numbers();

            let rendered = match format.as_str() {
                "text" => render_answers_text(&puzzle),
                "html" => render_answers_html(&puzzle),
                other => anyhow::bail!("unknown format {other} (use text|html)"),
            };

            if let Some(out) = out {
                fs::write(&out, rendered).with_context(|| format!("writing {}", out.display()))?;
            } else {
                print!("{rendered}");
            }
        }

        Command::BuildDict {
            input,
            output,
            min_len,
            max_len,
            allow_no_vowel,
            min_consonants,
            min_distinct,
            max_run,
        } => {
            if min_len == 0 {
                anyhow::bail!("--min-len must be > 0");
            }
            if max_len < min_len {
                anyhow::bail!("--max-len must be >= --min-len");
            }
            if max_run == 0 {
                anyhow::bail!("--max-run must be > 0");
            }

            let bytes = fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let text = String::from_utf8_lossy(&bytes);

            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            let mut total_lines = 0usize;
            let mut normalized = 0usize;
            let mut kept = 0usize;

            for line in text.lines() {
                total_lines += 1;
                let Some(word) = normalize_word_ascii_alpha(line) else { continue };
                normalized += 1;

                let len = word.len();
                if len < min_len || len > max_len {
                    continue;
                }

                if !allow_no_vowel && !has_vowel(&word) {
                    continue;
                }

                if count_consonants(&word) < min_consonants {
                    continue;
                }

                if count_distinct_letters(&word) < min_distinct {
                    continue;
                }

                if max_repeated_run(&word) > max_run {
                    continue;
                }

                if seen.insert(word) {
                    kept += 1;
                }
            }

            let mut out_words: Vec<String> = seen.into_iter().collect();
            out_words.sort();
            let rendered = out_words.join("\n") + "\n";
            fs::write(&output, rendered).with_context(|| format!("writing {}", output.display()))?;

            eprintln!(
                "build-dict: lines={} normalized={} kept={} wrote={}",
                total_lines,
                normalized,
                kept,
                output.display()
            );
        }

        Command::GenerateMini {
            dict,
            out,
            size,
            time_limit_ms,
            title,
            author,
            seed,
            blocks,
            min_word_len,
        } => {
            if size == 0 || size > MXSZ {
                anyhow::bail!("--size must be in 1..={}", MXSZ);
            }
            let dict = dict.or_else(default_dict_path);
            let Some(dict_path) = dict else {
                anyhow::bail!("no dictionary provided and no default dictionary found");
            };

            let mut rng = match seed {
                Some(s) => StdRng::seed_from_u64(s),
                None => StdRng::from_entropy(),
            };

            let blocks_spec = parse_blocks_spec(&blocks)?;
            let time_limit = if time_limit_ms == 0 {
                None
            } else {
                Some(Duration::from_millis(time_limit_ms))
            };

            let puzzle = match blocks_spec {
                BlocksSpec::Fixed(0) => {
                let words_n = load_wordlist_len(&dict_path, size)
                    .with_context(|| format!("loading wordlist {}", dict_path.display()))?;
                if words_n.len() < 500 {
                    eprintln!(
                        "warning: only {} {}-letter words loaded; open-grid generation may fail",
                        words_n.len(),
                        size
                    );
                }
                if let Some(tl) = time_limit {
                    generate_mini_puzzle_from_wordlist_size_limited(&words_n, size, title, author, &mut rng, Some(tl))?
                } else {
                    generate_mini_puzzle_from_wordlist_size(&words_n, size, title, author, &mut rng)?
                }
                }
                BlocksSpec::Fixed(n) => {
                let lens: Vec<usize> = (min_word_len..=size).collect();
                let dictmap = load_wordlist_lens(&dict_path, &lens)
                    .with_context(|| format!("loading wordlist {}", dict_path.display()))?;
                for len in min_word_len..=size {
                    let n = dictmap.get(&len).map(|v| v.len()).unwrap_or(0);
                    if n < 200 {
                        eprintln!(
                            "warning: only {} {}-letter words loaded; blocked generation may fail",
                            n, len
                        );
                    }
                }
                if let Some(tl) = time_limit {
                    generate_mini_puzzle_blocked_from_wordlist_size_limited(
                        size,
                        &dictmap,
                        n,
                        min_word_len,
                        title,
                        author,
                        &mut rng,
                        Some(tl),
                    )?
                } else {
                    generate_mini_puzzle_blocked_from_wordlist_size(
                        size,
                        &dictmap,
                        n,
                        min_word_len,
                        title,
                        author,
                        &mut rng,
                    )?
                }
                }
                BlocksSpec::Auto => {
                    let lens: Vec<usize> = (min_word_len..=size).collect();
                    let dictmap = load_wordlist_lens(&dict_path, &lens)
                        .with_context(|| format!("loading wordlist {}", dict_path.display()))?;
                    for len in min_word_len..=size {
                        let n = dictmap.get(&len).map(|v| v.len()).unwrap_or(0);
                        if n < 200 {
                            eprintln!(
                                "warning: only {} {}-letter words loaded; blocked generation may fail",
                                n, len
                            );
                        }
                    }
                    if let Some(tl) = time_limit {
                        generate_mini_puzzle_blocked_auto_from_wordlist_size_limited(
                            size,
                            &dictmap,
                            min_word_len,
                            title,
                            author,
                            &mut rng,
                            Some(tl),
                        )?
                    } else {
                        generate_mini_puzzle_blocked_auto_from_wordlist_size(
                            size,
                            &dictmap,
                            min_word_len,
                            title,
                            author,
                            &mut rng,
                        )?
                    }
                }
            };
            save_qxw2(&puzzle, &out)?;
        }
    }

    Ok(())
}

fn normalize_word_ascii_alpha(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(t.len());
    for ch in t.chars() {
        if ch.is_ascii_alphabetic() {
            out.push(ch.to_ascii_uppercase());
        } else {
            // reject words with punctuation/diacritics/etc.
            return None;
        }
    }
    Some(out)
}

fn has_vowel(word: &str) -> bool {
    word.as_bytes()
        .iter()
        .any(|&b| matches!(b, b'A' | b'E' | b'I' | b'O' | b'U' | b'Y'))
}

fn count_consonants(word: &str) -> usize {
    word.as_bytes()
        .iter()
        .filter(|&&b| !matches!(b, b'A' | b'E' | b'I' | b'O' | b'U' | b'Y'))
        .count()
}

fn count_distinct_letters(word: &str) -> usize {
    let mut mask: u32 = 0;
    for &b in word.as_bytes() {
        if !(b'A'..=b'Z').contains(&b) {
            continue;
        }
        mask |= 1u32 << (b - b'A');
    }
    mask.count_ones() as usize
}

fn max_repeated_run(word: &str) -> usize {
    let bytes = word.as_bytes();
    if bytes.is_empty() {
        return 0;
    }
    let mut best = 1usize;
    let mut cur = 1usize;
    for i in 1..bytes.len() {
        if bytes[i] == bytes[i - 1] {
            cur += 1;
            best = best.max(cur);
        } else {
            cur = 1;
        }
    }
    best
}

fn default_dict_path() -> Option<PathBuf> {
    // Reasonable defaults across macOS/Linux. We pick the first that exists.
    let candidates = [
        "/usr/share/dict/words",
        "/usr/dict/words",
        "/usr/share/dict/web2",
    ];
    for c in candidates {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn render_dump(pz: &Puzzle, show_numbers: bool) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "title={:?}\nauthor={:?}\n\n",
        pz.title, pz.author
    ));
    out.push_str(&format!(
        "gtype={} width={} height={} ndir={} symmr={} symmm={} symmd={}\n",
        pz.gtype,
        pz.width,
        pz.height,
        pz.ndir(),
        pz.symmr,
        pz.symmm,
        pz.symmd
    ));

    let mut ingrid = 0usize;
    let mut clear = 0usize;
    let mut blocked = 0usize;
    let mut cutout = 0usize;
    let mut letters = 0usize;
    let mut bars = 0usize;
    let mut merges = 0usize;
    for y in 0..pz.height {
        for x in 0..pz.width {
            if !pz.is_ingrid(x, y) {
                continue;
            }
            ingrid += 1;
            let sq = pz.square(x, y).unwrap();
            if (sq.fl & 0x08) != 0 {
                cutout += 1;
                continue;
            }
            if (sq.fl & 0x01) != 0 {
                blocked += 1;
            } else {
                clear += 1;
            }
            if sq.ch != b' ' {
                letters += 1;
            }
            for d in 0..pz.ndir() {
                if pz.is_bar(x, y, d) {
                    bars += 1;
                }
                if pz.is_merge(x, y, d) {
                    merges += 1;
                }
            }
        }
    }

    out.push_str(&format!(
        "cells: ingrid={} clear={} blocked={} cutout={} letters={} bars={} merges={}\n\n",
        ingrid, clear, blocked, cutout, letters, bars, merges
    ));

    if pz.gtype == 0 {
        out.push_str(if show_numbers {
            "grid (gtype=0, bars shown, numbers):\n"
        } else {
            "grid (gtype=0, bars shown):\n"
        });

        let w = pz.width;
        let h = pz.height;
        let cell_w = 4;

        for y in 0..h {
            draw_hborder_g0(&mut out, pz, w, y, cell_w);

            // Cell contents with vertical bars
            out.push('|');
            for x in 0..w {
                let sq = pz.square(x, y).unwrap();
                if (sq.fl & 0x08) != 0 {
                    // cutout
                    for _ in 0..cell_w {
                        out.push(' ');
                    }
                } else if (sq.fl & 0x01) != 0 {
                    // blocked
                    for _ in 0..cell_w {
                        out.push('#');
                    }
                } else if show_numbers {
                    // Show both number (if any) and letter.
                    let letter = if sq.ch != b' ' { sq.ch as char } else { '.' };
                    if sq.number >= 0 {
                        // 4-wide cell: " 1A ", "12A ", etc.
                        let s = format!("{:>2}{} ", sq.number, letter);
                        out.push_str(&s);
                    } else {
                        // no number; show letter (or '.') centered-ish
                        out.push(' ');
                        out.push(letter);
                        out.push(' ');
                        out.push(' ');
                    }
                } else if sq.ch != b' ' {
                    // letter centered-ish
                    out.push(' ');
                    out.push(sq.ch as char);
                    for _ in 0..(cell_w - 2) {
                        out.push(' ');
                    }
                } else {
                    for _ in 0..cell_w {
                        out.push('.');
                    }
                }

                let wall = if x == w - 1 {
                    true
                } else {
                    // bar between (x,y) and (x+1,y)
                    pz.is_bar(x, y, 0)
                };
                out.push(if wall { '|' } else { ' ' });
            }
            out.push('\n');
        }

        draw_hborder_g0(&mut out, pz, w, h, cell_w);
    } else {
        out.push_str("(ASCII grid rendering currently only for gtype=0)\n");
    }

    out
}

fn draw_hborder_g0(out: &mut String, pz: &Puzzle, w: i32, y: i32, cell_w: usize) {
    out.push('+');
    for x in 0..w {
        let wall = if y == 0 {
            true
        } else if y == pz.height {
            true
        } else {
            // bar between (x,y-1) and (x,y)
            pz.is_bar(x, y - 1, 1)
        };
        for _ in 0..cell_w {
            out.push(if wall { '-' } else { ' ' });
        }
        out.push('+');
    }
    out.push('\n');
}

fn render_answers_text(pz: &Puzzle) -> String {
    let mut out = String::new();
    for d in 0..pz.ndir() {
        out.push_str(&format!("{}\n\n", DNAME[pz.gtype][d]));
        for (dir, _x, _y, mut word, number) in pz.iter_lights() {
            if dir != d {
                continue;
            }
            word = word.chars().map(|c| if c == ' ' { '.' } else { c }).collect();
            out.push_str(&format!("{} {} ({})\n", number, word, word.len()));
        }
        out.push('\n');
    }
    out
}

fn esc_html(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => o.push_str("&amp;"),
            '"' => o.push_str("&quot;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '|' => o.push_str("&#124;"),
            _ => o.push(ch),
        }
    }
    o
}

fn render_answers_html(pz: &Puzzle) -> String {
    let mut out = String::new();
    out.push_str("<html><body><!-- file generated by qxw-rs -->\n");
    for d in 0..pz.ndir() {
        out.push_str(&format!("\n<b>{}</b><br>\n\n", esc_html(DNAME[pz.gtype][d])));
        for (dir, _x, _y, mut word, number) in pz.iter_lights() {
            if dir != d {
                continue;
            }
            word = word.chars().map(|c| if c == ' ' { '.' } else { c }).collect();
            out.push_str(&format!("<b>{}</b> {} ({})<br>\n", number, esc_html(&word), word.len()));
        }
    }
    out.push_str("</body></html>\n");
    out
}
