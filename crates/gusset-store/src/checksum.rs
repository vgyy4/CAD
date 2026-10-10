//! CRC-32C (Castagnoli), the checksum used by every Gusset storage format.
//!
//! Parameters: reflected polynomial `0x82F63B78` (normal form `0x1EDC6F41`), initial
//! value `0xFFFFFFFF`, final XOR `0xFFFFFFFF`, reflected input and output. This is the
//! same CRC as iSCSI, ext4 metadata and SSE4.2 `crc32`. Check value:
//! `crc32c(b"123456789") == 0xE306_9283`.
//!
//! Implemented here in safe, table-driven code (slicing-by-8, several GB/s) instead of
//! depending on the unmaintained `crc32c` crate, as ADR 0005 allows.

const POLY: u32 = 0x82F6_3B78;

static TABLES: [[u32; 256]; 8] = make_tables();

#[expect(clippy::indexing_slicing, reason = "const-evaluated loops with indices bounded by the array sizes")]
const fn make_tables() -> [[u32; 256]; 8] {
    let mut t = [[0u32; 256]; 8];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { (c >> 1) ^ POLY } else { c >> 1 };
            k += 1;
        }
        t[0][i] = c;
        i += 1;
    }
    let mut i = 0;
    while i < 256 {
        let mut s = 1;
        while s < 8 {
            let prev = t[s - 1][i];
            t[s][i] = (prev >> 8) ^ t[0][(prev & 0xff) as usize];
            s += 1;
        }
        i += 1;
    }
    t
}

#[inline]
#[expect(clippy::indexing_slicing, reason = "a u8 index into a 256-entry table is always in bounds")]
fn tbl(table: usize, byte: u8) -> u32 {
    TABLES[table & 7][usize::from(byte)]
}

/// Computes the CRC-32C of `data`.
#[must_use]
pub fn crc32c(data: &[u8]) -> u32 {
    crc32c_update(0, data)
}

/// Continues a CRC-32C: `crc32c_update(crc32c(a), b) == crc32c(a ++ b)`.
#[must_use]
pub fn crc32c_update(crc: u32, data: &[u8]) -> u32 {
    let mut c = !crc;
    let (blocks, rest) = data.as_chunks::<8>();
    for block in blocks {
        let x = u64::from_le_bytes(*block) ^ u64::from(c);
        let b = x.to_le_bytes();
        c = tbl(7, b[0])
            ^ tbl(6, b[1])
            ^ tbl(5, b[2])
            ^ tbl(4, b[3])
            ^ tbl(3, b[4])
            ^ tbl(2, b[5])
            ^ tbl(1, b[6])
            ^ tbl(0, b[7]);
    }
    for &byte in rest {
        c = tbl(0, (c as u8) ^ byte) ^ (c >> 8);
    }
    !c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bitwise(data: &[u8]) -> u32 {
        let mut c = !0u32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 { (c >> 1) ^ POLY } else { c >> 1 };
            }
        }
        !c
    }

    #[test]
    fn check_value() {
        assert_eq!(crc32c(b"123456789"), 0xE306_9283);
        assert_eq!(crc32c(b""), 0);
        // RFC 3720 B.4: 32 bytes of zeros.
        assert_eq!(crc32c(&[0u8; 32]), 0x8A91_36AA);
        assert_eq!(crc32c(&[0xffu8; 32]), 0x62A8_AB43);
    }

    #[test]
    fn matches_bitwise_reference_and_is_incremental() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
        for len in [0, 1, 7, 8, 9, 15, 16, 17, 63, 64, 65, 999, 1000] {
            let d = &data[..len];
            assert_eq!(crc32c(d), bitwise(d), "len {len}");
            for split in [0, len / 3, len / 2, len] {
                assert_eq!(crc32c_update(crc32c(&d[..split]), &d[split..]), crc32c(d));
            }
        }
    }
}
