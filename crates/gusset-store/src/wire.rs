//! Panic-free little-endian byte reading and writing helpers.

/// A bounds-checked cursor over a byte slice. Every read returns `None` instead of
/// panicking when the input is too short.
#[derive(Debug, Clone)]
pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    pub(crate) fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    pub(crate) fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.buf.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }

    pub(crate) fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N).and_then(|s| s.try_into().ok())
    }

    pub(crate) fn u8(&mut self) -> Option<u8> {
        self.array::<1>().map(|a| a[0])
    }

    pub(crate) fn u16(&mut self) -> Option<u16> {
        self.array().map(u16::from_le_bytes)
    }

    pub(crate) fn u32(&mut self) -> Option<u32> {
        self.array().map(u32::from_le_bytes)
    }

    pub(crate) fn u64(&mut self) -> Option<u64> {
        self.array().map(u64::from_le_bytes)
    }
}

/// Converts a `u64` file offset/length to `usize`, or `None` if it does not fit.
pub(crate) fn usize_of(v: u64) -> Option<usize> {
    usize::try_from(v).ok()
}

/// Lowercase hex encoding.
pub(crate) fn to_hex(bytes: &[u8]) -> String {
    let digit = |v: u8| char::from_digit(u32::from(v), 16).unwrap_or('0');
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(digit(b >> 4));
        s.push(digit(b & 0xf));
    }
    s
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Decodes exactly `2 * out.len()` hex digits into `out`. Returns false on any error.
pub(crate) fn from_hex_into(s: &str, out: &mut [u8]) -> bool {
    let b = s.as_bytes();
    if b.len() != out.len() * 2 {
        return false;
    }
    let (pairs, _) = b.as_chunks::<2>();
    for (o, pair) in out.iter_mut().zip(pairs) {
        match (hex_val(pair[0]), hex_val(pair[1])) {
            (Some(h), Some(l)) => *o = (h << 4) | l,
            _ => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_never_panics() {
        let mut r = Reader::new(&[1, 2, 3]);
        assert_eq!(r.u16(), Some(0x0201));
        assert_eq!(r.u16(), None);
        assert_eq!(r.u8(), Some(3));
        assert_eq!(r.u8(), None);
        assert_eq!(r.take(usize::MAX), None);
        assert_eq!(r.remaining(), 0);
    }
}
