use anyhow::{bail, Result};

pub const NGTYPE: usize = 5;
pub const MXSZ: usize = 63;
pub const MAXNDIR: usize = 3;

pub const NDIRECTIONS: [usize; NGTYPE] = [2, 3, 3, 2, 2];

pub const DNAME: [[&str; MAXNDIR]; NGTYPE] = [
    ["Across", "Down", ""],
    ["Northeast", "Southeast", "South"],
    ["East", "Southeast", "Southwest"],
    ["Ring", "Radial", ""],
    ["Ring", "Radial", ""],
];

pub const MAXNDICTS: usize = 9;
pub const NMSG: usize = 2;

#[derive(Debug, Clone, Copy, Default)]
pub struct SProp {
    pub bgcol: u32,
    pub fgcol: u32,
    pub ten: bool,
    pub spor: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LProp {
    pub dmask: u32,
    pub emask: u32,
    pub ten: bool,
    pub lpor: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct Square {
    pub bars: u32,
    pub merge: u32,
    pub fl: u8,
    pub dsel: u8,
    pub ch: u8,
    pub sp: SProp,
    pub lp: [LProp; MAXNDIR],
    pub number: i32,
}

impl Default for Square {
    fn default() -> Self {
        Self {
            bars: 0,
            merge: 0,
            fl: 0,
            dsel: 0,
            ch: b' ',
            sp: SProp {
                bgcol: 0xFF_FF_FF,
                fgcol: 0x00_00_00,
                ten: false,
                spor: false,
            },
            lp: [
                LProp {
                    dmask: 1,
                    emask: 1,
                    ten: false,
                    lpor: false,
                },
                LProp {
                    dmask: 1,
                    emask: 1,
                    ten: false,
                    lpor: false,
                },
                LProp {
                    dmask: 1,
                    emask: 1,
                    ten: false,
                    lpor: false,
                },
            ],
            number: -1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Puzzle {
    pub gtype: usize,
    pub width: i32,
    pub height: i32,
    pub symmr: i32,
    pub symmm: i32,
    pub symmd: i32,
    pub title: String,
    pub author: String,
    pub dsp: SProp,
    pub dlp: LProp,

    // QXW2 persisted settings (used by the original app's filler/treatment features)
    pub treatmode: i32,
    pub tambaw: i32,
    pub tpifname: String,
    pub treatmsg: [String; NMSG],
    pub dfnames: [String; MAXNDICTS],
    pub dsfilters: [String; MAXNDICTS],
    pub dafilters: [String; MAXNDICTS],

    squares: Vec<Square>,
}

impl Puzzle {
    pub fn new() -> Self {
        Self {
            gtype: 0,
            width: 12,
            height: 12,
            symmr: 2,
            symmm: 0,
            symmd: 0,
            title: String::new(),
            author: String::new(),
            dsp: SProp {
                bgcol: 0xFF_FF_FF,
                fgcol: 0x00_00_00,
                ten: false,
                spor: false,
            },
            dlp: LProp {
                dmask: 1,
                emask: 1,
                ten: false,
                lpor: false,
            },

            treatmode: 0,
            tambaw: 0,
            tpifname: String::new(),
            treatmsg: [String::new(), String::new()],
            dfnames: core::array::from_fn(|_| String::new()),
            dsfilters: core::array::from_fn(|_| String::new()),
            dafilters: core::array::from_fn(|_| String::new()),

            squares: vec![Square::default(); MXSZ * MXSZ],
        }
    }

    #[inline]
    fn idx(x: i32, y: i32) -> usize {
        (x as usize) + (y as usize) * MXSZ
    }

    pub fn square(&self, x: i32, y: i32) -> Option<&Square> {
        if x < 0 || y < 0 || x >= MXSZ as i32 || y >= MXSZ as i32 {
            return None;
        }
        Some(&self.squares[Self::idx(x, y)])
    }

    pub fn square_mut(&mut self, x: i32, y: i32) -> Option<&mut Square> {
        if x < 0 || y < 0 || x >= MXSZ as i32 || y >= MXSZ as i32 {
            return None;
        }
        let idx = Self::idx(x, y);
        Some(&mut self.squares[idx])
    }

    pub fn ndir(&self) -> usize {
        NDIRECTIONS[self.gtype]
    }

    pub fn validate_basic(&self) -> Result<()> {
        if self.gtype >= NGTYPE {
            bail!("invalid gtype {}", self.gtype);
        }
        if !(1..=MXSZ as i32).contains(&self.width) {
            bail!("invalid width {}", self.width);
        }
        if !(1..=MXSZ as i32).contains(&self.height) {
            bail!("invalid height {}", self.height);
        }
        Ok(())
    }

    pub fn is_ingrid(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return false;
        }
        match self.gtype {
            1 => {
                // hexH
                if (self.width & 1) == 1 && (x & 1) == 1 && y == self.height - 1 {
                    return false;
                }
            }
            2 => {
                // hexV
                if (self.height & 1) == 1 && (y & 1) == 1 && x == self.width - 1 {
                    return false;
                }
            }
            _ => {}
        }
        true
    }

    pub fn is_clear(&self, x: i32, y: i32) -> bool {
        if !self.is_ingrid(x, y) {
            return false;
        }
        let Some(sq) = self.square(x, y) else { return false };
        (sq.fl & 0x09) == 0
    }

    pub fn step_back(&self, x: &mut i32, y: &mut i32, d: usize) {
        let nd = self.ndir();
        if d >= nd {
            self.step_forw(x, y, d - nd);
            return;
        }
        match self.gtype {
            0 => {
                if d == 1 {
                    *y -= 1;
                } else {
                    *x -= 1;
                }
            }
            1 => match d {
                0 => {
                    if (*x & 1) == 1 {
                        *y += 1;
                    }
                    *x -= 1;
                }
                1 => {
                    if (*x & 1) == 0 {
                        *y -= 1;
                    }
                    *x -= 1;
                }
                2 => *y -= 1,
                _ => {}
            },
            2 => match d {
                2 => {
                    if (*y & 1) == 1 {
                        *x += 1;
                    }
                    *y -= 1;
                }
                1 => {
                    if (*y & 1) == 0 {
                        *x -= 1;
                    }
                    *y -= 1;
                }
                0 => *x -= 1,
                _ => {}
            },
            3 | 4 => {
                if d == 1 {
                    *y -= 1;
                } else {
                    *x = (*x + self.width - 1) % self.width;
                }
            }
            _ => {}
        }
    }

    pub fn step_forw(&self, x: &mut i32, y: &mut i32, d: usize) {
        let nd = self.ndir();
        if d >= nd {
            self.step_back(x, y, d - nd);
            return;
        }
        match self.gtype {
            0 => {
                if d == 1 {
                    *y += 1;
                } else {
                    *x += 1;
                }
            }
            1 => match d {
                0 => {
                    *x += 1;
                    if (*x & 1) == 1 {
                        *y -= 1;
                    }
                }
                1 => {
                    *x += 1;
                    if (*x & 1) == 0 {
                        *y += 1;
                    }
                }
                2 => *y += 1,
                _ => {}
            },
            2 => match d {
                2 => {
                    *y += 1;
                    if (*y & 1) == 1 {
                        *x -= 1;
                    }
                }
                1 => {
                    *y += 1;
                    if (*y & 1) == 0 {
                        *x += 1;
                    }
                }
                0 => *x += 1,
                _ => {}
            },
            3 | 4 => {
                if d == 1 {
                    *y += 1;
                } else {
                    *x = (*x + 1) % self.width;
                }
            }
            _ => {}
        }
    }

    fn step_forw_m(&self, x: &mut i32, y: &mut i32, d: usize) {
        let x0 = *x;
        let y0 = *y;
        while self.is_merge(*x, *y, d) {
            self.step_forw(x, y, d);
            if *x == x0 && *y == y0 {
                return;
            }
        }
        self.step_forw(x, y, d);
    }

    pub fn is_bar(&self, mut x: i32, mut y: i32, mut d: usize) -> bool {
        let nd = self.ndir();
        if d >= nd {
            d -= nd;
            self.step_back(&mut x, &mut y, d);
        }
        if !self.is_ingrid(x, y) {
            return false;
        }
        let u = (self.square(x, y).unwrap().bars >> d) & 1;
        self.step_forw(&mut x, &mut y, d);
        if !self.is_ingrid(x, y) {
            return false;
        }
        u != 0
    }

    pub fn is_merge(&self, mut x: i32, mut y: i32, mut d: usize) -> bool {
        let nd = self.ndir();
        if d >= nd {
            d -= nd;
            self.step_back(&mut x, &mut y, d);
        }
        if !self.is_ingrid(x, y) {
            return false;
        }
        let u = (self.square(x, y).unwrap().merge >> d) & 1;
        self.step_forw(&mut x, &mut y, d);
        if !self.is_ingrid(x, y) {
            return false;
        }
        u != 0
    }

    pub fn clear_before(&self, x: i32, y: i32, d: usize) -> bool {
        let mut tx = x;
        let mut ty = y;
        self.step_back(&mut tx, &mut ty, d);
        if !self.is_ingrid(tx, ty) {
            return false;
        }
        if !self.is_clear(tx, ty) {
            return false;
        }
        if self.is_bar(tx, ty, d) {
            return false;
        }
        true
    }

    pub fn clear_after(&self, x: i32, y: i32, d: usize) -> bool {
        let mut tx = x;
        let mut ty = y;
        self.step_forw(&mut tx, &mut ty, d);
        if !self.is_ingrid(tx, ty) {
            return false;
        }
        if !self.is_clear(tx, ty) {
            return false;
        }
        if self.is_bar(x, y, d) {
            return false;
        }
        true
    }

    fn clear_after_m(&self, x: i32, y: i32, d: usize) -> bool {
        let mut tx = x;
        let mut ty = y;
        self.step_forw_m(&mut tx, &mut ty, d);
        if x == tx && y == ty {
            return false;
        }
        if !self.is_ingrid(tx, ty) {
            return false;
        }
        if !self.is_clear(tx, ty) {
            return false;
        }
        self.step_back(&mut tx, &mut ty, d);
        if self.is_bar(tx, ty, d) {
            return false;
        }
        true
    }

    pub fn get_merge_dir(&self, x: i32, y: i32) -> i32 {
        if !self.is_clear(x, y) {
            return -1;
        }
        let nd = self.ndir();
        for d in 0..nd {
            if self.is_merge(x, y, d) || self.is_merge(x, y, d + nd) {
                return d as i32;
            }
        }
        0
    }

    pub fn get_merge_rep_d(&self, x: i32, y: i32, d: usize) -> (i32, i32) {
        let nd = self.ndir();
        let mut mx = x;
        let mut my = y;
        if !self.is_clear(x, y) {
            return (mx, my);
        }
        if !self.is_merge(x, y, d + nd) {
            return (mx, my);
        }
        let mut cx = x;
        let mut cy = y;
        let x0 = x;
        let y0 = y;
        loop {
            self.step_back(&mut cx, &mut cy, d);
            if !self.is_clear(cx, cy) {
                break;
            }
            if (cx + cy * (MXSZ as i32)) < (mx + my * (MXSZ as i32)) {
                mx = cx;
                my = cy;
            }
            if cx == x0 && cy == y0 {
                break;
            }
            if !self.is_merge(cx, cy, d + nd) {
                mx = cx;
                my = cy;
                break;
            }
        }
        (mx, my)
    }

    pub fn get_merge_rep(&self, x: i32, y: i32) -> (i32, i32) {
        let d = self.get_merge_dir(x, y);
        if d < 0 {
            return (x, y);
        }
        self.get_merge_rep_d(x, y, d as usize)
    }

    pub fn is_own_merge_rep(&self, x: i32, y: i32) -> bool {
        let (mx, my) = self.get_merge_rep(x, y);
        x == mx && y == my
    }

    pub fn get_merge_group(&self, x: i32, y: i32) -> Vec<(i32, i32)> {
        if !self.is_clear(x, y) {
            return vec![(x, y)];
        }
        let d = self.get_merge_dir(x, y);
        debug_assert!(d >= 0);
        self.get_merge_group_d(x, y, d as usize)
    }

    pub fn get_merge_group_d(&self, x: i32, y: i32, d: usize) -> Vec<(i32, i32)> {
        if !self.is_clear(x, y) {
            return vec![(x, y)];
        }
        let (mut cx, mut cy) = self.get_merge_rep_d(x, y, d);
        let x0 = cx;
        let y0 = cy;
        let mut out = Vec::new();
        loop {
            out.push((cx, cy));
            if !self.is_merge(cx, cy, d) {
                break;
            }
            self.step_forw(&mut cx, &mut cy, d);
            if !self.is_clear(cx, cy) {
                break;
            }
            if cx == x0 && cy == y0 {
                break;
            }
        }
        out
    }

    pub fn is_start_of_light(&self, x: i32, y: i32, d: usize) -> bool {
        if !self.is_clear(x, y) {
            return false;
        }
        if self.clear_before(x, y, d) {
            return false;
        }
        self.clear_after_m(x, y, d)
    }

    pub fn get_light(&self, x: i32, y: i32, d: usize) -> Option<Vec<(i32, i32)>> {
        if !self.is_clear(x, y) {
            return None;
        }
        // find start
        let mut sx = x;
        let mut sy = y;
        let x0 = x;
        let y0 = y;
        while self.clear_before(sx, sy, d) {
            self.step_back(&mut sx, &mut sy, d);
            if sx == x0 && sy == y0 {
                return None;
            }
        }
        let mut out = Vec::new();
        let mut cx = sx;
        let mut cy = sy;
        loop {
            let (rx, ry) = self.get_merge_rep(cx, cy);
            out.push((rx, ry));
            if !self.clear_after_m(cx, cy, d) {
                break;
            }
            self.step_forw_m(&mut cx, &mut cy, d);
        }
        Some(out)
    }

    pub fn get_word(&self, x: i32, y: i32, d: usize) -> Option<String> {
        let light = self.get_light(x, y, d)?;
        let mut s = String::with_capacity(light.len());
        for (lx, ly) in light {
            let ch = self.square(lx, ly).map(|q| q.ch).unwrap_or(b' ');
            s.push(ch as char);
        }
        Some(s)
    }

    pub fn compute_numbers(&mut self) {
        for y in 0..self.height {
            for x in 0..self.width {
                if let Some(sq) = self.square_mut(x, y) {
                    sq.number = -1;
                }
            }
        }

        let mut num = 1;
        for y in 0..self.height {
            for i0 in 0..self.width {
                let x = if self.gtype == 1 {
                    // special case for hexH grid
                    let mut i = i0 * 2;
                    if i >= self.width {
                        i = (i - self.width) | 1;
                    }
                    i
                } else {
                    i0
                };

                if self.is_clear(x, y) {
                    for d in 0..self.ndir() {
                        if self.is_start_of_light(x, y, d) {
                            if let Some(sq) = self.square_mut(x, y) {
                                sq.number = num;
                            }
                            num += 1;
                            break;
                        }
                    }
                }
            }
        }
    }

    pub fn iter_lights(&self) -> impl Iterator<Item = (usize, i32, i32, String, i32)> + '_ {
        // yields (dir, startx, starty, word, number)
        (0..self.ndir()).flat_map(move |d| {
            (0..self.height).flat_map(move |y| {
                (0..self.width).filter_map(move |x| {
                    if !self.is_start_of_light(x, y, d) {
                        return None;
                    }
                    let word = self.get_word(x, y, d)?;
                    let number = self.square(x, y).map(|sq| sq.number).unwrap_or(-1);
                    Some((d, x, y, word, number))
                })
            })
        })
    }
}
