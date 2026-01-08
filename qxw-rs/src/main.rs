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
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::thread;
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

    /// Generate a batch of daily puzzles (one per day of a year)
    GenerateYear {
        /// Output directory (one YYYY-MM-DD.qxw per day)
        out_dir: PathBuf,

        /// Year to generate (e.g. 2026)
        #[arg(long)]
        year: i32,

        /// Word list file (one word per line). Defaults to ./dict-wordfreq.txt if present,
        /// otherwise falls back to a system dictionary.
        #[arg(long)]
        dict: Option<PathBuf>,

        /// Grid sizes to cycle through (comma-separated)
        #[arg(long, value_delimiter = ',', default_value = "5")]
        sizes: Vec<usize>,

        /// Time limit in milliseconds per puzzle (0 disables the limit)
        #[arg(long, default_value_t = 5000)]
        time_limit_ms: u64,

        /// Minimum across/down entry length in blocked grids
        #[arg(long, default_value_t = 3)]
        min_word_len: usize,

        /// Optional author to embed in each puzzle
        #[arg(long)]
        author: Option<String>,

        /// Base RNG seed (combined with date). Defaults to the year.
        #[arg(long)]
        seed: Option<u64>,

        /// Overwrite existing output files
        #[arg(long)]
        overwrite: bool,
    },

    /// Export a rectangular (gtype=0) puzzle to iPuz JSON
    ExportIpuz {
        /// Input .qxw file
        input: PathBuf,

        /// Optional clue file in the simple "mini-clues.txt" format
        #[arg(long)]
        clues: Option<PathBuf>,

        /// Output .ipuz path
        output: PathBuf,
    },

    /// Export one or more puzzles as printable SVGs (completed grid)
    ExportSvg {
        /// Output directory (one .svg per input)
        out_dir: PathBuf,

        /// Input .qxw files
        inputs: Vec<PathBuf>,

        /// Cell size in pixels
        #[arg(long, default_value_t = 40)]
        cell: u32,

        /// Do not print letters (blank grid)
        #[arg(long)]
        blank: bool,

        /// Do not print clue numbers
        #[arg(long)]
        no_numbers: bool,

        /// Overwrite existing output files
        #[arg(long)]
        overwrite: bool,
    },

    /// Generate clue files for a directory of .qxw puzzles using the OpenAI API
    GenerateCluesOpenai {
        /// Input directory containing .qxw files
        puzzles_dir: PathBuf,

        /// Output directory for clue files (mini-clues format)
        #[arg(long, default_value = "puzzles/clues")]
        clues_dir: PathBuf,

        /// System prompt path (defaults to clues_dir/system-prompt.txt if present)
        #[arg(long)]
        system_prompt: Option<PathBuf>,

        /// Path to .env file containing OPENAI_API_KEY or OPENAI-KEY
        #[arg(long, default_value = ".env")]
        env: PathBuf,

        /// OpenAI model name (e.g. gpt-4o-mini)
        #[arg(long, default_value = "gpt-4o-mini")]
        model: String,

        /// Temperature (0.0-2.0)
        #[arg(long, default_value_t = 0.7)]
        temperature: f32,

        /// Sleep this many milliseconds between API requests (helps avoid rate limits)
        #[arg(long, default_value_t = 0)]
        delay_ms: u64,

        /// Only generate at most this many puzzles (for testing)
        #[arg(long)]
        max: Option<usize>,

        /// Overwrite existing clue files
        #[arg(long)]
        overwrite: bool,

        /// Stop immediately on the first error (default: continue and report failures)
        #[arg(long)]
        fail_fast: bool,

        /// Do not call the API; just print what would be generated
        #[arg(long)]
        dry_run: bool,
    },

    /// Diagnostic: make a tiny OpenAI API request and print detailed errors
    OpenaiDiagnose {
        /// Path to .env file containing OPENAI_API_KEY or OPENAI-KEY
        #[arg(long, default_value = ".env")]
        env: PathBuf,

        /// OpenAI model name for the chat test
        #[arg(long, default_value = "gpt-4o-mini")]
        model: String,

        /// Temperature for the chat test
        #[arg(long, default_value_t = 0.0)]
        temperature: f32,

        /// Timeout (seconds)
        #[arg(long, default_value_t = 30)]
        timeout_s: u64,
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

        Command::GenerateYear {
            out_dir,
            year,
            dict,
            sizes,
            time_limit_ms,
            min_word_len,
            author,
            seed,
            overwrite,
        } => {
            if sizes.is_empty() {
                anyhow::bail!("--sizes must contain at least one size");
            }
            for &s in &sizes {
                if s == 0 || s > MXSZ {
                    anyhow::bail!("invalid size {s} in --sizes (must be in 1..={})", MXSZ);
                }
                if min_word_len == 0 || min_word_len > s {
                    anyhow::bail!("--min-word-len must be in 1..={s} for size {s}");
                }
            }

            let dict = dict.or_else(default_batch_dict_path);
            let Some(dict_path) = dict else {
                anyhow::bail!("no dictionary provided and no default dictionary found");
            };

            fs::create_dir_all(&out_dir)
                .with_context(|| format!("creating output dir {}", out_dir.display()))?;

            let time_limit = if time_limit_ms == 0 {
                None
            } else {
                Some(Duration::from_millis(time_limit_ms))
            };

            // Pre-load word lists per size so we don't re-read the dictionary 365 times.
            let mut open_words_by_size: HashMap<usize, Vec<String>> = HashMap::new();
            let mut blocked_dict_by_size: HashMap<usize, HashMap<usize, Vec<String>>> = HashMap::new();

            for &size in &sizes {
                let words_n = load_wordlist_len(&dict_path, size)
                    .with_context(|| format!("loading wordlist {}", dict_path.display()))?;
                open_words_by_size.insert(size, words_n);

                let lens: Vec<usize> = (min_word_len..=size).collect();
                let dictmap = load_wordlist_lens(&dict_path, &lens)
                    .with_context(|| format!("loading wordlist {}", dict_path.display()))?;
                blocked_dict_by_size.insert(size, dictmap);
            }

            let mut seen: HashSet<String> = HashSet::new();
            let base_seed: u64 = seed.unwrap_or(year as u64);

            let mut generated = 0usize;
            for (month, day) in iter_days_in_year(year) {
                let date_str = format!("{year:04}-{month:02}-{day:02}");
                let out_path = out_dir.join(format!("{date_str}.qxw"));
                if out_path.exists() && !overwrite {
                    anyhow::bail!(
                        "output already exists: {} (use --overwrite)",
                        out_path.display()
                    );
                }

                // Find a unique puzzle for this day, retrying with bumped seeds.
                let mut day_ok = false;
                for bump in 0u64..200 {
                    let s = mix64(base_seed ^ ymd_seed(year, month, day) ^ (bump.wrapping_mul(0x9E37_79B9_7F4A_7C15)));
                    let mut rng = StdRng::seed_from_u64(s);

                    let size = sizes[(s as usize) % sizes.len()];
                    let title = Some(date_str.clone());
                    let author = author.clone();

                    let layout_roll = ((s >> 8) % 10) as u8;
                    let pz_res: Result<Puzzle> = match layout_roll {
                        0 => {
                            // Open (word-square) day.
                            let words_n = open_words_by_size
                                .get(&size)
                                .with_context(|| format!("missing cached wordlist for size {size}"))?;
                            if let Some(tl) = time_limit {
                                generate_mini_puzzle_from_wordlist_size_limited(
                                    words_n,
                                    size,
                                    title,
                                    author,
                                    &mut rng,
                                    Some(tl),
                                )
                            } else {
                                generate_mini_puzzle_from_wordlist_size(words_n, size, title, author, &mut rng)
                            }
                        }
                        8 | 9 => {
                            // Blocked auto day.
                            let dictmap = blocked_dict_by_size
                                .get(&size)
                                .with_context(|| format!("missing cached dictmap for size {size}"))?;
                            if let Some(tl) = time_limit {
                                generate_mini_puzzle_blocked_auto_from_wordlist_size_limited(
                                    size,
                                    dictmap,
                                    min_word_len,
                                    title,
                                    author,
                                    &mut rng,
                                    Some(tl),
                                )
                            } else {
                                generate_mini_puzzle_blocked_auto_from_wordlist_size(
                                    size,
                                    dictmap,
                                    min_word_len,
                                    title,
                                    author,
                                    &mut rng,
                                )
                            }
                        }
                        _ => {
                            // Blocked fixed day.
                            let dictmap = blocked_dict_by_size
                                .get(&size)
                                .with_context(|| format!("missing cached dictmap for size {size}"))?;
                            let cands = block_candidates(size, min_word_len);
                            if cands.is_empty() {
                                anyhow::bail!("no valid block candidates for size={size} min_word_len={min_word_len}");
                            }
                            let blocks = cands[((s >> 16) as usize) % cands.len()];
                            if let Some(tl) = time_limit {
                                generate_mini_puzzle_blocked_from_wordlist_size_limited(
                                    size,
                                    dictmap,
                                    blocks,
                                    min_word_len,
                                    title,
                                    author,
                                    &mut rng,
                                    Some(tl),
                                )
                            } else {
                                generate_mini_puzzle_blocked_from_wordlist_size(
                                    size,
                                    dictmap,
                                    blocks,
                                    min_word_len,
                                    title,
                                    author,
                                    &mut rng,
                                )
                            }
                        }
                    };

                    let Ok(pz) = pz_res else {
                        continue;
                    };
                    let sig = puzzle_signature(&pz);
                    if seen.insert(sig) {
                        save_qxw2(&pz, &out_path)?;
                        day_ok = true;
                        break;
                    }
                }

                if !day_ok {
                    anyhow::bail!("failed to generate a unique puzzle for {date_str} after many attempts");
                }

                generated += 1;
                if generated % 25 == 0 {
                    eprintln!("generated {generated} puzzles...");
                }
            }

            eprintln!("generated {generated} puzzles in {}", out_dir.display());
        }

        Command::ExportIpuz {
            input,
            clues,
            output,
        } => {
            let mut puzzle = load_qxw(&input).with_context(|| format!("loading {}", input.display()))?;
            puzzle.compute_numbers();
            let clue_text = match clues {
                Some(p) => Some(
                    fs::read_to_string(&p)
                        .with_context(|| format!("reading clues {}", p.display()))?,
                ),
                None => None,
            };
            let ipuz = build_ipuz(&puzzle, clue_text.as_deref())?;
            let json = serde_json::to_string_pretty(&ipuz)?;
            fs::write(&output, json + "\n")
                .with_context(|| format!("writing {}", output.display()))?;
        }

        Command::ExportSvg {
            out_dir,
            inputs,
            cell,
            blank,
            no_numbers,
            overwrite,
        } => {
            if inputs.is_empty() {
                anyhow::bail!("export-svg requires at least one input .qxw file");
            }
            fs::create_dir_all(&out_dir)
                .with_context(|| format!("creating output dir {}", out_dir.display()))?;

            for input in inputs {
                let mut puzzle =
                    load_qxw(&input).with_context(|| format!("loading {}", input.display()))?;
                puzzle.compute_numbers();

                if puzzle.gtype != 0 {
                    anyhow::bail!(
                        "SVG export currently only supports gtype=0 rectangular puzzles (got gtype={})",
                        puzzle.gtype
                    );
                }

                let stem = input
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("puzzle");
                let out_path = out_dir.join(format!("{stem}.svg"));
                if out_path.exists() && !overwrite {
                    anyhow::bail!(
                        "output already exists: {} (use --overwrite)",
                        out_path.display()
                    );
                }

                let svg = render_puzzle_svg(
                    &puzzle,
                    SvgRenderOptions {
                        cell_px: cell,
                        show_letters: !blank,
                        show_numbers: !no_numbers,
                    },
                );
                fs::write(&out_path, svg)
                    .with_context(|| format!("writing {}", out_path.display()))?;
            }
        }

        Command::GenerateCluesOpenai {
            puzzles_dir,
            clues_dir,
            system_prompt,
            env,
            model,
            temperature,
            delay_ms,
            max,
            overwrite,
            fail_fast,
            dry_run,
        } => {
            let api_key = read_openai_key_from_env_file(&env)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "no OpenAI API key found in {} (set OPENAI_API_KEY=... or OPENAI-KEY=...)",
                    env.display()
                )
            })?;

            fs::create_dir_all(&clues_dir)
                .with_context(|| format!("creating output dir {}", clues_dir.display()))?;

            let sys_path = system_prompt
                .or_else(|| {
                    let p = clues_dir.join("system-prompt.txt");
                    if p.exists() { Some(p) } else { None }
                });
            let system_text = match sys_path {
                Some(p) => Some(
                    sanitize_system_prompt(&fs::read_to_string(&p)
                        .with_context(|| format!("reading system prompt {}", p.display()))?),
                ),
                None => None,
            };

            let mut inputs = collect_qxw_files(&puzzles_dir)
                .with_context(|| format!("scanning {}", puzzles_dir.display()))?;
            inputs.sort();
            if let Some(n) = max {
                inputs.truncate(n);
            }
            if inputs.is_empty() {
                anyhow::bail!("no .qxw files found in {}", puzzles_dir.display());
            }

            if dry_run {
                eprintln!("dry-run: would generate {} clue files into {}", inputs.len(), clues_dir.display());
                for p in &inputs {
                    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("puzzle");
                    let out_path = clues_dir.join(format!("{stem}.txt"));
                    eprintln!("{} -> {}", p.display(), out_path.display());
                }
                return Ok(());
            }

            let agent = ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(20))
                .timeout_read(Duration::from_secs(120))
                .timeout_write(Duration::from_secs(120))
                .build();

            let mut done = 0usize;
            let mut skipped = 0usize;
            let mut failed = 0usize;
            for input in inputs {
                let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("puzzle");
                let out_path = clues_dir.join(format!("{stem}.txt"));
                if out_path.exists() && !overwrite {
                    skipped += 1;
                    continue;
                }

                if delay_ms > 0 {
                    thread::sleep(Duration::from_millis(delay_ms));
                }

                let mut puzzle =
                    load_qxw(&input).with_context(|| format!("loading {}", input.display()))?;
                puzzle.compute_numbers();
                if puzzle.gtype != 0 {
                    anyhow::bail!(
                        "clue generation currently only supports gtype=0 rectangular puzzles (got gtype={})",
                        puzzle.gtype
                    );
                }

                let (across, down) = puzzle_answer_lists(&puzzle);
                let user_prompt = build_clue_user_prompt(&puzzle, &across, &down);

                let mut last_err: Option<anyhow::Error> = None;
                let mut text: Option<String> = None;
                for attempt in 0..3u32 {
                    if attempt > 0 {
                        // Small backoff between retries.
                        thread::sleep(Duration::from_secs((attempt * 2) as u64));
                    }

                    let resp = openai_chat_completion(
                        &agent,
                        &api_key,
                        &model,
                        temperature,
                        system_text.as_deref(),
                        &user_prompt,
                    );

                    match resp {
                        Ok(t) => {
                            if let Err(e) = validate_mini_clues_for_puzzle(&puzzle, &t) {
                                last_err = Some(e);
                                // Try one more time with a stricter format reminder.
                                let strict_prompt = format!(
                                    "Your previous output did not match the required format. \
Rewrite ONLY the clues in the exact mini-clues format.\n\n{}",
                                    user_prompt
                                );
                                let retry = openai_chat_completion(
                                    &agent,
                                    &api_key,
                                    &model,
                                    temperature,
                                    system_text.as_deref(),
                                    &strict_prompt,
                                );
                                match retry {
                                    Ok(t2) => {
                                        if let Err(e2) = validate_mini_clues_for_puzzle(&puzzle, &t2) {
                                            last_err = Some(e2);
                                            continue;
                                        }
                                        text = Some(t2);
                                        break;
                                    }
                                    Err(e2) => {
                                        last_err = Some(e2);
                                        continue;
                                    }
                                }
                            }
                            text = Some(t);
                            break;
                        }
                        Err(e) => {
                            last_err = Some(e);
                            continue;
                        }
                    }
                }

                let Some(text) = text else {
                    failed += 1;
                    let err = last_err
                        .unwrap_or_else(|| anyhow::anyhow!("failed to generate clues (unknown error)"));
                    if fail_fast {
                        return Err(err).with_context(|| {
                            format!("generating clues for {}", input.display())
                        });
                    } else {
                        eprintln!("clues: FAILED {}: {err:#}", input.display());
                        continue;
                    }
                };

                fs::write(&out_path, text.trim_end().to_string() + "\n")
                    .with_context(|| format!("writing {}", out_path.display()))?;

                done += 1;
                if (done + skipped) % 10 == 0 {
                    eprintln!("clues: done={} skipped={} failed={}", done, skipped, failed);
                }
            }

            eprintln!(
                "clues: done={} skipped={} failed={} out={}",
                done,
                skipped,
                failed,
                clues_dir.display()
            );
        }

        Command::OpenaiDiagnose {
            env,
            model,
            temperature,
            timeout_s,
        } => {
            let api_key = read_openai_key_from_env_file(&env)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "no OpenAI API key found in {} (set OPENAI_API_KEY=... or OPENAI-KEY=...)",
                    env.display()
                )
            })?;

            let agent = ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(timeout_s))
                .timeout_read(Duration::from_secs(timeout_s))
                .timeout_write(Duration::from_secs(timeout_s))
                .build();

            println!("OpenAI diagnose");
            println!("- env: {}", env.display());
            println!("- model: {model}");

            println!("\n1) GET /v1/models");
            match openai_get_models(&agent, &api_key) {
                Ok(ids) => {
                    println!("- OK ({} models visible)", ids.len());
                    let preview: Vec<String> = ids.into_iter().take(8).collect();
                    if !preview.is_empty() {
                        println!("- sample: {}", preview.join(", "));
                    }
                }
                Err(e) => {
                    println!("- FAILED: {e:#}");
                }
            }

            println!("\n2) POST /v1/chat/completions (tiny)");
            match openai_chat_completion_tiny(&agent, &api_key, &model, temperature) {
                Ok(text) => {
                    println!("- OK");
                    println!("- content: {}", text.trim());
                }
                Err(e) => {
                    println!("- FAILED: {e:#}");
                    println!(
                        "\nIf you see 429 insufficient_quota, billing/credits/project budget is blocking API calls; usage may show 0 because requests are rejected before billing."
                    );
                }
            }
        }
    }

    Ok(())
}

fn collect_qxw_files(dir: &PathBuf) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        anyhow::bail!("directory does not exist: {}", dir.display());
    }
    let mut out = Vec::new();
    for ent in fs::read_dir(dir).with_context(|| format!("reading dir {}", dir.display()))? {
        let ent = ent?;
        let p = ent.path();
        if p.is_file() {
            if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
                if ext.eq_ignore_ascii_case("qxw") {
                    out.push(p);
                }
            }
        }
    }
    Ok(out)
}

fn sanitize_system_prompt(s: &str) -> String {
    // Remove stray Markdown fence markers if present.
    let mut lines: Vec<&str> = s.lines().collect();
    while let Some(last) = lines.last() {
        let t = last.trim();
        if t == "```" || t == "``" {
            lines.pop();
        } else {
            break;
        }
    }
    lines.join("\n").trim().to_string()
}

fn read_openai_key_from_env_file(path: &PathBuf) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let key = k.trim();
        if key != "OPENAI_API_KEY" && key != "OPENAI_KEY" && key != "OPENAI-KEY" {
            continue;
        }
        let mut val = v.trim().to_string();
        if (val.starts_with('"') && val.ends_with('"')) || (val.starts_with('\'') && val.ends_with('\'')) {
            val = val[1..val.len() - 1].to_string();
        }
        if !val.is_empty() {
            return Ok(Some(val));
        }
    }
    Ok(None)
}

fn puzzle_answer_lists(pz: &Puzzle) -> (Vec<(u32, String)>, Vec<(u32, String)>) {
    // For gtype=0, dir=0 is Across and dir=1 is Down.
    let mut across = Vec::new();
    let mut down = Vec::new();
    for (dir, _x, _y, word, number) in pz.iter_lights() {
        let w = word.replace(' ', "");
        if dir == 0 {
            across.push((number as u32, w));
        } else if dir == 1 {
            down.push((number as u32, w));
        }
    }
    across.sort_by_key(|(n, _)| *n);
    down.sort_by_key(|(n, _)| *n);
    (across, down)
}

fn build_clue_user_prompt(pz: &Puzzle, across: &[(u32, String)], down: &[(u32, String)]) -> String {
    let mut out = String::new();
    out.push_str("Write crossword clues for this puzzle. Return ONLY the clues in the exact mini-clues format shown below.\n\n");
    if !pz.title.trim().is_empty() {
        out.push_str(&format!("Title: {}\n", pz.title.trim()));
    }
    if !pz.author.trim().is_empty() {
        out.push_str(&format!("Author: {}\n", pz.author.trim()));
    }
    out.push_str(&format!("Grid: {}x{}\n\n", pz.width, pz.height));

    out.push_str("Answers (with numbers):\n\nAcross\n");
    for (n, w) in across {
        out.push_str(&format!("{n}A {w}\n"));
    }
    out.push_str("\nDown\n");
    for (n, w) in down {
        out.push_str(&format!("{n}D {w}\n"));
    }

    out.push_str("\nRequired output format (example):\n");
    out.push_str("Across\n1A ANSWER — clue text\n...\n\nDown\n1D ANSWER — clue text\n...\n");
    out.push_str("\nDo not add any extra commentary or headings.\n");
    out
}

fn validate_mini_clues_for_puzzle(pz: &Puzzle, text: &str) -> Result<()> {
    let parsed = parse_mini_clues(text)?;
    let (exp_across, exp_down) = puzzle_answer_lists(pz);
    let got_across: Vec<u32> = parsed.across.iter().map(|(n, _)| *n).collect();
    let got_down: Vec<u32> = parsed.down.iter().map(|(n, _)| *n).collect();
    let exp_across_nums: Vec<u32> = exp_across.into_iter().map(|(n, _)| n).collect();
    let exp_down_nums: Vec<u32> = exp_down.into_iter().map(|(n, _)| n).collect();
    if got_across != exp_across_nums {
        anyhow::bail!("across clue numbers mismatch");
    }
    if got_down != exp_down_nums {
        anyhow::bail!("down clue numbers mismatch");
    }
    Ok(())
}

fn openai_chat_completion(
    agent: &ureq::Agent,
    api_key: &str,
    model: &str,
    temperature: f32,
    system_prompt: Option<&str>,
    user_prompt: &str,
) -> Result<String> {
    let mut messages = Vec::new();
    if let Some(sys) = system_prompt {
        if !sys.trim().is_empty() {
            messages.push(serde_json::json!({"role": "system", "content": sys}));
        }
    }
    messages.push(serde_json::json!({"role": "user", "content": user_prompt}));

    let body = serde_json::json!({
        "model": model,
        "temperature": temperature,
        "messages": messages,
    });

    let req = agent
        .post("https://api.openai.com/v1/chat/completions")
        .set("Authorization", &format!("Bearer {api_key}"))
        .set("Content-Type", "application/json");

    // Basic retries on transient errors.
    for attempt in 0u32..4 {
        if attempt > 0 {
            thread::sleep(Duration::from_secs((attempt * 2) as u64));
        }
        match req.clone().send_json(body.clone()) {
            Ok(resp) => {
                let v: serde_json::Value = resp.into_json()?;
                let content = v
                    .pointer("/choices/0/message/content")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| anyhow::anyhow!("unexpected OpenAI response shape"))?;
                return Ok(content.to_string());
            }
            Err(ureq::Error::Status(code, r)) => {
                // 429/5xx are often transient.
                let retryable = code == 429 || (500..600).contains(&code);
                if retryable && attempt < 3 {
                    // Try to consume body for helpful context, but don't fail if it can't be read.
                    let _ = r.into_string();
                    continue;
                }
                let msg = r.into_string().unwrap_or_else(|_| format!("HTTP {code}"));
                anyhow::bail!("OpenAI API error {code}: {msg}");
            }
            Err(e) => {
                if attempt < 3 {
                    continue;
                }
                return Err(anyhow::anyhow!(e));
            }
        }
    }
    anyhow::bail!("OpenAI request failed after retries")
}

fn openai_get_models(agent: &ureq::Agent, api_key: &str) -> Result<Vec<String>> {
    let req = agent
        .get("https://api.openai.com/v1/models")
        .set("Authorization", &format!("Bearer {api_key}"));
    match req.call() {
        Ok(resp) => {
            let v: serde_json::Value = resp.into_json()?;
            let arr = v
                .get("data")
                .and_then(|d| d.as_array())
                .ok_or_else(|| anyhow::anyhow!("unexpected /v1/models response shape"))?;
            let mut ids = Vec::new();
            for m in arr {
                if let Some(id) = m.get("id").and_then(|x| x.as_str()) {
                    ids.push(id.to_string());
                }
            }
            ids.sort();
            Ok(ids)
        }
        Err(ureq::Error::Status(code, r)) => {
            let req_id = r.header("x-request-id").map(|s| s.to_string());
            let msg = r.into_string().unwrap_or_else(|_| format!("HTTP {code}"));
            if let Some(req_id) = req_id {
                anyhow::bail!("OpenAI API error {code} (x-request-id={req_id}): {msg}");
            }
            anyhow::bail!("OpenAI API error {code}: {msg}");
        }
        Err(e) => Err(anyhow::anyhow!(e)),
    }
}

fn openai_chat_completion_tiny(
    agent: &ureq::Agent,
    api_key: &str,
    model: &str,
    temperature: f32,
) -> Result<String> {
    let body = serde_json::json!({
        "model": model,
        "temperature": temperature,
        "max_tokens": 16,
        "messages": [
            {"role": "user", "content": "Reply with exactly: OK"}
        ]
    });

    let req = agent
        .post("https://api.openai.com/v1/chat/completions")
        .set("Authorization", &format!("Bearer {api_key}"))
        .set("Content-Type", "application/json");

    match req.send_json(body) {
        Ok(resp) => {
            let v: serde_json::Value = resp.into_json()?;
            let content = v
                .pointer("/choices/0/message/content")
                .and_then(|x| x.as_str())
                .ok_or_else(|| anyhow::anyhow!("unexpected OpenAI response shape"))?;
            Ok(content.to_string())
        }
        Err(ureq::Error::Status(code, r)) => {
            let req_id = r.header("x-request-id").map(|s| s.to_string());
            let msg = r.into_string().unwrap_or_else(|_| format!("HTTP {code}"));
            if let Some(req_id) = req_id {
                anyhow::bail!("OpenAI API error {code} (x-request-id={req_id}): {msg}");
            }
            anyhow::bail!("OpenAI API error {code}: {msg}");
        }
        Err(e) => Err(anyhow::anyhow!(e)),
    }
}

#[derive(Debug, Clone, Copy)]
struct SvgRenderOptions {
    cell_px: u32,
    show_letters: bool,
    show_numbers: bool,
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

fn render_puzzle_svg(pz: &Puzzle, opt: SvgRenderOptions) -> String {
    let w = pz.width.max(0) as u32;
    let h = pz.height.max(0) as u32;
    let cell = opt.cell_px.max(10);
    let margin = 12u32;
    let header_h = if pz.title.is_empty() && pz.author.is_empty() {
        0u32
    } else {
        36u32
    };

    let grid_w = w * cell;
    let grid_h = h * cell;
    let svg_w = margin * 2 + grid_w;
    let svg_h = margin * 2 + header_h + grid_h;

    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{svg_w}\" height=\"{svg_h}\" viewBox=\"0 0 {svg_w} {svg_h}\">\n"
    ));
    out.push_str("<rect x=\"0\" y=\"0\" width=\"100%\" height=\"100%\" fill=\"white\"/>\n");

    if header_h > 0 {
        let title = xml_escape(&pz.title);
        let author = xml_escape(&pz.author);
        let header_y = margin + 18;
        if !title.is_empty() {
            out.push_str(&format!(
                "<text x=\"{margin}\" y=\"{header_y}\" font-family=\"sans-serif\" font-size=\"16\" font-weight=\"700\" fill=\"black\">{title}</text>\n"
            ));
        }
        if !author.is_empty() {
            out.push_str(&format!(
                "<text x=\"{margin}\" y=\"{}\" font-family=\"sans-serif\" font-size=\"12\" fill=\"black\">{author}</text>\n",
                header_y + 14
            ));
        }
    }

    let origin_x = margin;
    let origin_y = margin + header_h;

    // Draw cell backgrounds + contents.
    for y in 0..h {
        for x in 0..w {
            let xi = origin_x + x * cell;
            let yi = origin_y + y * cell;
            if !pz.is_ingrid(x as i32, y as i32) {
                continue;
            }
            let sq = pz.square(x as i32, y as i32).unwrap();
            if (sq.fl & 0x08) != 0 {
                // Cutout: skip entirely.
                continue;
            }

            let is_block = (sq.fl & 0x01) != 0;
            let fill = if is_block { "black" } else { "white" };
            out.push_str(&format!(
                "<rect x=\"{xi}\" y=\"{yi}\" width=\"{cell}\" height=\"{cell}\" fill=\"{fill}\" stroke=\"black\" stroke-width=\"1\"/>\n"
            ));

            if is_block {
                continue;
            }

            if opt.show_numbers {
                let n = sq.number;
                if n > 0 {
                    let nx = xi + 3;
                    let ny = yi + 11;
                    out.push_str(&format!(
                        "<text x=\"{nx}\" y=\"{ny}\" font-family=\"sans-serif\" font-size=\"10\" fill=\"black\">{}</text>\n",
                        n
                    ));
                }
            }

            if opt.show_letters {
                let ch = sq.ch;
                if ch != b' ' {
                    let cx = xi + (cell / 2);
                    let cy = yi + (cell / 2) + (cell / 6);
                    let letter = xml_escape(&(ch as char).to_string());
                    let font_size = (cell as f32 * 0.55).round() as u32;
                    out.push_str(&format!(
                        "<text x=\"{cx}\" y=\"{cy}\" text-anchor=\"middle\" font-family=\"sans-serif\" font-size=\"{font_size}\" font-weight=\"700\" fill=\"black\">{letter}</text>\n"
                    ));
                }
            }
        }
    }

    out.push_str("</svg>\n");
    out
}

#[derive(Debug, Serialize)]
struct IpuzDimensions {
    width: usize,
    height: usize,
}

#[derive(Debug, Serialize)]
struct Ipuz {
    // iPuz uses a URL-like string for version/kind.
    version: String,
    kind: Vec<String>,
    dimensions: IpuzDimensions,

    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    author: Option<String>,

    // "puzzle" uses numbering: # for blocks, number for starts, 0 otherwise
    puzzle: Vec<Vec<serde_json::Value>>,

    // "solution" uses letters and # for blocks
    solution: Vec<Vec<serde_json::Value>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    clues: Option<IpuzClues>,
}

#[derive(Debug, Serialize)]
struct IpuzClues {
    #[serde(rename = "Across")]
    across: Vec<(u32, String)>,

    #[serde(rename = "Down")]
    down: Vec<(u32, String)>,
}

fn build_ipuz(pz: &Puzzle, clues_text: Option<&str>) -> Result<Ipuz> {
    if pz.gtype != 0 {
        anyhow::bail!("iPuz export currently only supports gtype=0 rectangular puzzles");
    }

    let width = pz.width as usize;
    let height = pz.height as usize;

    let mut puzzle_grid: Vec<Vec<serde_json::Value>> = Vec::with_capacity(height);
    let mut solution_grid: Vec<Vec<serde_json::Value>> = Vec::with_capacity(height);

    for y in 0..pz.height {
        let mut prow: Vec<serde_json::Value> = Vec::with_capacity(width);
        let mut srow: Vec<serde_json::Value> = Vec::with_capacity(width);
        for x in 0..pz.width {
            let sq = pz.square(x, y).ok_or_else(|| anyhow::anyhow!("missing square"))?;
            let is_cutout = (sq.fl & 0x08) != 0;
            let is_block = (sq.fl & 0x01) != 0;

            if is_cutout || is_block {
                prow.push(serde_json::Value::String("#".to_string()));
                srow.push(serde_json::Value::String("#".to_string()));
                continue;
            }

            if sq.number >= 0 {
                prow.push(serde_json::Value::Number((sq.number as i64).into()));
            } else {
                prow.push(serde_json::Value::Number(0.into()));
            }

            let ch = if sq.ch == b' ' { ' ' } else { sq.ch as char };
            if ch == ' ' {
                srow.push(serde_json::Value::String("".to_string()));
            } else {
                srow.push(serde_json::Value::String(ch.to_string()));
            }
        }
        puzzle_grid.push(prow);
        solution_grid.push(srow);
    }

    let clues = clues_text.map(parse_mini_clues).transpose()?;

    Ok(Ipuz {
        version: "http://ipuz.org/v2".to_string(),
        kind: vec!["http://ipuz.org/crossword#1".to_string()],
        dimensions: IpuzDimensions { width, height },
        title: if pz.title.trim().is_empty() { None } else { Some(pz.title.clone()) },
        author: if pz.author.trim().is_empty() { None } else { Some(pz.author.clone()) },
        puzzle: puzzle_grid,
        solution: solution_grid,
        clues,
    })
}

fn parse_mini_clues(text: &str) -> Result<IpuzClues> {
    // Expected format:
    // Across\n\n1A WORD — clue text\n...
    // Down\n\n1D WORD — clue text\n...
    let mut across: Vec<(u32, String)> = Vec::new();
    let mut down: Vec<(u32, String)> = Vec::new();

    #[derive(Clone, Copy)]
    enum Sec {
        None,
        Across,
        Down,
    }
    let mut sec = Sec::None;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line.eq_ignore_ascii_case("across") {
            sec = Sec::Across;
            continue;
        }
        if line.eq_ignore_ascii_case("down") {
            sec = Sec::Down;
            continue;
        }
        if matches!(sec, Sec::None) {
            continue;
        }

        // Split "1A SUE — clue" into left/right on an em dash (preferred) or a hyphen.
        let (lhs, rhs) = if let Some((a, b)) = line.split_once('—') {
            (a.trim(), b.trim())
        } else if let Some((a, b)) = line.split_once("--") {
            (a.trim(), b.trim())
        } else if let Some((a, b)) = line.split_once('-') {
            (a.trim(), b.trim())
        } else {
            anyhow::bail!("clue line missing separator (expected '—' or '-'): {line:?}");
        };

        // LHS begins with something like "12A" or "12D".
        let mut it = lhs.split_whitespace();
        let tok = it
            .next()
            .ok_or_else(|| anyhow::anyhow!("bad clue line: {line:?}"))?;

        let mut digits = String::new();
        let mut dir: Option<char> = None;
        for ch in tok.chars() {
            if ch.is_ascii_digit() {
                digits.push(ch);
            } else if ch.eq_ignore_ascii_case(&'A') || ch.eq_ignore_ascii_case(&'D') {
                dir = Some(ch.to_ascii_uppercase());
                break;
            } else {
                break;
            }
        }

        let n: u32 = digits
            .parse()
            .with_context(|| format!("bad clue number in {tok:?}"))?;
        let dir = dir.ok_or_else(|| anyhow::anyhow!("bad clue direction in {tok:?}"))?;

        let clue = rhs.to_string();
        match (sec, dir) {
            (Sec::Across, 'A') => across.push((n, clue)),
            (Sec::Down, 'D') => down.push((n, clue)),
            // If the file is inconsistent (e.g. section says Across but token says D), trust token.
            (_, 'A') => across.push((n, clue)),
            (_, 'D') => down.push((n, clue)),
            _ => {}
        }
    }

    across.sort_by_key(|(n, _)| *n);
    down.sort_by_key(|(n, _)| *n);

    Ok(IpuzClues { across, down })
}

#[cfg(test)]
mod ipuz_tests {
    use super::*;

    #[test]
    fn parses_mini_clues_basic() {
        let t = "Across\n\n1A SUE — take a girl to court\n\nDown\n1D SAM — Uncle ___";
        let c = parse_mini_clues(t).unwrap();
        assert_eq!(c.across.len(), 1);
        assert_eq!(c.across[0].0, 1);
        assert_eq!(c.down.len(), 1);
        assert_eq!(c.down[0].0, 1);
    }
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

fn default_batch_dict_path() -> Option<PathBuf> {
    // Prefer the repo-provided wordfreq dictionary when running from qxw-rs.
    let p = PathBuf::from("dict-wordfreq.txt");
    if p.exists() {
        return Some(p);
    }
    default_dict_path()
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0) && ((year % 100 != 0) || (year % 400 == 0))
}

fn iter_days_in_year(year: i32) -> impl Iterator<Item = (u32, u32)> {
    let feb = if is_leap_year(year) { 29 } else { 28 };
    let month_days: [u32; 12] = [31, feb, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut out = Vec::new();
    for (i, &days) in month_days.iter().enumerate() {
        let m = (i + 1) as u32;
        for d in 1..=days {
            out.push((m, d));
        }
    }
    out.into_iter()
}

fn ymd_seed(year: i32, month: u32, day: u32) -> u64 {
    // Pack YYYYMMDD into a u64.
    (year as u64) * 10_000 + (month as u64) * 100 + (day as u64)
}

fn mix64(mut x: u64) -> u64 {
    // SplitMix64 mixer.
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn puzzle_signature(pz: &Puzzle) -> String {
    let w = pz.width.max(0) as usize;
    let h = pz.height.max(0) as usize;
    let mut out = String::with_capacity(16 + w * h);
    out.push_str(&format!("{w}x{h}:"));
    for y in 0..h {
        for x in 0..w {
            let sq = pz.square(x as i32, y as i32).unwrap();
            if (sq.fl & 0x01) != 0 {
                out.push('#');
            } else if sq.ch == b' ' {
                out.push('.');
            } else {
                out.push(sq.ch as char);
            }
        }
        out.push('/');
    }
    out
}

fn block_candidates(size: usize, min_word_len: usize) -> Vec<usize> {
    // Keep this in sync with generator's heuristics: try a few densities.
    let total = size * size;
    let mut cands = Vec::new();
    for frac in [0.18f64, 0.22, 0.26, 0.30, 0.34] {
        let mut b = (total as f64 * frac).round() as isize;
        if b < 1 {
            b = 1;
        }
        if b as usize >= total {
            b = (total as isize) - 1;
        }
        let mut b = b as usize;
        if (size % 2) == 0 {
            b &= !1;
            if b == 0 {
                b = 2.min(total);
            }
        } else if min_word_len >= 3 {
            b &= !1;
            if b == 0 {
                b = 2.min(total);
            }
        }
        if b > 0 && b < total {
            cands.push(b);
        }
    }
    cands.sort();
    cands.dedup();
    cands
}

#[cfg(test)]
mod batch_tests {
    use super::*;

    #[test]
    fn leap_year_rules() {
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(2025));
        assert!(!is_leap_year(1900));
        assert!(is_leap_year(2000));
    }

    #[test]
    fn days_in_year_counts() {
        assert_eq!(iter_days_in_year(2025).count(), 365);
        assert_eq!(iter_days_in_year(2024).count(), 366);
    }
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
