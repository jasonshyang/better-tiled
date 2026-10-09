//! zlib and gzip streams with exactly the bytes zlib produces for Tiled's call.
//!
//! Tiled compresses layer data with zlib's
//! `deflateInit2(level, Z_DEFLATED, 15 (+16 for gzip), 8, Z_DEFAULT_STRATEGY)` and one
//! `deflate(Z_FINISH)` (`libtiled/compression.cpp`). Other deflate implementations produce other,
//! equally valid streams, which would make every save of an unchanged map rewrite all its layer
//! data. This module is a port of the parts of zlib's `deflate.c` and `trees.c` that call uses:
//! levels 1 to 9, the default strategy, a 32 KiB window, `memLevel` 8, all input at hand. It is
//! the one place in the workspace that reproduces another program's bytes rather than its
//! behaviour; `deflate_vectors.in` (written by `scripts/deflate-vectors.py` with zlib itself)
//! pins it.
//!
//! The port keeps zlib's array arithmetic, whose indices zlib's invariants bound (hash chains
//! masked to the window, codes below their table sizes), so this module alone allows indexing;
//! the tests run every level over inputs up to several windows long.
//!
//! This is an altered version of zlib's source code (ported to Rust and reduced to the code
//! paths above), not the original software. The original notice follows:
//!
//! ```text
//! Copyright (C) 1995-2026 Jean-loup Gailly and Mark Adler
//!
//! This software is provided 'as-is', without any express or implied
//! warranty.  In no event will the authors be held liable for any damages
//! arising from the use of this software.
//!
//! Permission is granted to anyone to use this software for any purpose,
//! including commercial applications, and to alter it and redistribute it
//! freely, subject to the following restrictions:
//!
//! 1. The origin of this software must not be misrepresented; you must not
//!    claim that you wrote the original software. If you use this software
//!    in a product, an acknowledgment in the product documentation would be
//!    appreciated but is not required.
//! 2. Altered source versions must be plainly marked as such, and must not be
//!    misrepresented as being the original software.
//! 3. This notice may not be removed or altered from any source distribution.
//!
//! Jean-loup Gailly        Mark Adler
//! jloup@gzip.org          madler@alumni.caltech.edu
//! ```

#![allow(clippy::indexing_slicing)]

use std::sync::OnceLock;

const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const MIN_LOOKAHEAD: usize = MAX_MATCH + MIN_MATCH + 1;
const LENGTH_CODES: usize = 29;
const LITERALS: usize = 256;
const L_CODES: usize = LITERALS + 1 + LENGTH_CODES;
const D_CODES: usize = 30;
const BL_CODES: usize = 19;
const HEAP_SIZE: usize = 2 * L_CODES + 1;
const MAX_BITS: usize = 15;
const MAX_BL_BITS: usize = 7;
const END_BLOCK: usize = 256;
const REP_3_6: usize = 16;
const REPZ_3_10: usize = 17;
const REPZ_11_138: usize = 18;
const BUF_SIZE: i32 = 16;
const STORED_BLOCK: u32 = 0;
const STATIC_TREES: u32 = 1;
const DYN_TREES: u32 = 2;

const W_BITS: usize = 15;
const W_SIZE: usize = 1 << W_BITS;
const W_MASK: usize = W_SIZE - 1;
const MEM_LEVEL: usize = 8;
const HASH_BITS: usize = MEM_LEVEL + 7;
const HASH_SIZE: usize = 1 << HASH_BITS;
const HASH_MASK: u32 = (HASH_SIZE - 1) as u32;
const HASH_SHIFT: u32 = HASH_BITS.div_ceil(MIN_MATCH) as u32;
const LIT_BUFSIZE: usize = 1 << (MEM_LEVEL + 6);
const MAX_DIST: usize = W_SIZE - MIN_LOOKAHEAD;
const TOO_FAR: u32 = 4096;
const WIN_INIT: usize = MAX_MATCH;

const EXTRA_LBITS: [i32; LENGTH_CODES] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const EXTRA_DBITS: [i32; D_CODES] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const EXTRA_BLBITS: [i32; BL_CODES] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 3, 7];
const BL_ORDER: [usize; BL_CODES] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// Levels → (good_length, max_lazy, nice_length, max_chain); levels 1–3 use `deflate_fast`.
const CONFIGURATION_TABLE: [(u32, u32, i32, u32); 10] = [
    (0, 0, 0, 0),
    (4, 4, 8, 4),
    (4, 5, 16, 8),
    (4, 6, 32, 32),
    (4, 4, 16, 16),
    (8, 16, 32, 32),
    (8, 16, 128, 128),
    (8, 32, 128, 256),
    (32, 128, 258, 1024),
    (32, 258, 258, 4096),
];

/// zlib's `ct_data`: `fc` is the frequency or the code, `dl` the father ("dad") or the length.
/// The two meanings share storage in zlib and the algorithms rely on that.
#[derive(Copy, Clone, Default)]
struct Ct {
    fc: u16,
    dl: u16,
}

struct StaticTables {
    ltree: [Ct; L_CODES + 2],
    dtree: [Ct; D_CODES],
    dist_code: [u8; 512],
    length_code: [u8; MAX_MATCH - MIN_MATCH + 1],
    base_length: [i32; LENGTH_CODES],
    base_dist: [i32; D_CODES],
}

/// `tr_static_init`.
fn tables() -> &'static StaticTables {
    static TABLES: OnceLock<StaticTables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut t = StaticTables {
            ltree: [Ct::default(); L_CODES + 2],
            dtree: [Ct::default(); D_CODES],
            dist_code: [0; 512],
            length_code: [0; MAX_MATCH - MIN_MATCH + 1],
            base_length: [0; LENGTH_CODES],
            base_dist: [0; D_CODES],
        };
        let mut length = 0usize;
        let mut code = 0usize;
        while code < LENGTH_CODES - 1 {
            t.base_length[code] = length as i32;
            for _ in 0..(1 << EXTRA_LBITS[code]) {
                t.length_code[length] = code as u8;
                length += 1;
            }
            code += 1;
        }
        // Length 258 can be coded two ways; use the cheaper code 285.
        t.length_code[length - 1] = code as u8;

        let mut dist = 0usize;
        code = 0;
        while code < 16 {
            t.base_dist[code] = dist as i32;
            for _ in 0..(1 << EXTRA_DBITS[code]) {
                t.dist_code[dist] = code as u8;
                dist += 1;
            }
            code += 1;
        }
        dist >>= 7;
        while code < D_CODES {
            t.base_dist[code] = (dist << 7) as i32;
            for _ in 0..(1 << (EXTRA_DBITS[code] - 7)) {
                t.dist_code[256 + dist] = code as u8;
                dist += 1;
            }
            code += 1;
        }

        let mut bl_count = [0u16; MAX_BITS + 1];
        for (n, ct) in t.ltree.iter_mut().enumerate() {
            let len = match n {
                0..=143 => 8,
                144..=255 => 9,
                256..=279 => 7,
                _ => 8,
            };
            ct.dl = len;
            bl_count[len as usize] += 1;
        }
        gen_codes(&mut t.ltree, (L_CODES + 1) as i32, &bl_count);
        for (n, ct) in t.dtree.iter_mut().enumerate() {
            ct.dl = 5;
            ct.fc = bi_reverse(n as u32, 5) as u16;
        }
        t
    })
}

struct StaticDesc {
    static_tree: Option<fn(&StaticTables) -> &[Ct]>,
    extra_bits: &'static [i32],
    extra_base: usize,
    elems: usize,
    max_length: usize,
}

const L_DESC: StaticDesc = StaticDesc {
    static_tree: Some(|t| &t.ltree),
    extra_bits: &EXTRA_LBITS,
    extra_base: LITERALS + 1,
    elems: L_CODES,
    max_length: MAX_BITS,
};
const D_DESC: StaticDesc = StaticDesc {
    static_tree: Some(|t| &t.dtree),
    extra_bits: &EXTRA_DBITS,
    extra_base: 0,
    elems: D_CODES,
    max_length: MAX_BITS,
};
const BL_DESC: StaticDesc = StaticDesc {
    static_tree: None,
    extra_bits: &EXTRA_BLBITS,
    extra_base: 0,
    elems: BL_CODES,
    max_length: MAX_BL_BITS,
};

/// Reverses the first `len` bits of a code.
fn bi_reverse(mut code: u32, mut len: i32) -> u32 {
    let mut res = 0;
    loop {
        res |= code & 1;
        code >>= 1;
        res <<= 1;
        len -= 1;
        if len <= 0 {
            break;
        }
    }
    res >> 1
}

/// Generates the codes for a tree given the bit length counts.
fn gen_codes(tree: &mut [Ct], max_code: i32, bl_count: &[u16; MAX_BITS + 1]) {
    let mut next_code = [0u16; MAX_BITS + 1];
    let mut code: u32 = 0;
    for bits in 1..=MAX_BITS {
        code = (code + u32::from(bl_count[bits - 1])) << 1;
        next_code[bits] = code as u16;
    }
    for n in 0..=max_code.max(-1) {
        let ct = &mut tree[n as usize];
        let len = ct.dl as usize;
        if len == 0 {
            continue;
        }
        ct.fc = bi_reverse(u32::from(next_code[len]), len as i32) as u16;
        next_code[len] = next_code[len].wrapping_add(1);
    }
}

/// Heap and statistics shared by the tree builders (separate from the trees for borrowing).
struct HeapState {
    heap: [i32; 2 * L_CODES + 1],
    heap_len: i32,
    heap_max: i32,
    depth: [u8; 2 * L_CODES + 1],
    bl_count: [u16; MAX_BITS + 1],
    opt_len: u64,
    static_len: u64,
}

fn smaller(tree: &[Ct], n: i32, m: i32, depth: &[u8]) -> bool {
    let (tn, tm) = (tree[n as usize].fc, tree[m as usize].fc);
    tn < tm || (tn == tm && depth[n as usize] <= depth[m as usize])
}

fn pqdownheap(hs: &mut HeapState, tree: &[Ct], mut k: i32) {
    let v = hs.heap[k as usize];
    let mut j = k << 1;
    while j <= hs.heap_len {
        if j < hs.heap_len
            && smaller(
                tree,
                hs.heap[(j + 1) as usize],
                hs.heap[j as usize],
                &hs.depth,
            )
        {
            j += 1;
        }
        if smaller(tree, v, hs.heap[j as usize], &hs.depth) {
            break;
        }
        hs.heap[k as usize] = hs.heap[j as usize];
        k = j;
        j <<= 1;
    }
    hs.heap[k as usize] = v;
}

fn gen_bitlen(hs: &mut HeapState, tree: &mut [Ct], desc: &StaticDesc, max_code: i32) {
    let stree = desc.static_tree.map(|f| f(tables()));
    let max_length = desc.max_length;
    let mut overflow = 0i32;
    hs.bl_count = [0; MAX_BITS + 1];

    let root = hs.heap[hs.heap_max as usize] as usize;
    tree[root].dl = 0;
    let mut h = hs.heap_max as usize + 1;
    while h < HEAP_SIZE {
        let n = hs.heap[h] as usize;
        let mut bits = tree[tree[n].dl as usize].dl as usize + 1;
        if bits > max_length {
            bits = max_length;
            overflow += 1;
        }
        tree[n].dl = bits as u16;
        h += 1;
        if n as i32 > max_code {
            continue;
        }
        hs.bl_count[bits] += 1;
        let xbits = if n >= desc.extra_base {
            desc.extra_bits[n - desc.extra_base]
        } else {
            0
        };
        let f = u64::from(tree[n].fc);
        hs.opt_len = hs.opt_len.wrapping_add(f * (bits as u64 + xbits as u64));
        if let Some(stree) = stree {
            hs.static_len = hs
                .static_len
                .wrapping_add(f * (u64::from(stree[n].dl) + xbits as u64));
        }
    }
    if overflow == 0 {
        return;
    }
    loop {
        let mut bits = max_length - 1;
        while hs.bl_count[bits] == 0 {
            bits -= 1;
        }
        hs.bl_count[bits] -= 1;
        hs.bl_count[bits + 1] += 2;
        hs.bl_count[max_length] -= 1;
        overflow -= 2;
        if overflow <= 0 {
            break;
        }
    }
    let mut h = HEAP_SIZE;
    let mut bits = max_length;
    while bits != 0 {
        let mut n = hs.bl_count[bits];
        while n != 0 {
            h -= 1;
            let m = hs.heap[h];
            if m > max_code {
                continue;
            }
            let ct = &mut tree[m as usize];
            if usize::from(ct.dl) != bits {
                let delta = (bits as u64)
                    .wrapping_sub(u64::from(ct.dl))
                    .wrapping_mul(u64::from(ct.fc));
                hs.opt_len = hs.opt_len.wrapping_add(delta);
                ct.dl = bits as u16;
            }
            n -= 1;
        }
        bits -= 1;
    }
}

/// Builds one Huffman tree; returns its `max_code`.
fn build_tree(hs: &mut HeapState, tree: &mut [Ct], desc: &StaticDesc) -> i32 {
    let stree = desc.static_tree.map(|f| f(tables()));
    let elems = desc.elems;
    let mut max_code: i32 = -1;
    hs.heap_len = 0;
    hs.heap_max = HEAP_SIZE as i32;
    for (n, ct) in tree.iter_mut().enumerate().take(elems) {
        if ct.fc != 0 {
            hs.heap_len += 1;
            hs.heap[hs.heap_len as usize] = n as i32;
            max_code = n as i32;
            hs.depth[n] = 0;
        } else {
            ct.dl = 0;
        }
    }
    // Force at least two codes of non-zero frequency.
    while hs.heap_len < 2 {
        let node = if max_code < 2 {
            max_code += 1;
            max_code
        } else {
            0
        };
        hs.heap_len += 1;
        hs.heap[hs.heap_len as usize] = node;
        tree[node as usize].fc = 1;
        hs.depth[node as usize] = 0;
        hs.opt_len = hs.opt_len.wrapping_sub(1);
        if let Some(stree) = stree {
            hs.static_len = hs
                .static_len
                .wrapping_sub(u64::from(stree[node as usize].dl));
        }
    }
    let mut n = hs.heap_len / 2;
    while n >= 1 {
        pqdownheap(hs, tree, n);
        n -= 1;
    }
    let mut node = elems as i32;
    loop {
        let n = hs.heap[1];
        hs.heap[1] = hs.heap[hs.heap_len as usize];
        hs.heap_len -= 1;
        pqdownheap(hs, tree, 1);
        let m = hs.heap[1];
        hs.heap_max -= 1;
        hs.heap[hs.heap_max as usize] = n;
        hs.heap_max -= 1;
        hs.heap[hs.heap_max as usize] = m;

        let (nu, mu, nodeu) = (n as usize, m as usize, node as usize);
        tree[nodeu].fc = tree[nu].fc.wrapping_add(tree[mu].fc);
        hs.depth[nodeu] = hs.depth[nu].max(hs.depth[mu]) + 1;
        tree[nu].dl = node as u16;
        tree[mu].dl = node as u16;
        hs.heap[1] = node;
        node += 1;
        pqdownheap(hs, tree, 1);
        if hs.heap_len < 2 {
            break;
        }
    }
    hs.heap_max -= 1;
    hs.heap[hs.heap_max as usize] = hs.heap[1];
    gen_bitlen(hs, tree, desc, max_code);
    gen_codes(tree, max_code, &hs.bl_count);
    max_code
}

struct Deflate<'a> {
    input: &'a [u8],
    next_in: usize,
    out: Vec<u8>,

    window: Vec<u8>,
    window_size: usize,
    high_water: usize,
    prev: Vec<u16>,
    head: Vec<u16>,
    ins_h: u32,
    block_start: i64,
    match_length: u32,
    prev_match: u32,
    match_available: bool,
    strstart: u32,
    match_start: u32,
    lookahead: u32,
    prev_length: u32,
    max_chain_length: u32,
    max_lazy_match: u32,
    good_match: u32,
    nice_match: i32,
    insert: u32,

    dyn_ltree: [Ct; HEAP_SIZE],
    dyn_dtree: [Ct; 2 * D_CODES + 1],
    bl_tree: [Ct; 2 * BL_CODES + 1],
    l_max_code: i32,
    d_max_code: i32,
    hs: HeapState,
    sym_buf: Vec<u8>,
    sym_next: usize,
    sym_end: usize,
    bi_buf: u16,
    bi_valid: i32,
}

impl<'a> Deflate<'a> {
    fn new(input: &'a [u8], level: usize) -> Deflate<'a> {
        let (good, lazy, nice, chain) = CONFIGURATION_TABLE[level];
        let mut s = Deflate {
            input,
            next_in: 0,
            out: Vec::with_capacity(input.len() / 4 + 64),
            window: vec![0; 2 * W_SIZE],
            window_size: 2 * W_SIZE,
            high_water: 0,
            prev: vec![0; W_SIZE],
            head: vec![0; HASH_SIZE],
            ins_h: 0,
            block_start: 0,
            match_length: MIN_MATCH as u32 - 1,
            prev_match: 0,
            match_available: false,
            strstart: 0,
            match_start: 0,
            lookahead: 0,
            prev_length: MIN_MATCH as u32 - 1,
            max_chain_length: chain,
            max_lazy_match: lazy,
            good_match: good,
            nice_match: nice,
            insert: 0,
            dyn_ltree: [Ct::default(); HEAP_SIZE],
            dyn_dtree: [Ct::default(); 2 * D_CODES + 1],
            bl_tree: [Ct::default(); 2 * BL_CODES + 1],
            l_max_code: 0,
            d_max_code: 0,
            hs: HeapState {
                heap: [0; 2 * L_CODES + 1],
                heap_len: 0,
                heap_max: 0,
                depth: [0; 2 * L_CODES + 1],
                bl_count: [0; MAX_BITS + 1],
                opt_len: 0,
                static_len: 0,
            },
            sym_buf: vec![0; LIT_BUFSIZE * 3],
            sym_next: 0,
            sym_end: (LIT_BUFSIZE - 1) * 3,
            bi_buf: 0,
            bi_valid: 0,
        };
        s.init_block();
        s
    }

    // --- Bit output (trees.c) -------------------------------------------------------------------

    fn put_short(&mut self, w: u16) {
        self.out.push((w & 0xff) as u8);
        self.out.push((w >> 8) as u8);
    }

    fn send_bits(&mut self, value: u32, length: i32) {
        let value = value as u16;
        if self.bi_valid > BUF_SIZE - length {
            self.bi_buf |= ((u32::from(value)) << self.bi_valid) as u16;
            self.put_short(self.bi_buf);
            self.bi_buf = (u32::from(value) >> (BUF_SIZE - self.bi_valid)) as u16;
            self.bi_valid += length - BUF_SIZE;
        } else {
            self.bi_buf |= ((u32::from(value)) << self.bi_valid) as u16;
            self.bi_valid += length;
        }
    }

    fn bi_windup(&mut self) {
        if self.bi_valid > 8 {
            self.put_short(self.bi_buf);
        } else if self.bi_valid > 0 {
            self.out.push(self.bi_buf as u8);
        }
        self.bi_buf = 0;
        self.bi_valid = 0;
    }

    fn init_block(&mut self) {
        for ct in &mut self.dyn_ltree[..L_CODES] {
            ct.fc = 0;
        }
        for ct in &mut self.dyn_dtree[..D_CODES] {
            ct.fc = 0;
        }
        for ct in &mut self.bl_tree[..BL_CODES] {
            ct.fc = 0;
        }
        self.dyn_ltree[END_BLOCK].fc = 1;
        self.hs.opt_len = 0;
        self.hs.static_len = 0;
        self.sym_next = 0;
    }

    fn tally_lit(&mut self, c: u8) -> bool {
        self.sym_buf[self.sym_next] = 0;
        self.sym_buf[self.sym_next + 1] = 0;
        self.sym_buf[self.sym_next + 2] = c;
        self.sym_next += 3;
        let ct = &mut self.dyn_ltree[c as usize];
        ct.fc = ct.fc.wrapping_add(1);
        self.sym_next == self.sym_end
    }

    fn tally_dist(&mut self, distance: u32, length: u32) -> bool {
        let t = tables();
        let dist = distance as u16;
        let len = length as u8;
        self.sym_buf[self.sym_next] = dist as u8;
        self.sym_buf[self.sym_next + 1] = (dist >> 8) as u8;
        self.sym_buf[self.sym_next + 2] = len;
        self.sym_next += 3;
        let dist = dist - 1;
        let lc = &mut self.dyn_ltree[t.length_code[len as usize] as usize + LITERALS + 1];
        lc.fc = lc.fc.wrapping_add(1);
        let dc = &mut self.dyn_dtree[d_code(t, u32::from(dist))];
        dc.fc = dc.fc.wrapping_add(1);
        self.sym_next == self.sym_end
    }

    /// Scans a literal or distance tree to determine the frequencies of the bit length codes.
    fn scan_tree(bl_tree: &mut [Ct], tree: &mut [Ct], max_code: i32) {
        let mut prevlen: i32 = -1;
        let mut nextlen = i32::from(tree[0].dl);
        let mut count = 0;
        let (mut max_count, mut min_count) = (7, 4);
        if nextlen == 0 {
            max_count = 138;
            min_count = 3;
        }
        tree[(max_code + 1) as usize].dl = 0xffff; // guard
        for n in 0..=max_code as usize {
            let curlen = nextlen;
            nextlen = i32::from(tree[n + 1].dl);
            count += 1;
            if count < max_count && curlen == nextlen {
                continue;
            } else if count < min_count {
                bl_tree[curlen as usize].fc += count as u16;
            } else if curlen != 0 {
                if curlen != prevlen {
                    bl_tree[curlen as usize].fc += 1;
                }
                bl_tree[REP_3_6].fc += 1;
            } else if count <= 10 {
                bl_tree[REPZ_3_10].fc += 1;
            } else {
                bl_tree[REPZ_11_138].fc += 1;
            }
            count = 0;
            prevlen = curlen;
            if nextlen == 0 {
                max_count = 138;
                min_count = 3;
            } else if curlen == nextlen {
                max_count = 6;
                min_count = 3;
            } else {
                max_count = 7;
                min_count = 4;
            }
        }
    }

    fn send_code(&mut self, code: usize, tree: Tree) {
        let ct = self.ct(tree, code);
        self.send_bits(u32::from(ct.fc), i32::from(ct.dl));
    }

    fn ct(&self, tree: Tree, n: usize) -> Ct {
        match tree {
            Tree::DynL => self.dyn_ltree[n],
            Tree::DynD => self.dyn_dtree[n],
            Tree::Bl => self.bl_tree[n],
            Tree::StaticL => tables().ltree[n],
            Tree::StaticD => tables().dtree[n],
        }
    }

    /// Sends a literal or distance tree in compressed form, using the codes in `bl_tree`.
    fn send_tree(&mut self, tree: Tree, max_code: i32) {
        let mut prevlen: i32 = -1;
        let mut nextlen = i32::from(self.ct(tree, 0).dl);
        let mut count = 0;
        let (mut max_count, mut min_count) = (7, 4);
        if nextlen == 0 {
            max_count = 138;
            min_count = 3;
        }
        for n in 0..=max_code as usize {
            let curlen = nextlen;
            nextlen = i32::from(self.ct(tree, n + 1).dl);
            count += 1;
            if count < max_count && curlen == nextlen {
                continue;
            } else if count < min_count {
                loop {
                    self.send_code(curlen as usize, Tree::Bl);
                    count -= 1;
                    if count == 0 {
                        break;
                    }
                }
            } else if curlen != 0 {
                if curlen != prevlen {
                    self.send_code(curlen as usize, Tree::Bl);
                    count -= 1;
                }
                self.send_code(REP_3_6, Tree::Bl);
                self.send_bits((count - 3) as u32, 2);
            } else if count <= 10 {
                self.send_code(REPZ_3_10, Tree::Bl);
                self.send_bits((count - 3) as u32, 3);
            } else {
                self.send_code(REPZ_11_138, Tree::Bl);
                self.send_bits((count - 11) as u32, 7);
            }
            count = 0;
            prevlen = curlen;
            if nextlen == 0 {
                max_count = 138;
                min_count = 3;
            } else if curlen == nextlen {
                max_count = 6;
                min_count = 3;
            } else {
                max_count = 7;
                min_count = 4;
            }
        }
    }

    /// Builds the bit length tree; returns the index in `BL_ORDER` of the last code to send.
    fn build_bl_tree(&mut self) -> i32 {
        Self::scan_tree(&mut self.bl_tree, &mut self.dyn_ltree, self.l_max_code);
        Self::scan_tree(&mut self.bl_tree, &mut self.dyn_dtree, self.d_max_code);
        build_tree(&mut self.hs, &mut self.bl_tree, &BL_DESC);
        let mut max_blindex = BL_CODES as i32 - 1;
        while max_blindex >= 3 {
            if self.bl_tree[BL_ORDER[max_blindex as usize]].dl != 0 {
                break;
            }
            max_blindex -= 1;
        }
        self.hs.opt_len = self
            .hs
            .opt_len
            .wrapping_add(3 * (max_blindex as u64 + 1) + 5 + 5 + 4);
        max_blindex
    }

    fn send_all_trees(&mut self, lcodes: i32, dcodes: i32, blcodes: i32) {
        self.send_bits((lcodes - 257) as u32, 5);
        self.send_bits((dcodes - 1) as u32, 5);
        self.send_bits((blcodes - 4) as u32, 4);
        for &code in BL_ORDER.iter().take(blcodes as usize) {
            let len = self.bl_tree[code].dl;
            self.send_bits(u32::from(len), 3);
        }
        self.send_tree(Tree::DynL, lcodes - 1);
        self.send_tree(Tree::DynD, dcodes - 1);
    }

    fn stored_block(&mut self, buf: usize, stored_len: usize, last: bool) {
        self.send_bits((STORED_BLOCK << 1) + u32::from(last), 3);
        self.bi_windup();
        self.put_short(stored_len as u16);
        self.put_short(!(stored_len as u16));
        let block = &self.window[buf..buf + stored_len];
        self.out.extend_from_slice(block);
    }

    fn compress_block(&mut self, ltree: Tree, dtree: Tree) {
        let t = tables();
        let mut sx = 0;
        while sx < self.sym_next {
            let mut dist = u32::from(self.sym_buf[sx]) | u32::from(self.sym_buf[sx + 1]) << 8;
            let mut lc = u32::from(self.sym_buf[sx + 2]);
            sx += 3;
            if dist == 0 {
                self.send_code(lc as usize, ltree);
            } else {
                let code = t.length_code[lc as usize] as usize;
                self.send_code(code + LITERALS + 1, ltree);
                let extra = EXTRA_LBITS[code];
                if extra != 0 {
                    lc -= t.base_length[code] as u32;
                    self.send_bits(lc, extra);
                }
                dist -= 1;
                let code = d_code(t, dist);
                self.send_code(code, dtree);
                let extra = EXTRA_DBITS[code];
                if extra != 0 {
                    dist -= t.base_dist[code] as u32;
                    self.send_bits(dist, extra);
                }
            }
        }
        self.send_code(END_BLOCK, ltree);
    }

    /// `_tr_flush_block`: picks dynamic trees, static trees or a stored block.
    fn tr_flush_block(&mut self, buf: Option<usize>, stored_len: u64, last: bool) {
        self.l_max_code = build_tree(&mut self.hs, &mut self.dyn_ltree, &L_DESC);
        self.d_max_code = build_tree(&mut self.hs, &mut self.dyn_dtree, &D_DESC);
        let max_blindex = self.build_bl_tree();

        let mut opt_lenb = self.hs.opt_len.wrapping_add(3 + 7) >> 3;
        let static_lenb = self.hs.static_len.wrapping_add(3 + 7) >> 3;
        if static_lenb <= opt_lenb {
            opt_lenb = static_lenb;
        }
        match buf {
            Some(buf) if stored_len + 4 <= opt_lenb => {
                self.stored_block(buf, stored_len as usize, last)
            }
            _ if static_lenb == opt_lenb => {
                self.send_bits((STATIC_TREES << 1) + u32::from(last), 3);
                self.compress_block(Tree::StaticL, Tree::StaticD);
            }
            _ => {
                self.send_bits((DYN_TREES << 1) + u32::from(last), 3);
                self.send_all_trees(self.l_max_code + 1, self.d_max_code + 1, max_blindex + 1);
                self.compress_block(Tree::DynL, Tree::DynD);
            }
        }
        self.init_block();
        if last {
            self.bi_windup();
        }
    }

    fn flush_block(&mut self, last: bool) {
        let buf = (self.block_start >= 0).then_some(self.block_start as usize);
        let stored_len = (i64::from(self.strstart) - self.block_start) as u64;
        self.tr_flush_block(buf, stored_len, last);
        self.block_start = i64::from(self.strstart);
    }

    // --- LZ77 (deflate.c) ----------------------------------------------------------------------

    fn update_hash(h: u32, c: u8) -> u32 {
        ((h << HASH_SHIFT) ^ u32::from(c)) & HASH_MASK
    }

    /// `INSERT_STRING`: returns the previous head of the hash chain.
    fn insert_string(&mut self, str: usize) -> u32 {
        self.ins_h = Self::update_hash(self.ins_h, self.window[str + MIN_MATCH - 1]);
        let head = self.head[self.ins_h as usize];
        self.prev[str & W_MASK] = head;
        self.head[self.ins_h as usize] = str as u16;
        u32::from(head)
    }

    fn slide_hash(&mut self) {
        let wsize = W_SIZE as u16;
        for h in self.head.iter_mut().chain(self.prev.iter_mut()) {
            *h = h.saturating_sub(wsize);
        }
    }

    fn read_buf(&mut self, at: usize, size: usize) -> usize {
        let len = (self.input.len() - self.next_in).min(size);
        self.window[at..at + len].copy_from_slice(&self.input[self.next_in..self.next_in + len]);
        self.next_in += len;
        len
    }

    fn fill_window(&mut self) {
        loop {
            let mut more = self.window_size - self.lookahead as usize - self.strstart as usize;
            if self.strstart as usize >= W_SIZE + MAX_DIST {
                self.window.copy_within(W_SIZE..W_SIZE + W_SIZE - more, 0);
                self.match_start = self.match_start.wrapping_sub(W_SIZE as u32);
                self.strstart -= W_SIZE as u32;
                self.block_start -= W_SIZE as i64;
                if self.insert > self.strstart {
                    self.insert = self.strstart;
                }
                self.slide_hash();
                more += W_SIZE;
            }
            if self.next_in == self.input.len() {
                break;
            }
            let n = self.read_buf(self.strstart as usize + self.lookahead as usize, more);
            self.lookahead += n as u32;

            if (self.lookahead + self.insert) as usize >= MIN_MATCH {
                let mut str = (self.strstart - self.insert) as usize;
                self.ins_h = u32::from(self.window[str]);
                self.ins_h = Self::update_hash(self.ins_h, self.window[str + 1]);
                while self.insert != 0 {
                    self.ins_h = Self::update_hash(self.ins_h, self.window[str + MIN_MATCH - 1]);
                    self.prev[str & W_MASK] = self.head[self.ins_h as usize];
                    self.head[self.ins_h as usize] = str as u16;
                    str += 1;
                    self.insert -= 1;
                    if ((self.lookahead + self.insert) as usize) < MIN_MATCH {
                        break;
                    }
                }
            }
            if !((self.lookahead as usize) < MIN_LOOKAHEAD && self.next_in != self.input.len()) {
                break;
            }
        }

        // Zero the bytes after the data that the match finder may look at (zlib's WIN_INIT).
        if self.high_water < self.window_size {
            let curr = self.strstart as usize + self.lookahead as usize;
            if self.high_water < curr {
                let init = (self.window_size - curr).min(WIN_INIT);
                self.window[curr..curr + init].fill(0);
                self.high_water = curr + init;
            } else if self.high_water < curr + WIN_INIT {
                let init =
                    (curr + WIN_INIT - self.high_water).min(self.window_size - self.high_water);
                self.window[self.high_water..self.high_water + init].fill(0);
                self.high_water += init;
            }
        }
    }

    fn longest_match(&mut self, mut cur_match: u32) -> u32 {
        let mut chain_length = self.max_chain_length;
        let scan = self.strstart as usize;
        let mut best_len = self.prev_length as usize;
        let mut nice_match = self.nice_match;
        let limit = if self.strstart as usize > MAX_DIST {
            self.strstart - MAX_DIST as u32
        } else {
            0
        };
        let w = &self.window;
        let mut scan_end1 = w[scan + best_len - 1];
        let mut scan_end = w[scan + best_len];

        if self.prev_length >= self.good_match {
            chain_length >>= 2;
        }
        if nice_match as u32 > self.lookahead {
            nice_match = self.lookahead as i32;
        }
        loop {
            let m = cur_match as usize;
            if w[m + best_len] == scan_end
                && w[m + best_len - 1] == scan_end1
                && w[m] == w[scan]
                && w[m + 1] == w[scan + 1]
            {
                // scan[2] == match[2] is implied by the hash; compare from offset 3, 8 at a
                // time, checking the end of the window only after each group (like zlib).
                let mut k = 2;
                loop {
                    let mut all = true;
                    for _ in 0..8 {
                        k += 1;
                        if w[scan + k] != w[m + k] {
                            all = false;
                            break;
                        }
                    }
                    if !all || k >= MAX_MATCH {
                        break;
                    }
                }
                let len = k;
                if len > best_len {
                    self.match_start = cur_match;
                    best_len = len;
                    if len as i32 >= nice_match {
                        break;
                    }
                    scan_end1 = w[scan + best_len - 1];
                    scan_end = w[scan + best_len];
                }
            }
            cur_match = u32::from(self.prev[cur_match as usize & W_MASK]);
            if cur_match <= limit {
                break;
            }
            chain_length -= 1;
            if chain_length == 0 {
                break;
            }
        }
        (best_len as u32).min(self.lookahead)
    }

    /// Levels 1–3: no lazy matching.
    fn deflate_fast(&mut self) {
        loop {
            if (self.lookahead as usize) < MIN_LOOKAHEAD {
                self.fill_window();
                if self.lookahead == 0 {
                    break;
                }
            }
            let mut hash_head = 0;
            if self.lookahead as usize >= MIN_MATCH {
                hash_head = self.insert_string(self.strstart as usize);
            }
            if hash_head != 0 && (self.strstart - hash_head) as usize <= MAX_DIST {
                self.match_length = self.longest_match(hash_head);
            }
            let bflush;
            if self.match_length as usize >= MIN_MATCH {
                bflush = self.tally_dist(
                    self.strstart - self.match_start,
                    self.match_length - MIN_MATCH as u32,
                );
                self.lookahead -= self.match_length;
                if self.match_length <= self.max_lazy_match && self.lookahead as usize >= MIN_MATCH
                {
                    self.match_length -= 1;
                    loop {
                        self.strstart += 1;
                        self.insert_string(self.strstart as usize);
                        self.match_length -= 1;
                        if self.match_length == 0 {
                            break;
                        }
                    }
                    self.strstart += 1;
                } else {
                    self.strstart += self.match_length;
                    self.match_length = 0;
                    let s = self.strstart as usize;
                    self.ins_h = u32::from(self.window[s]);
                    self.ins_h = Self::update_hash(self.ins_h, self.window[s + 1]);
                }
            } else {
                bflush = self.tally_lit(self.window[self.strstart as usize]);
                self.lookahead -= 1;
                self.strstart += 1;
            }
            if bflush {
                self.flush_block(false);
            }
        }
        self.insert = self.strstart.min(MIN_MATCH as u32 - 1);
        self.flush_block(true);
    }

    /// Levels 4–9: lazy matching.
    fn deflate_slow(&mut self) {
        loop {
            if (self.lookahead as usize) < MIN_LOOKAHEAD {
                self.fill_window();
                if self.lookahead == 0 {
                    break;
                }
            }
            let mut hash_head = 0;
            if self.lookahead as usize >= MIN_MATCH {
                hash_head = self.insert_string(self.strstart as usize);
            }
            self.prev_length = self.match_length;
            self.prev_match = self.match_start;
            self.match_length = MIN_MATCH as u32 - 1;
            if hash_head != 0
                && self.prev_length < self.max_lazy_match
                && (self.strstart - hash_head) as usize <= MAX_DIST
            {
                self.match_length = self.longest_match(hash_head);
                if self.match_length <= 5
                    && self.match_length as usize == MIN_MATCH
                    && self.strstart - self.match_start > TOO_FAR
                {
                    self.match_length = MIN_MATCH as u32 - 1;
                }
            }
            if self.prev_length as usize >= MIN_MATCH && self.match_length <= self.prev_length {
                let max_insert = (self.strstart + self.lookahead).wrapping_sub(MIN_MATCH as u32);
                let bflush = self.tally_dist(
                    self.strstart - 1 - self.prev_match,
                    self.prev_length - MIN_MATCH as u32,
                );
                self.lookahead -= self.prev_length - 1;
                self.prev_length -= 2;
                loop {
                    self.strstart += 1;
                    if self.strstart <= max_insert {
                        self.insert_string(self.strstart as usize);
                    }
                    self.prev_length -= 1;
                    if self.prev_length == 0 {
                        break;
                    }
                }
                self.match_available = false;
                self.match_length = MIN_MATCH as u32 - 1;
                self.strstart += 1;
                if bflush {
                    self.flush_block(false);
                }
            } else if self.match_available {
                let bflush = self.tally_lit(self.window[self.strstart as usize - 1]);
                if bflush {
                    self.flush_block(false);
                }
                self.strstart += 1;
                self.lookahead -= 1;
            } else {
                self.match_available = true;
                self.strstart += 1;
                self.lookahead -= 1;
            }
        }
        if self.match_available {
            self.tally_lit(self.window[self.strstart as usize - 1]);
            self.match_available = false;
        }
        self.insert = self.strstart.min(MIN_MATCH as u32 - 1);
        self.flush_block(true);
    }
}

#[derive(Copy, Clone)]
enum Tree {
    DynL,
    DynD,
    Bl,
    StaticL,
    StaticD,
}

fn d_code(t: &StaticTables, dist: u32) -> usize {
    if dist < 256 {
        t.dist_code[dist as usize] as usize
    } else {
        t.dist_code[256 + (dist >> 7) as usize] as usize
    }
}

fn adler32(data: &[u8]) -> u32 {
    const BASE: u32 = 65521;
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= BASE;
        b %= BASE;
    }
    (b << 16) | a
}

/// The stream format written around the deflate data.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Wrapper {
    /// A zlib stream (`compression="zlib"`).
    Zlib,
    /// A gzip member (`compression="gzip"`), with no name, no time and the Unix system code.
    Gzip,
}

/// Compresses like zlib's `deflateInit2(level, Z_DEFLATED, 15 or 31, 8, Z_DEFAULT_STRATEGY)`
/// followed by `deflate(Z_FINISH)`. A negative level is zlib's default (6); others are clamped
/// to 1–9.
pub(crate) fn compress(data: &[u8], level: i32, wrapper: Wrapper) -> Vec<u8> {
    let level = if level < 0 {
        6
    } else {
        level.clamp(1, 9) as usize
    };
    let mut s = Deflate::new(data, level);
    match wrapper {
        Wrapper::Zlib => {
            let level_flags = match level {
                0 | 1 => 0,
                2..=5 => 1,
                6 => 2,
                _ => 3,
            };
            let mut header: u16 = (8 + (((W_BITS - 8) as u16) << 4)) << 8;
            header |= level_flags << 6;
            header += 31 - (header % 31);
            s.out.extend_from_slice(&header.to_be_bytes());
        }
        Wrapper::Gzip => {
            let xfl = if level == 9 {
                2
            } else if level < 2 {
                4
            } else {
                0
            };
            s.out
                .extend_from_slice(&[31, 139, 8, 0, 0, 0, 0, 0, xfl, 3]);
        }
    }
    if level <= 3 {
        s.deflate_fast();
    } else {
        s.deflate_slow();
    }
    let mut out = s.out;
    match wrapper {
        Wrapper::Zlib => out.extend_from_slice(&adler32(data).to_be_bytes()),
        Wrapper::Gzip => {
            let mut crc = flate2::Crc::new();
            crc.update(data);
            out.extend_from_slice(&crc.sum().to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    /// The generator `scripts/deflate-vectors.py` mirrors.
    struct XorShift(u64);

    impl XorShift {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    const KINDS: [&str; 4] = ["random", "tiles", "text", "zeros"];

    fn generate(kind: &str, n: usize) -> Vec<u8> {
        let index = KINDS.iter().position(|k| *k == kind).unwrap() as u64;
        let mut r = XorShift(1 + n as u64 + 31 * index);
        match kind {
            "random" => (0..n).map(|_| (r.next() & 0xFF) as u8).collect(),
            "tiles" => {
                let mut out = Vec::new();
                let mut current: u64 = 1;
                while out.len() < n {
                    let v = r.next();
                    if v.is_multiple_of(7) {
                        current = (v >> 8) % 300;
                    }
                    let run = 1 + (v >> 20) % 40;
                    for _ in 0..run {
                        let gid = if !r.next().is_multiple_of(5) {
                            current
                        } else {
                            (r.next() >> 3) % 4000
                        };
                        out.extend_from_slice(&(gid as u32).to_le_bytes());
                    }
                }
                out.truncate(n);
                out
            }
            "text" => {
                const WORDS: [&[u8]; 10] = [
                    b"alpha", b"beta", b"gamma", b"delta", b"tile", b"layer", b"map", b"\n", b" ",
                    b",",
                ];
                let mut out = Vec::new();
                while out.len() < n {
                    out.extend_from_slice(WORDS[(r.next() % 10) as usize]);
                }
                out.truncate(n);
                out
            }
            _ => vec![0; n],
        }
    }

    /// (kind, size, level, gzip, compressed length, CRC-32 of the compressed bytes).
    const VECTORS: &[(&str, usize, i32, bool, usize, u32)] = &include!("deflate_vectors.in");

    fn decompressed(stream: &[u8], wrapper: Wrapper) -> Vec<u8> {
        let mut back = Vec::new();
        match wrapper {
            Wrapper::Gzip => flate2::read::GzDecoder::new(stream).read_to_end(&mut back),
            Wrapper::Zlib => flate2::read::ZlibDecoder::new(stream).read_to_end(&mut back),
        }
        .unwrap();
        back
    }

    #[test]
    fn streams_match_zlib_byte_for_byte() {
        let mut failures = Vec::new();
        for &(kind, n, level, gzip, len, crc) in VECTORS {
            let data = generate(kind, n);
            let wrapper = if gzip { Wrapper::Gzip } else { Wrapper::Zlib };
            let out = compress(&data, level, wrapper);
            let mut hasher = flate2::Crc::new();
            hasher.update(&out);
            if (out.len(), hasher.sum()) != (len, crc) {
                failures.push(format!(
                    "{kind} n={n} level={level} gzip={gzip}: {} bytes, zlib wrote {len}",
                    out.len()
                ));
            }
            assert_eq!(
                decompressed(&out, wrapper),
                data,
                "{kind} n={n} level={level}"
            );
        }
        assert_eq!(VECTORS.len(), 800);
        assert!(
            failures.is_empty(),
            "{} of {} differ:\n{}",
            failures.len(),
            VECTORS.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn any_input_round_trips_at_any_level() {
        let mut r = XorShift(99);
        for round in 0..40 {
            let n = (r.next() % 90_000) as usize;
            let alphabet = 1 + r.next() % 255;
            let data: Vec<u8> = (0..n).map(|_| (r.next() % alphabet) as u8).collect();
            for level in [-7, 0, 1, 4, 9, 12] {
                for wrapper in [Wrapper::Zlib, Wrapper::Gzip] {
                    let out = compress(&data, level, wrapper);
                    assert_eq!(
                        decompressed(&out, wrapper),
                        data,
                        "round {round} level {level}"
                    );
                }
            }
        }
    }
}
