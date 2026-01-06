use crate::model::Puzzle;
use anyhow::{bail, Context, Result};
use rand::seq::SliceRandom;
use rand::Rng;
use std::collections::HashSet;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

fn normalize_word(s: &str) -> Option<String> {
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

pub fn load_wordlist_len(path: impl AsRef<Path>, len: usize) -> Result<Vec<String>> {
    let bytes = fs::read(&path).with_context(|| format!("reading wordlist {}", path.as_ref().display()))?;
    let text = String::from_utf8_lossy(&bytes);

    let mut out = Vec::new();
    for line in text.lines() {
        let Some(w) = normalize_word(line) else { continue };
        if w.len() == len {
            out.push(w);
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

pub fn load_wordlist_lens(path: impl AsRef<Path>, lens: &[usize]) -> Result<HashMap<usize, Vec<String>>> {
    let bytes = fs::read(&path).with_context(|| format!("reading wordlist {}", path.as_ref().display()))?;
    let text = String::from_utf8_lossy(&bytes);

    let want: HashSet<usize> = lens.iter().copied().collect();
    let mut tmp: HashMap<usize, Vec<String>> = HashMap::new();
    for line in text.lines() {
        let Some(w) = normalize_word(line) else { continue };
        if want.contains(&w.len()) {
            tmp.entry(w.len()).or_default().push(w);
        }
    }

    for v in tmp.values_mut() {
        v.sort();
        v.dedup();
    }
    Ok(tmp)
}

fn build_prefix_index(words: &[String]) -> HashMap<String, Vec<usize>> {
    let mut idx: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, w) in words.iter().enumerate() {
        for p in 0..=w.len() {
            idx.entry(w[..p].to_string()).or_default().push(i);
        }
    }
    idx
}

fn generate_word_square_limited(
    words: &[String],
    n: usize,
    rng: &mut impl Rng,
    deadline: Option<Instant>,
) -> Result<Vec<String>> {
    if words.is_empty() {
        bail!("empty word list");
    }
    if n == 0 {
        bail!("size must be > 0");
    }
    if words.iter().any(|w| w.len() != n) {
        bail!("word list must contain only {}-letter words", n);
    }

    let pref = build_prefix_index(words);

    let mut rows: Vec<usize> = Vec::with_capacity(n);

    // Randomize starting words to avoid always generating the same square.
    let mut starters: Vec<usize> = (0..words.len()).collect();
    starters.shuffle(rng);

    fn col_prefix(words: &[String], rows: &[usize], col: usize) -> String {
        let mut s = String::with_capacity(rows.len());
        for &r in rows {
            s.push(words[r].as_bytes()[col] as char);
        }
        s
    }

    fn backtrack(
        words: &[String],
        pref: &HashMap<String, Vec<usize>>,
        rows: &mut Vec<usize>,
        n: usize,
        rng: &mut impl Rng,
        deadline: Option<Instant>,
    ) -> bool {
        if let Some(d) = deadline {
            if Instant::now() >= d {
                return false;
            }
        }
        let k = rows.len();
        if k == n {
            return true;
        }

        // required prefix for row k is the prefix of column k
        let need = col_prefix(words, rows, k);
        let Some(cands) = pref.get(&need) else {
            return false;
        };

        // shuffle candidates for variation
        let mut order: Vec<usize> = cands.clone();
        order.shuffle(rng);

        'cand: for &wi in &order {
            // must satisfy symmetry constraints with existing rows
            // (redundant given prefix indexing, but keep it explicit)
            let w = &words[wi];
            for i in 0..k {
                if w.as_bytes()[i] != words[rows[i]].as_bytes()[k] {
                    continue 'cand;
                }
            }

            // Forward-check: every column prefix after adding this row must exist.
            rows.push(wi);
            for col in 0..n {
                let p = col_prefix(words, rows, col);
                if !pref.contains_key(&p) {
                    rows.pop();
                    continue 'cand;
                }
            }

            if backtrack(words, pref, rows, n, rng, deadline) {
                return true;
            }
            rows.pop();
        }
        false
    }

    for start in starters {
        if let Some(d) = deadline {
            if Instant::now() >= d {
                break;
            }
        }
        rows.clear();
        rows.push(start);
        if backtrack(words, &pref, &mut rows, n, rng, deadline) {
            let mut sq = Vec::with_capacity(n);
            for &ri in &rows {
                sq.push(words[ri].clone());
            }
            return Ok(sq);
        }
    }

    bail!("no {n}x{n} word square found in this wordlist")
}

pub fn generate_word_square(words: &[String], n: usize, rng: &mut impl Rng) -> Result<Vec<String>> {
    generate_word_square_limited(words, n, rng, None)
}

pub fn generate_word_square_5(words: &[String], rng: &mut impl Rng) -> Result<[String; 5]> {
    let sq = generate_word_square(words, 5, rng)?;
    Ok([
        sq[0].clone(),
        sq[1].clone(),
        sq[2].clone(),
        sq[3].clone(),
        sq[4].clone(),
    ])
}

pub fn generate_mini_puzzle_from_wordlist_size(
    words: &[String],
    size: usize,
    title: Option<String>,
    author: Option<String>,
    rng: &mut impl Rng,
) -> Result<Puzzle> {
    let square = generate_word_square(words, size, rng)?;

    let mut pz = Puzzle::new();
    pz.gtype = 0;
    pz.width = size as i32;
    pz.height = size as i32;
    // rotational symmetry (not super meaningful for an open grid, but matches Qxw defaults)
    pz.symmr = 2;
    pz.symmm = 0;
    pz.symmd = 0;
    pz.title = title.unwrap_or_else(|| "Mini".to_string());
    pz.author = author.unwrap_or_else(|| "".to_string());

    for y in 0..size {
        for x in 0..size {
            let sq = pz.square_mut(x as i32, y as i32).unwrap();
            sq.fl = 0;
            sq.bars = 0;
            sq.merge = 0;
            sq.ch = square[y].as_bytes()[x];
        }
    }

    pz.compute_numbers();
    Ok(pz)
}

pub fn generate_mini_puzzle_from_wordlist_size_limited(
    words: &[String],
    size: usize,
    title: Option<String>,
    author: Option<String>,
    rng: &mut impl Rng,
    time_limit: Option<Duration>,
) -> Result<Puzzle> {
    let deadline = time_limit.map(|d| Instant::now() + d);
    let square = generate_word_square_limited(words, size, rng, deadline)?;

    let mut pz = Puzzle::new();
    pz.gtype = 0;
    pz.width = size as i32;
    pz.height = size as i32;
    pz.symmr = 2;
    pz.symmm = 0;
    pz.symmd = 0;
    pz.title = title.unwrap_or_else(|| "Mini".to_string());
    pz.author = author.unwrap_or_else(|| "".to_string());

    for y in 0..size {
        for x in 0..size {
            let sq = pz.square_mut(x as i32, y as i32).unwrap();
            sq.fl = 0;
            sq.bars = 0;
            sq.merge = 0;
            sq.ch = square[y].as_bytes()[x];
        }
    }

    pz.compute_numbers();
    Ok(pz)
}

pub fn generate_mini_puzzle_from_wordlist(
    words5: &[String],
    title: Option<String>,
    author: Option<String>,
    rng: &mut impl Rng,
) -> Result<Puzzle> {
    generate_mini_puzzle_from_wordlist_size(words5, 5, title, author, rng)
}

#[derive(Debug, Clone)]
struct Light {
    cells: Vec<(i32, i32)>,
}

fn collect_lights(pz: &Puzzle) -> Vec<Light> {
    let mut out = Vec::new();
    for d in 0..pz.ndir() {
        for y in 0..pz.height {
            for x in 0..pz.width {
                if !pz.is_start_of_light(x, y, d) {
                    continue;
                }
                if let Some(cells) = pz.get_light(x, y, d) {
                    let _ = (d, x, y); // keep variables named for readability
                    out.push(Light { cells });
                }
            }
        }
    }
    out
}

fn word_pattern(pz: &Puzzle, cells: &[(i32, i32)]) -> String {
    let mut s = String::with_capacity(cells.len());
    for &(x, y) in cells {
        let ch = pz.square(x, y).map(|q| q.ch).unwrap_or(b' ');
        if ch == b' ' {
            s.push('.');
        } else {
            s.push(ch as char);
        }
    }
    s
}

fn matches_pattern(word: &str, pattern: &str) -> bool {
    if word.len() != pattern.len() {
        return false;
    }
    for (wc, pc) in word.chars().zip(pattern.chars()) {
        if pc != '.' && pc != wc {
            return false;
        }
    }
    true
}

fn choose_next_light(pz: &Puzzle, lights: &[Light], dict: &HashMap<usize, Vec<String>>, used: &HashSet<String>) -> Option<usize> {
    let mut best: Option<(usize, usize)> = None; // (index, candidate_count)
    for (i, l) in lights.iter().enumerate() {
        let pat = word_pattern(pz, &l.cells);
        if !pat.contains('.') {
            continue;
        }
        let Some(words) = dict.get(&l.cells.len()) else {
            return Some(i);
        };
        let mut count = 0usize;
        for w in words {
            if used.contains(w) {
                continue;
            }
            if matches_pattern(w, &pat) {
                count += 1;
            }
        }
        match best {
            None => best = Some((i, count)),
            Some((_bi, bc)) if count < bc => best = Some((i, count)),
            _ => {}
        }
    }
    best.map(|(i, _)| i)
}

fn apply_word(pz: &mut Puzzle, cells: &[(i32, i32)], word: &str) -> Result<Vec<(i32, i32)>> {
    let mut changed = Vec::new();
    for ((x, y), ch) in cells.iter().copied().zip(word.as_bytes().iter().copied()) {
        let sq = pz.square_mut(x, y).unwrap();
        if (sq.fl & 0x01) != 0 {
            bail!("attempted to write into blocked cell ({},{})", x, y);
        }
        if sq.ch == b' ' {
            sq.ch = ch;
            changed.push((x, y));
        } else if sq.ch != ch {
            bail!("letter conflict at ({},{})", x, y);
        }
    }
    Ok(changed)
}

fn undo_word(pz: &mut Puzzle, changed: &[(i32, i32)]) {
    for &(x, y) in changed {
        let sq = pz.square_mut(x, y).unwrap();
        sq.ch = b' ';
    }
}

fn fill_blocked_mini_limited(
    pz: &mut Puzzle,
    dict: &HashMap<usize, Vec<String>>,
    rng: &mut impl Rng,
    deadline: Option<Instant>,
) -> bool {
    let lights = collect_lights(pz);
    let mut used: HashSet<String> = HashSet::new();

    fn backtrack(
        pz: &mut Puzzle,
        lights: &[Light],
        dict: &HashMap<usize, Vec<String>>,
        used: &mut HashSet<String>,
        rng: &mut impl Rng,
        deadline: Option<Instant>,
    ) -> bool {
        if let Some(d) = deadline {
            if Instant::now() >= d {
                return false;
            }
        }
        let Some(li) = choose_next_light(pz, lights, dict, used) else {
            return true;
        };
        let l = &lights[li];
        let pat = word_pattern(pz, &l.cells);
        let Some(words) = dict.get(&l.cells.len()) else {
            return false;
        };

        let mut cands: Vec<&String> = words
            .iter()
            .filter(|w| !used.contains(*w) && matches_pattern(w, &pat))
            .collect();
        cands.shuffle(rng);
        if cands.is_empty() {
            return false;
        }

        for w in cands {
            let changed = match apply_word(pz, &l.cells, w) {
                Ok(c) => c,
                Err(_) => continue,
            };
            used.insert(w.clone());
            if backtrack(pz, lights, dict, used, rng, deadline) {
                return true;
            }
            used.remove(w);
            undo_word(pz, &changed);
        }
        false
    }

    backtrack(pz, &lights, dict, &mut used, rng, deadline)
}

fn fill_blocked_mini(pz: &mut Puzzle, dict: &HashMap<usize, Vec<String>>, rng: &mut impl Rng) -> bool {
    fill_blocked_mini_limited(pz, dict, rng, None)
}

fn validate_block_mask(size: usize, mask: &[bool], min_word_len: usize) -> bool {
    let w = size;
    let h = size;
    if mask.len() != w * h {
        return false;
    }

    // symmetry (180-degree rotational)
    for y in 0..h {
        for x in 0..w {
            let a = x + y * w;
            let b = (w - 1 - x) + (h - 1 - y) * w;
            if mask[a] != mask[b] {
                return false;
            }
        }
    }

    // min word length constraints across
    for y in 0..h {
        let mut run = 0usize;
        for x in 0..w {
            let blocked = mask[x + y * w];
            if blocked {
                if run > 0 && run < min_word_len {
                    return false;
                }
                run = 0;
            } else {
                run += 1;
            }
        }
        if run > 0 && run < min_word_len {
            return false;
        }
    }

    // min word length constraints down
    for x in 0..w {
        let mut run = 0usize;
        for y in 0..h {
            let blocked = mask[x + y * w];
            if blocked {
                if run > 0 && run < min_word_len {
                    return false;
                }
                run = 0;
            } else {
                run += 1;
            }
        }
        if run > 0 && run < min_word_len {
            return false;
        }
    }

    // connectivity of open cells (4-neighbor)
    let mut start: Option<usize> = None;
    for i in 0..(w * h) {
        if !mask[i] {
            start = Some(i);
            break;
        }
    }
    let Some(s) = start else {
        return false;
    };
    let mut stack = vec![s];
    let mut seen = vec![false; w * h];
    seen[s] = true;
    while let Some(i) = stack.pop() {
        let x = i % w;
        let y = i / w;
        let neigh = [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ];
        for (nx, ny) in neigh {
            if nx >= w || ny >= h {
                continue;
            }
            let ni = nx + ny * w;
            if mask[ni] {
                continue;
            }
            if !seen[ni] {
                seen[ni] = true;
                stack.push(ni);
            }
        }
    }
    for i in 0..(w * h) {
        if !mask[i] && !seen[i] {
            return false;
        }
    }

    true
}

#[cfg(test)]
fn validate_block_mask_5(mask: &[bool; 25], min_word_len: usize) -> bool {
    validate_block_mask(5, mask, min_word_len)
}

fn generate_rotational_block_mask(
    size: usize,
    blocks: usize,
    min_word_len: usize,
    rng: &mut impl Rng,
) -> Result<Vec<bool>> {
    let total = size * size;
    if blocks > total {
        bail!("blocks must be <= {}", total);
    }
    if min_word_len == 0 || min_word_len > size {
        bail!("min_word_len must be 1..={}", size);
    }

    // Under 180° symmetry, an odd number of blocks forces the center to be blocked (only exists when size is odd).
    // For typical crossword constraints (min_word_len >= 3), a forced center block often creates short entries.
    // We don't hard-reject generically here; instead we rely on validate_block_mask to filter.
    if (blocks % 2) == 1 && (size % 2) == 0 {
        bail!("on an even-sized grid with 180° rotational symmetry, blocks must be even");
    }

    // Choose from unique rotational orbits.
    let mut orbits: Vec<(usize, usize)> = Vec::new();
    for y in 0..size {
        for x in 0..size {
            let a = x + y * size;
            let b = (size - 1 - x) + (size - 1 - y) * size;
            if a <= b {
                orbits.push((a, b));
            }
        }
    }

    let want = blocks;
    let mut attempts = 0usize;
    while attempts < 5000 {
        attempts += 1;
        let mut mask = vec![false; total];
        let mut remaining = want;

        // Randomize orbit order.
        let mut order = orbits.clone();
        order.shuffle(rng);

        // Greedily decide to include or skip each orbit.
        for (a, b) in order {
            if remaining == 0 {
                break;
            }
            let cost = if a == b { 1 } else { 2 };
            if cost > remaining {
                continue;
            }
            // biased coin-flip to include blocks, but ensure we can still reach remaining.
            let take = rng.gen_bool(0.5);
            if take {
                mask[a] = true;
                mask[b] = true;
                remaining -= cost;
            }
        }

        // If we didn't hit the target, try to top up deterministically.
        if remaining != 0 {
            let mut order2 = orbits.clone();
            order2.shuffle(rng);
            for (a, b) in order2 {
                if remaining == 0 {
                    break;
                }
                let cost = if a == b { 1 } else { 2 };
                if cost > remaining {
                    continue;
                }
                if mask[a] && mask[b] {
                    continue;
                }
                mask[a] = true;
                mask[b] = true;
                remaining -= cost;
            }
        }

        if remaining != 0 {
            continue;
        }

        if validate_block_mask(size, &mask, min_word_len) {
            return Ok(mask);
        }
    }

    bail!("failed to generate a valid {size}x{size} symmetric block layout with {blocks} blocks")
}

pub fn generate_mini_puzzle_blocked_from_wordlist_size(
    size: usize,
    dict: &HashMap<usize, Vec<String>>,
    blocks: usize,
    min_word_len: usize,
    title: Option<String>,
    author: Option<String>,
    rng: &mut impl Rng,
) -> Result<Puzzle> {
    if blocks == 0 {
        bail!("blocks must be > 0 for blocked mini generation");
    }
    if size == 0 {
        bail!("size must be > 0");
    }

    let mut pz = Puzzle::new();
    pz.gtype = 0;
    pz.width = size as i32;
    pz.height = size as i32;
    pz.symmr = 2;
    pz.symmm = 0;
    pz.symmd = 0;
    pz.title = title.unwrap_or_else(|| "Mini".to_string());
    pz.author = author.unwrap_or_else(|| "".to_string());

    // Try multiple layouts until we find one that fills.
    for _attempt in 0..200 {
        let mask = generate_rotational_block_mask(size, blocks, min_word_len, rng)?;

        for y in 0..size {
            for x in 0..size {
                let i = x + y * size;
                let sq = pz.square_mut(x as i32, y as i32).unwrap();
                sq.bars = 0;
                sq.merge = 0;
                sq.number = -1;
                sq.ch = b' ';
                sq.fl = if mask[i] { 0x01 } else { 0x00 };
            }
        }

        if fill_blocked_mini(&mut pz, dict, rng) {
            pz.compute_numbers();
            return Ok(pz);
        }
    }

    bail!("failed to fill a blocked {size}x{size} mini after many attempts (try a different dictionary or different block count)")
}

pub fn generate_mini_puzzle_blocked_from_wordlist_size_limited(
    size: usize,
    dict: &HashMap<usize, Vec<String>>,
    blocks: usize,
    min_word_len: usize,
    title: Option<String>,
    author: Option<String>,
    rng: &mut impl Rng,
    time_limit: Option<Duration>,
) -> Result<Puzzle> {
    if blocks == 0 {
        bail!("blocks must be > 0 for blocked mini generation");
    }
    if size == 0 {
        bail!("size must be > 0");
    }

    let deadline = time_limit.map(|d| Instant::now() + d);

    let mut pz = Puzzle::new();
    pz.gtype = 0;
    pz.width = size as i32;
    pz.height = size as i32;
    pz.symmr = 2;
    pz.symmm = 0;
    pz.symmd = 0;
    pz.title = title.unwrap_or_else(|| "Mini".to_string());
    pz.author = author.unwrap_or_else(|| "".to_string());

    for _attempt in 0..200 {
        if let Some(d) = deadline {
            if Instant::now() >= d {
                break;
            }
        }
        let mask = generate_rotational_block_mask(size, blocks, min_word_len, rng)?;

        for y in 0..size {
            for x in 0..size {
                let i = x + y * size;
                let sq = pz.square_mut(x as i32, y as i32).unwrap();
                sq.bars = 0;
                sq.merge = 0;
                sq.number = -1;
                sq.ch = b' ';
                sq.fl = if mask[i] { 0x01 } else { 0x00 };
            }
        }

        if fill_blocked_mini_limited(&mut pz, dict, rng, deadline) {
            pz.compute_numbers();
            return Ok(pz);
        }
    }

    if time_limit.is_some() {
        bail!("timed out trying to fill a blocked {size}x{size} mini (try increasing --time-limit-ms, changing --blocks, or using a better dictionary)")
    } else {
        bail!("failed to fill a blocked {size}x{size} mini after many attempts (try a different dictionary or different block count)")
    }
}

pub fn generate_mini_puzzle_blocked_from_wordlist(
    dict: &HashMap<usize, Vec<String>>,
    blocks: usize,
    min_word_len: usize,
    title: Option<String>,
    author: Option<String>,
    rng: &mut impl Rng,
) -> Result<Puzzle> {
    generate_mini_puzzle_blocked_from_wordlist_size(5, dict, blocks, min_word_len, title, author, rng)
}

fn auto_block_candidates(size: usize, min_word_len: usize) -> Vec<usize> {
    let total = size * size;
    let mut cands = Vec::new();
    // heuristic densities
    for frac in [0.20f64, 0.25, 0.30, 0.35] {
        let mut b = (total as f64 * frac).round() as isize;
        if b < 1 {
            b = 1;
        }
        if b as usize >= total {
            b = (total as isize) - 1;
        }
        let mut b = b as usize;
        // symmetry constraints: even size requires even blocks; for typical min_word_len>=3, prefer even to avoid forced center block.
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

pub fn generate_mini_puzzle_blocked_auto_from_wordlist_size(
    size: usize,
    dict: &HashMap<usize, Vec<String>>,
    min_word_len: usize,
    title: Option<String>,
    author: Option<String>,
    rng: &mut impl Rng,
) -> Result<Puzzle> {
    let mut candidates = auto_block_candidates(size, min_word_len);
    candidates.shuffle(rng);
    let mut last_err: Option<anyhow::Error> = None;
    for blocks in candidates {
        match generate_mini_puzzle_blocked_from_wordlist_size(
            size,
            dict,
            blocks,
            min_word_len,
            title.clone(),
            author.clone(),
            rng,
        ) {
            Ok(pz) => return Ok(pz),
            Err(e) => last_err = Some(e),
        }
    }
    if let Some(e) = last_err {
        Err(e)
    } else {
        bail!("failed to generate a blocked {size}x{size} mini")
    }
}

pub fn generate_mini_puzzle_blocked_auto_from_wordlist_size_limited(
    size: usize,
    dict: &HashMap<usize, Vec<String>>,
    min_word_len: usize,
    title: Option<String>,
    author: Option<String>,
    rng: &mut impl Rng,
    time_limit: Option<Duration>,
) -> Result<Puzzle> {
    let mut candidates = auto_block_candidates(size, min_word_len);
    candidates.shuffle(rng);
    let mut last_err: Option<anyhow::Error> = None;
    for blocks in candidates {
        match generate_mini_puzzle_blocked_from_wordlist_size_limited(
            size,
            dict,
            blocks,
            min_word_len,
            title.clone(),
            author.clone(),
            rng,
            time_limit,
        ) {
            Ok(pz) => return Ok(pz),
            Err(e) => last_err = Some(e),
        }
    }
    if let Some(e) = last_err {
        Err(e)
    } else {
        bail!("failed to generate a blocked {size}x{size} mini")
    }
}

pub fn generate_mini_puzzle_blocked_auto_from_wordlist(
    dict: &HashMap<usize, Vec<String>>,
    min_word_len: usize,
    title: Option<String>,
    author: Option<String>,
    rng: &mut impl Rng,
) -> Result<Puzzle> {
    generate_mini_puzzle_blocked_auto_from_wordlist_size(5, dict, min_word_len, title, author, rng)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    #[test]
    fn generates_from_known_square_wordlist() {
        // Classic 5x5 word square.
        let words = vec![
            "SATOR".to_string(),
            "AREPO".to_string(),
            "TENET".to_string(),
            "OPERA".to_string(),
            "ROTAS".to_string(),
        ];
        let mut rng = StdRng::seed_from_u64(1);
        let sq = generate_word_square(&words, 5, &mut rng).unwrap();
        assert_eq!(sq[0], "SATOR");
        assert_eq!(sq[4], "ROTAS");
    }

    #[test]
    fn validates_block_mask_symmetry_and_minlen() {
        // Simple symmetric mask with just the center blocked.
        let mut mask = [false; 25];
        mask[12] = true;
        // This creates 2-letter entries in the center row/col, so min_word_len=3 should reject.
        assert!(!validate_block_mask_5(&mask, 3));
        assert!(validate_block_mask_5(&mask, 2));

        // Break symmetry.
        let mut mask2 = mask;
        mask2[0] = true;
        assert!(!validate_block_mask_5(&mask2, 3));

        // Create a 2-letter across entry (invalid for min_len=3).
        // Row 0: open open block open open => runs of 2.
        let mut mask3 = [false; 25];
        mask3[2] = true;
        mask3[22] = true; // keep symmetry
        assert!(!validate_block_mask_5(&mask3, 3));
    }
}
