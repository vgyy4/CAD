//! CRC-32 (IEEE) as used by the stream records of the chunk container.

use std::sync::LazyLock;

static TABLE: LazyLock<[u32; 256]> = LazyLock::new(|| {
    let mut t = [0u32; 256];
    for (i, slot) in (0u32..).zip(t.iter_mut()) {
        let mut c = i;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    t
});

pub(crate) fn crc32(data: &[u8]) -> u32 {
    let table: &[u32; 256] = &TABLE;
    let mut c = !0u32;
    for &b in data {
        let idx = usize::from(((c ^ u32::from(b)) & 0xff) as u8);
        c = table.get(idx).copied().unwrap_or(0) ^ (c >> 8);
    }
    !c
}
