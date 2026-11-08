// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The byte format of the session's own messages (and, later, replication's): little-endian
//! fixed-width numbers, LEB128 varints and length-prefixed strings. A [`Reader`] never panics on
//! bad input: it returns `None`, so a corrupt packet is refused rather than trusted.

/// Appends values to a byte buffer.
#[derive(Debug, Default)]
pub struct Writer {
    pub buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.buf.push(v);
        self
    }

    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub fn u128(&mut self, v: u128) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    /// An unsigned number in 1 to 10 bytes (7 bits each, low first).
    pub fn varint(&mut self, mut v: u64) -> &mut Self {
        while v >= 0x80 {
            self.buf.push((v as u8 & 0x7f) | 0x80);
            v >>= 7;
        }
        self.buf.push(v as u8);
        self
    }

    pub fn str(&mut self, s: &str) -> &mut Self {
        self.varint(s.len() as u64);
        self.buf.extend_from_slice(s.as_bytes());
        self
    }

    pub fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(b);
        self
    }

    pub fn finish(self) -> Vec<u8> {
        self.buf
    }
}

/// Reads values from a byte slice.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Reader { buf }
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.buf.len() < n {
            return None;
        }
        let (head, rest) = self.buf.split_at(n);
        self.buf = rest;
        Some(head)
    }

    pub fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    pub fn u32(&mut self) -> Option<u32> {
        self.take(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap_or_default()))
    }

    pub fn u64(&mut self) -> Option<u64> {
        self.take(8)
            .map(|b| u64::from_le_bytes(b.try_into().unwrap_or_default()))
    }

    pub fn u128(&mut self) -> Option<u128> {
        self.take(16)
            .map(|b| u128::from_le_bytes(b.try_into().unwrap_or_default()))
    }

    pub fn varint(&mut self) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..70).step_by(7) {
            let b = self.u8()?;
            if shift == 63 && b > 1 {
                return None;
            }
            v |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }

    /// A string of at most `max` bytes (a hostile length cannot allocate).
    pub fn str(&mut self, max: usize) -> Option<String> {
        let len = usize::try_from(self.varint()?).ok()?;
        if len > max {
            return None;
        }
        String::from_utf8(self.take(len)?.to_vec()).ok()
    }

    /// Everything not read yet.
    pub fn rest(self) -> &'a [u8] {
        self.buf
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // M8-NET-04
    #[test]
    fn values_round_trip() {
        let mut w = Writer::new();
        w.u8(7)
            .u32(0xdead_beef)
            .u64(u64::MAX - 3)
            .u128(1 << 100)
            .str("héllo")
            .varint(300)
            .bytes(b"tail");
        let bytes = w.finish();
        let mut r = Reader::new(&bytes);
        assert_eq!(r.u8(), Some(7));
        assert_eq!(r.u32(), Some(0xdead_beef));
        assert_eq!(r.u64(), Some(u64::MAX - 3));
        assert_eq!(r.u128(), Some(1 << 100));
        assert_eq!(r.str(64).as_deref(), Some("héllo"));
        assert_eq!(r.varint(), Some(300));
        assert_eq!(r.rest(), b"tail");
    }

    // M8-NET-04
    #[test]
    fn varints_use_as_few_bytes_as_they_can() {
        for (v, len) in [
            (0u64, 1),
            (127, 1),
            (128, 2),
            (16_383, 2),
            (16_384, 3),
            (u64::MAX, 10),
        ] {
            let mut w = Writer::new();
            w.varint(v);
            assert_eq!(w.buf.len(), len, "{v}");
            assert_eq!(Reader::new(&w.buf).varint(), Some(v));
        }
    }

    // M8-NET-04
    #[test]
    fn truncated_or_hostile_input_returns_none_instead_of_panicking() {
        assert_eq!(Reader::new(&[1, 2]).u32(), None);
        assert_eq!(Reader::new(&[0x80; 11]).varint(), None);
        // A varint that overflows 64 bits.
        let mut over = vec![0xff; 9];
        over.push(0x02);
        assert_eq!(Reader::new(&over).varint(), None);
        // A string claiming a gigabyte.
        let mut w = Writer::new();
        w.varint(1 << 30);
        assert_eq!(Reader::new(&w.buf).str(1 << 20), None);
        // Invalid UTF-8.
        assert_eq!(Reader::new(&[2, 0xff, 0xfe]).str(8), None);
    }
}
