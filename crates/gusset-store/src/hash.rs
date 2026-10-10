use std::fmt;

/// A BLAKE3 hash (32 bytes). Container records are addressed by the hash of their
/// uncompressed payload; cache entries by the hash of their inputs.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Hash(pub [u8; 32]);

impl Hash {
    /// The BLAKE3 hash of `data`.
    #[must_use]
    pub fn of(data: &[u8]) -> Self {
        Self(*blake3::hash(data).as_bytes())
    }

    /// The raw 32 bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Lowercase hex (64 characters).
    #[must_use]
    pub fn to_hex(&self) -> String {
        crate::wire::to_hex(&self.0)
    }

    /// Parses exactly 64 hex digits (either case). Returns `None` for anything else.
    #[must_use]
    pub fn from_hex(s: &str) -> Option<Self> {
        let mut out = [0u8; 32];
        crate::wire::from_hex_into(s, &mut out).then_some(Self(out))
    }

    /// Builds a hash from a 32-byte slice, or `None` if the length is wrong.
    #[must_use]
    pub fn from_slice(bytes: &[u8]) -> Option<Self> {
        <[u8; 32]>::try_from(bytes).ok().map(Self)
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash({})", self.to_hex())
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let h = Hash::of(b"abc");
        assert_eq!(h.to_hex(), "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85");
        assert_eq!(Hash::from_hex(&h.to_hex()), Some(h));
        assert_eq!(Hash::from_hex(&h.to_hex().to_uppercase()), Some(h));
        assert_eq!(Hash::from_hex("00"), None);
        assert_eq!(Hash::from_hex(&"g".repeat(64)), None);
        assert_eq!(Hash::from_hex(&"é".repeat(32)), None);
    }
}
