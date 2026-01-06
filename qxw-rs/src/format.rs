use crate::model::{LProp, Puzzle, SProp, MXSZ};
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::Path;

fn trim_line_end(s: &str) -> &str {
    s.trim_end_matches(['\n', '\r'])
}

pub fn load_qxw(path: impl AsRef<Path>) -> Result<Puzzle> {
    let bytes = fs::read(&path).with_context(|| format!("reading {}", path.as_ref().display()))?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines = text.lines().map(trim_line_end);

    let first = lines.next().ok_or_else(|| anyhow::anyhow!("empty file"))?;
    let mut pz = Puzzle::new();

    if !first.starts_with("#QXW2") {
        // legacy format
        pz.gtype = 0;
        let parts: Vec<&str> = first.split_whitespace().collect();
        if parts.len() != 5 {
            bail!("legacy header parse error");
        }
        pz.width = parts[0].parse()?;
        pz.height = parts[1].parse()?;
        pz.symmr = parts[2].parse()?;
        pz.symmm = parts[3].parse()?;
        pz.symmd = parts[4].parse()?;
        // legacy symmr mapping
        pz.symmr = match pz.symmr {
            1 => 2,
            2 => 4,
            _ => 1,
        };
        pz.validate_basic()?;

        // read flags for height lines
        for j in 0..pz.height {
            let l = lines.next().ok_or_else(|| anyhow::anyhow!("unexpected EOF reading flags"))?;
            let mut it = l.split_whitespace();
            for i in 0..pz.width {
                let u: i32 = it
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("flag line too short"))?
                    .parse()?;
                if !(0..=31).contains(&u) {
                    bail!("invalid legacy flag value {u}");
                }
                if let Some(sq) = pz.square_mut(i, j) {
                    sq.fl = (u as u8) & 0x19;
                    sq.bars = ((u >> 1) & 3) as u32;
                    sq.merge = 0;
                }
            }
        }

        // grid chars: height lines, width chars each
        for j in 0..pz.height {
            let l = lines
                .next()
                .ok_or_else(|| anyhow::anyhow!("unexpected EOF reading grid"))?;
            let row = l.as_bytes();
            if row.len() < pz.width as usize {
                bail!("grid line too short");
            }
            for i in 0..pz.width {
                let c = row[i as usize];
                let ok = (b'A'..=b'Z').contains(&c) || (b'0'..=b'9').contains(&c) || c == b' ';
                if !ok {
                    bail!("invalid grid character {c:?}");
                }
                if let Some(sq) = pz.square_mut(i, j) {
                    sq.ch = c;
                }
            }
        }

        return Ok(pz);
    }

    // #QXW2 format
    let gp = lines.next().ok_or_else(|| anyhow::anyhow!("missing GP line"))?;
    let gp_parts: Vec<&str> = gp.split_whitespace().collect();
    if gp_parts.len() != 7 || gp_parts[0] != "GP" {
        bail!("bad GP line");
    }
    pz.gtype = gp_parts[1].parse()?;
    pz.width = gp_parts[2].parse()?;
    pz.height = gp_parts[3].parse()?;
    pz.symmr = gp_parts[4].parse()?;
    pz.symmm = gp_parts[5].parse()?;
    pz.symmd = gp_parts[6].parse()?;
    pz.validate_basic()?;

    // TTL
    expect_exact(lines.next(), "TTL")?;
    pz.title = read_plus_line(&mut lines, "title")?;

    // AUT
    expect_exact(lines.next(), "AUT")?;
    pz.author = read_plus_line(&mut lines, "author")?;

    // optional GLP, then zero+ GSP, then misc blocks
    let mut pending = lines.next().ok_or_else(|| anyhow::anyhow!("unexpected EOF"))?;

    if pending.starts_with("GLP ") {
        let parts: Vec<&str> = pending.split_whitespace().collect();
        if parts.len() != 5 {
            bail!("bad GLP line");
        }
        pz.dlp = LProp {
            dmask: parts[1].parse()?,
            emask: parts[2].parse()?,
            ten: parts[3].parse::<u8>()? != 0,
            lpor: false,
        };
        pending = lines.next().ok_or_else(|| anyhow::anyhow!("unexpected EOF"))?;
    }

    while pending.starts_with("GSP ") {
        let parts: Vec<&str> = pending.split_whitespace().collect();
        if parts.len() != 5 {
            bail!("bad GSP line");
        }
        pz.dsp = SProp {
            bgcol: u32::from_str_radix(parts[1], 16)?,
            fgcol: u32::from_str_radix(parts[2], 16)?,
            ten: parts[3].parse::<u8>()? != 0,
            spor: false,
        };
        pending = lines.next().ok_or_else(|| anyhow::anyhow!("unexpected EOF"))?;
    }

    // Parse TM/TMSG/DFN/DSF/DAF blocks (we still keep consuming until we reach SQ).
    loop {
        if pending.starts_with("TM ") {
            // TM j t0 t1 then +line
            let parts: Vec<&str> = pending.split_whitespace().collect();
            if parts.len() != 4 {
                bail!("bad TM line");
            }
            let j: i32 = parts[1].parse()?;
            let t0: i32 = parts[2].parse()?;
            let t1: i32 = parts[3].parse()?;
            let plus = lines.next().ok_or_else(|| anyhow::anyhow!("unexpected EOF after TM"))?;
            if j == 0 {
                pz.treatmode = t0;
                pz.tambaw = t1;
                if let Some(rest) = plus.strip_prefix('+') {
                    pz.tpifname = rest.to_string();
                }
            }
            pending = lines.next().ok_or_else(|| anyhow::anyhow!("unexpected EOF"))?;
            continue;
        }

        if pending.starts_with("TMSG ") {
            // TMSG j idx then +line
            let parts: Vec<&str> = pending.split_whitespace().collect();
            if parts.len() != 3 {
                bail!("bad TMSG line");
            }
            let j: i32 = parts[1].parse()?;
            let idx: usize = parts[2].parse()?;
            let plus = lines.next().ok_or_else(|| anyhow::anyhow!("unexpected EOF after TMSG"))?;
            if j == 0 {
                if idx < pz.treatmsg.len() {
                    if let Some(rest) = plus.strip_prefix('+') {
                        pz.treatmsg[idx] = rest.to_string();
                    }
                }
            }
            pending = lines.next().ok_or_else(|| anyhow::anyhow!("unexpected EOF"))?;
            continue;
        }

        if pending.starts_with("DFN ") {
            let parts: Vec<&str> = pending.split_whitespace().collect();
            if parts.len() != 2 {
                bail!("bad DFN line");
            }
            let idx: usize = parts[1].parse()?;
            let plus = lines.next().ok_or_else(|| anyhow::anyhow!("unexpected EOF after DFN"))?;
            if idx < pz.dfnames.len() {
                if let Some(rest) = plus.strip_prefix('+') {
                    pz.dfnames[idx] = rest.to_string();
                }
            }
            pending = lines.next().ok_or_else(|| anyhow::anyhow!("unexpected EOF"))?;
            continue;
        }

        if pending.starts_with("DSF ") {
            let parts: Vec<&str> = pending.split_whitespace().collect();
            if parts.len() != 2 {
                bail!("bad DSF line");
            }
            let idx: usize = parts[1].parse()?;
            let plus = lines.next().ok_or_else(|| anyhow::anyhow!("unexpected EOF after DSF"))?;
            if idx < pz.dsfilters.len() {
                if let Some(rest) = plus.strip_prefix('+') {
                    pz.dsfilters[idx] = rest.to_string();
                }
            }
            pending = lines.next().ok_or_else(|| anyhow::anyhow!("unexpected EOF"))?;
            continue;
        }

        if pending.starts_with("DAF ") {
            let parts: Vec<&str> = pending.split_whitespace().collect();
            if parts.len() != 2 {
                bail!("bad DAF line");
            }
            let idx: usize = parts[1].parse()?;
            let plus = lines.next().ok_or_else(|| anyhow::anyhow!("unexpected EOF after DAF"))?;
            if idx < pz.dafilters.len() {
                if let Some(rest) = plus.strip_prefix('+') {
                    pz.dafilters[idx] = rest.to_string();
                }
            }
            pending = lines.next().ok_or_else(|| anyhow::anyhow!("unexpected EOF"))?;
            continue;
        }

        break;
    }

        // SQ records
        let nd_u32 = pz.ndir() as u32;
    while pending.starts_with("SQ ") {
        let parts: Vec<&str> = pending.split_whitespace().collect();
        if parts.len() < 6 {
            bail!("bad SQ line");
        }
        let i: i32 = parts[1].parse()?;
        let j: i32 = parts[2].parse()?;
        let b: u32 = parts[3].parse()?;
        let m: u32 = parts[4].parse()?;
        let f: u8 = parts[5].parse()?;
        let c = parts.get(6).and_then(|s| s.as_bytes().first().copied()).unwrap_or(b' ');

        if (0..MXSZ as i32).contains(&i) && (0..MXSZ as i32).contains(&j) {
            if let Some(sq) = pz.square_mut(i, j) {
                sq.bars = b & ((1u32 << nd_u32) - 1);
                sq.merge = m & ((1u32 << nd_u32) - 1);
                sq.fl = f & 0x19;
                sq.ch = if (b'A'..=b'Z').contains(&c) || (b'0'..=b'9').contains(&c) {
                    c
                } else {
                    b' '
                };
            }
        }
        pending = lines.next().ok_or_else(|| anyhow::anyhow!("unexpected EOF"))?;
    }

    // SQSP records
    while pending.starts_with("SQSP ") {
        let parts: Vec<&str> = pending.split_whitespace().collect();
        if parts.len() != 7 {
            bail!("bad SQSP line");
        }
        let i: i32 = parts[1].parse()?;
        let j: i32 = parts[2].parse()?;
        if (0..MXSZ as i32).contains(&i) && (0..MXSZ as i32).contains(&j) {
            if let Some(sq) = pz.square_mut(i, j) {
                sq.sp = SProp {
                    bgcol: u32::from_str_radix(parts[3], 16)? & 0xFF_FF_FF,
                    fgcol: u32::from_str_radix(parts[4], 16)? & 0xFF_FF_FF,
                    ten: parts[5].parse::<u8>()? != 0,
                    spor: parts[6].parse::<u8>()? != 0,
                };
            }
        }
        pending = lines.next().ok_or_else(|| anyhow::anyhow!("unexpected EOF"))?;
    }

    // SQLP records
    while pending.starts_with("SQLP ") {
        let parts: Vec<&str> = pending.split_whitespace().collect();
        if parts.len() != 8 {
            bail!("bad SQLP line");
        }
        let i: i32 = parts[1].parse()?;
        let j: i32 = parts[2].parse()?;
        let d: usize = parts[3].parse()?;
        if (0..MXSZ as i32).contains(&i) && (0..MXSZ as i32).contains(&j) {
            if let Some(sq) = pz.square_mut(i, j) {
                if d < sq.lp.len() {
                    sq.lp[d] = LProp {
                        dmask: parts[4].parse()?,
                        emask: parts[5].parse()?,
                        ten: parts[6].parse::<u8>()? != 0,
                        lpor: parts[7].parse::<u8>()? != 0,
                    };
                }
            }
        }
        pending = lines.next().ok_or_else(|| anyhow::anyhow!("unexpected EOF"))?;
    }

    // END (or extra content)
    // We ignore the rest.

    Ok(pz)
}

pub fn save_qxw2(pz: &Puzzle, path: impl AsRef<Path>) -> Result<()> {
    use std::fmt::Write as _;

    let mut out = String::new();
    writeln!(&mut out, "#QXW2 http://www.quinapalus.com")?;
    writeln!(
        &mut out,
        "GP {} {} {} {} {} {}",
        pz.gtype, pz.width, pz.height, pz.symmr, pz.symmm, pz.symmd
    )?;
    writeln!(&mut out, "TTL")?;
    writeln!(&mut out, "+{}", pz.title)?;
    writeln!(&mut out, "AUT")?;
    writeln!(&mut out, "+{}", pz.author)?;
    writeln!(
        &mut out,
        "GLP {} {} {} {}",
        pz.dlp.dmask,
        pz.dlp.emask,
        pz.dlp.ten as u8,
        pz.dlp.lpor as u8
    )?;
    writeln!(
        &mut out,
        "GSP {:06x} {:06x} {} {}",
        pz.dsp.bgcol & 0xFF_FF_FF,
        pz.dsp.fgcol & 0xFF_FF_FF,
        pz.dsp.ten as u8,
        pz.dsp.spor as u8
    )?;

    writeln!(
        &mut out,
        "TM 0 {} {}",
        pz.treatmode,
        pz.tambaw
    )?;
    writeln!(&mut out, "+{}", pz.tpifname)?;
    for (i, msg) in pz.treatmsg.iter().enumerate() {
        writeln!(&mut out, "TMSG 0 {}", i)?;
        writeln!(&mut out, "+{}", msg)?;
    }
    for (i, v) in pz.dfnames.iter().enumerate() {
        writeln!(&mut out, "DFN {}", i)?;
        writeln!(&mut out, "+{}", v)?;
    }
    for (i, v) in pz.dsfilters.iter().enumerate() {
        writeln!(&mut out, "DSF {}", i)?;
        writeln!(&mut out, "+{}", v)?;
    }
    for (i, v) in pz.dafilters.iter().enumerate() {
        writeln!(&mut out, "DAF {}", i)?;
        writeln!(&mut out, "+{}", v)?;
    }

    // Squares are saved across the full active width/height.
    for j in 0..pz.height {
        for i in 0..pz.width {
            let sq = pz.square(i, j).ok_or_else(|| anyhow::anyhow!("square out of range"))?;
            // Match C's formatting: the final %c is emitted even if it is space.
            writeln!(
                &mut out,
                "SQ {} {} {} {} {} {}",
                i,
                j,
                sq.bars,
                sq.merge,
                sq.fl,
                sq.ch as char
            )?;
        }
    }
    for j in 0..pz.height {
        for i in 0..pz.width {
            let sq = pz.square(i, j).ok_or_else(|| anyhow::anyhow!("square out of range"))?;
            writeln!(
                &mut out,
                "SQSP {} {} {:06x} {:06x} {} {}",
                i,
                j,
                sq.sp.bgcol & 0xFF_FF_FF,
                sq.sp.fgcol & 0xFF_FF_FF,
                sq.sp.ten as u8,
                sq.sp.spor as u8
            )?;
        }
    }
    for j in 0..pz.height {
        for i in 0..pz.width {
            let sq = pz.square(i, j).ok_or_else(|| anyhow::anyhow!("square out of range"))?;
            for d in 0..pz.ndir() {
                let lp = sq.lp[d];
                writeln!(
                    &mut out,
                    "SQLP {} {} {} {} {} {} {}",
                    i,
                    j,
                    d,
                    lp.dmask,
                    lp.emask,
                    lp.ten as u8,
                    lp.lpor as u8
                )?;
            }
        }
    }
    writeln!(&mut out, "END")?;

    fs::write(&path, out).with_context(|| format!("writing {}", path.as_ref().display()))?;
    Ok(())
}

fn expect_exact(line: Option<&str>, expected: &str) -> Result<()> {
    let got = line.ok_or_else(|| anyhow::anyhow!("unexpected EOF"))?;
    if got != expected {
        bail!("expected {expected}, got {got}");
    }
    Ok(())
}

fn read_plus_line<'a>(lines: &mut impl Iterator<Item = &'a str>, ctx: &str) -> Result<String> {
    let l = lines
        .next()
        .ok_or_else(|| anyhow::anyhow!("unexpected EOF reading {ctx}"))?;
    let Some(rest) = l.strip_prefix('+') else {
        bail!("expected +line for {ctx}");
    };
    Ok(rest.to_string())
}
