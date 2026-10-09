//! Hashes: a fast hasher for maps and sets keyed by cell positions, and FNV-1a, the one content
//! hash of the workspace (files, embedded images, generated-file headers, history entries).

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

/// A fast, non-cryptographic hasher for small integer keys such as cell positions (the
/// multiply-rotate scheme of rustc's `FxHasher`). Not for data an attacker chooses.
#[derive(Clone, Copy, Debug, Default)]
pub struct CellHasher {
    state: u64,
}

impl CellHasher {
    fn add(&mut self, value: u64) {
        self.state = (self.state.rotate_left(5) ^ value).wrapping_mul(0x517C_C1B7_2722_0A95);
    }
}

impl Hasher for CellHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.add(u64::from(byte));
        }
    }

    fn write_u32(&mut self, value: u32) {
        self.add(u64::from(value));
    }

    fn write_i32(&mut self, value: i32) {
        self.add(u64::from(value.cast_unsigned()));
    }

    fn write_u64(&mut self, value: u64) {
        self.add(value);
    }

    fn write_usize(&mut self, value: usize) {
        // usize is at most 64 bits on every platform Rust supports.
        self.add(value as u64);
    }

    fn finish(&self) -> u64 {
        self.state
    }
}

/// Builds [`CellHasher`]s.
pub type CellBuildHasher = BuildHasherDefault<CellHasher>;

/// A hash map keyed by cells (or other small integers) with [`CellHasher`].
pub type CellHashMap<K, V> = HashMap<K, V, CellBuildHasher>;

/// A hash set of cells (or other small integers) with [`CellHasher`].
pub type CellHashSet<K> = HashSet<K, CellBuildHasher>;

const FNV64_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
const FNV64_PRIME: u64 = 0x0000_0100_0000_01B3;
const FNV32_OFFSET: u32 = 0x811C_9DC5;
const FNV32_PRIME: u32 = 0x0100_0193;

/// FNV-1a, 64 bits, over bytes written one call after another: the content hash of the
/// workspace. It detects changes; it is not a defence against someone forging content.
#[derive(Clone, Copy, Debug)]
pub struct Fnv64 {
    state: u64,
}

impl Fnv64 {
    /// A hash of nothing yet.
    pub const fn new() -> Fnv64 {
        Fnv64 {
            state: FNV64_OFFSET,
        }
    }
}

impl Default for Fnv64 {
    fn default() -> Fnv64 {
        Fnv64::new()
    }
}

impl Hasher for Fnv64 {
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.state = (self.state ^ u64::from(byte)).wrapping_mul(FNV64_PRIME);
        }
    }

    fn finish(&self) -> u64 {
        self.state
    }
}

/// FNV-1a, 64 bits, of `bytes`.
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash = Fnv64::new();
    hash.write(bytes);
    hash.finish()
}

/// FNV-1a, 32 bits, of `bytes`.
pub fn fnv1a_32(bytes: &[u8]) -> u32 {
    bytes.iter().fold(FNV32_OFFSET, |state, &byte| {
        (state ^ u32::from(byte)).wrapping_mul(FNV32_PRIME)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_matches_the_published_test_vectors() {
        assert_eq!(fnv1a_64(b""), 0xCBF2_9CE4_8422_2325);
        assert_eq!(fnv1a_64(b"a"), 0xAF63_DC4C_8601_EC8C);
        assert_eq!(fnv1a_64(b"foobar"), 0x8594_4171_F739_67E8);
        assert_eq!(fnv1a_32(b""), 0x811C_9DC5);
        assert_eq!(fnv1a_32(b"a"), 0xE40C_292C);
        assert_eq!(fnv1a_32(b"foobar"), 0xBF9C_F968);
        let mut parts = Fnv64::new();
        parts.write(b"foo");
        parts.write(b"bar");
        assert_eq!(
            parts.finish(),
            fnv1a_64(b"foobar"),
            "writing in pieces changes nothing"
        );
    }

    #[test]
    fn cell_maps_hash_positions() {
        let mut map: CellHashMap<(i32, i32), u8> = CellHashMap::default();
        map.insert((-1, 2), 1);
        map.insert((2, -1), 2);
        assert_eq!(
            (map.get(&(-1, 2)), map.get(&(2, -1)), map.get(&(0, 0))),
            (Some(&1), Some(&2), None)
        );
    }
}
