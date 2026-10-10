//! Minimal, defensive in-memory ZIP reader and deterministic ZIP writer (stored and
//! deflate entries only), used for 3MF packages.
//!
//! Reader: parses the central directory (including ZIP64 sizes/offsets), validates every
//! entry name, and inflates only requested entries, with per-entry and total
//! decompressed-size budgets, a compression-ratio check (zip-bomb protection) and a
//! CRC-32 check. Nothing is ever written to disk.
//!
//! Writer: fixed timestamps (1980-01-01 00:00), fixed attributes and caller-defined entry
//! order, so identical inputs give byte-identical archives.

use std::io::Write;

use crate::error::{MeshIoError, Result, limit, malformed};
use crate::{Format, Limits};

const SIG_LOCAL: u32 = 0x0403_4b50;
const SIG_CENTRAL: u32 = 0x0201_4b50;
const SIG_EOCD: u32 = 0x0605_4b50;
const SIG_EOCD64: u32 = 0x0606_4b50;
const SIG_EOCD64_LOC: u32 = 0x0706_4b50;

const FORMAT: Format = Format::ThreeMf;

// ---------------------------------------------------------------------------------------
// CRC-32 (IEEE 802.3, reflected 0xEDB88320)

static CRC_TABLE: std::sync::LazyLock<[u32; 256]> = std::sync::LazyLock::new(|| {
    std::array::from_fn(|i| {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        c
    })
});

pub(crate) fn crc32(data: &[u8]) -> u32 {
    let table: &[u32; 256] = &CRC_TABLE;
    let mut c = !0u32;
    for &b in data {
        let idx = ((c ^ u32::from(b)) & 0xff) as usize;
        c = table.get(idx).copied().unwrap_or(0) ^ (c >> 8);
    }
    !c
}

// ---------------------------------------------------------------------------------------
// little-endian helpers

fn u16_at(b: &[u8], off: usize) -> Option<u16> {
    let s = b.get(off..off.checked_add(2)?)?;
    Some(u16::from_le_bytes(s.try_into().ok()?))
}
fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    let s = b.get(off..off.checked_add(4)?)?;
    Some(u32::from_le_bytes(s.try_into().ok()?))
}
fn u64_at(b: &[u8], off: usize) -> Option<u64> {
    let s = b.get(off..off.checked_add(8)?)?;
    Some(u64::from_le_bytes(s.try_into().ok()?))
}

fn bad(detail: impl Into<String>) -> MeshIoError {
    malformed(FORMAT, "ZIP container", detail)
}

// ---------------------------------------------------------------------------------------
// reader

#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub(crate) name: String,
    method: u16,
    flags: u16,
    crc: u32,
    csize: u64,
    usize: u64,
    local_offset: u64,
}

#[derive(Debug)]
pub(crate) struct Archive<'a> {
    data: &'a [u8],
    entries: Vec<Entry>,
}

/// Checks a ZIP entry name: relative, no `..`/`.` segments, no backslashes, drive letters,
/// NUL or control characters.
pub(crate) fn check_entry_name(name: &str) -> Result<()> {
    let unsafe_path = || MeshIoError::UnsafePath { path: name.chars().take(200).collect() };
    if name.is_empty() || name.starts_with('/') || name.contains('\\') || name.contains(':') {
        return Err(unsafe_path());
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(unsafe_path());
    }
    let trimmed = name.strip_suffix('/').unwrap_or(name);
    if trimmed.split('/').any(|seg| seg.is_empty() || seg == "." || seg == "..") {
        return Err(unsafe_path());
    }
    Ok(())
}

impl<'a> Archive<'a> {
    pub(crate) fn parse(data: &'a [u8], limits: &Limits) -> Result<Archive<'a>> {
        if data.len() < 22 {
            return Err(bad("file is too small to be a ZIP archive"));
        }
        // End of central directory: last occurrence within the final 64 KiB + 22 bytes.
        let search_start = data.len().saturating_sub(22 + 0xffff);
        let eocd = (search_start..=data.len() - 22)
            .rev()
            .find(|&i| u32_at(data, i) == Some(SIG_EOCD))
            .ok_or_else(|| bad("end-of-central-directory record not found"))?;
        let disk = u16_at(data, eocd + 4).unwrap_or(0);
        let cd_disk = u16_at(data, eocd + 6).unwrap_or(0);
        if disk != 0 || cd_disk != 0 {
            return Err(MeshIoError::Unsupported { format: FORMAT, detail: "multi-volume ZIP archives".into() });
        }
        let mut count = u64::from(u16_at(data, eocd + 10).unwrap_or(0));
        let mut cd_size = u64::from(u32_at(data, eocd + 12).unwrap_or(0));
        let mut cd_off = u64::from(u32_at(data, eocd + 16).unwrap_or(0));

        if count == 0xffff || cd_size == 0xffff_ffff || cd_off == 0xffff_ffff {
            // ZIP64: locator sits right before the EOCD record.
            let loc = eocd.checked_sub(20).ok_or_else(|| bad("ZIP64 locator missing"))?;
            if u32_at(data, loc) != Some(SIG_EOCD64_LOC) {
                return Err(bad("ZIP64 locator missing"));
            }
            let rec = u64_at(data, loc + 8).ok_or_else(|| bad("ZIP64 locator truncated"))?;
            let rec = usize::try_from(rec).map_err(|_| bad("ZIP64 record offset out of range"))?;
            if u32_at(data, rec) != Some(SIG_EOCD64) {
                return Err(bad("ZIP64 end-of-central-directory record not found"));
            }
            count = u64_at(data, rec + 32).ok_or_else(|| bad("ZIP64 record truncated"))?;
            cd_size = u64_at(data, rec + 40).ok_or_else(|| bad("ZIP64 record truncated"))?;
            cd_off = u64_at(data, rec + 48).ok_or_else(|| bad("ZIP64 record truncated"))?;
        }
        if count > limits.max_zip_entries {
            return Err(limit("ZIP entry count", count, limits.max_zip_entries));
        }
        let cd_start = usize::try_from(cd_off).map_err(|_| bad("central directory offset out of range"))?;
        let cd_end = usize::try_from(cd_size)
            .ok()
            .and_then(|s| cd_start.checked_add(s))
            .filter(|&e| e <= data.len())
            .ok_or_else(|| bad("central directory lies outside the file"))?;
        let cd = data.get(cd_start..cd_end).ok_or_else(|| bad("central directory lies outside the file"))?;

        let mut entries = Vec::with_capacity(count.min(1024) as usize);
        let mut p = 0usize;
        for i in 0..count {
            let at = |what: &str| bad(format!("central directory entry {i} is truncated ({what})"));
            if u32_at(cd, p) != Some(SIG_CENTRAL) {
                return Err(bad(format!("central directory entry {i} has a bad signature")));
            }
            let flags = u16_at(cd, p + 8).ok_or_else(|| at("flags"))?;
            let method = u16_at(cd, p + 10).ok_or_else(|| at("method"))?;
            let crc = u32_at(cd, p + 16).ok_or_else(|| at("crc"))?;
            let mut csize = u64::from(u32_at(cd, p + 20).ok_or_else(|| at("sizes"))?);
            let mut usize_ = u64::from(u32_at(cd, p + 24).ok_or_else(|| at("sizes"))?);
            let nlen = usize::from(u16_at(cd, p + 28).ok_or_else(|| at("name length"))?);
            let xlen = usize::from(u16_at(cd, p + 30).ok_or_else(|| at("extra length"))?);
            let clen = usize::from(u16_at(cd, p + 32).ok_or_else(|| at("comment length"))?);
            let mut local_offset = u64::from(u32_at(cd, p + 42).ok_or_else(|| at("offset"))?);
            let name_bytes = cd.get(p + 46..p + 46 + nlen).ok_or_else(|| at("name"))?;
            let extra = cd.get(p + 46 + nlen..p + 46 + nlen + xlen).ok_or_else(|| at("extra field"))?;
            let name = String::from_utf8_lossy(name_bytes).into_owned();
            check_entry_name(&name)?;

            // ZIP64 extended information: present fields appear in a fixed order.
            let mut x = 0usize;
            while x + 4 <= extra.len() {
                let id = u16_at(extra, x).unwrap_or(0);
                let len = usize::from(u16_at(extra, x + 2).unwrap_or(0));
                let body = extra.get(x + 4..x + 4 + len).ok_or_else(|| at("extra field"))?;
                if id == 1 {
                    let mut q = 0usize;
                    let mut take = |v: &mut u64| -> Result<()> {
                        if *v == 0xffff_ffff {
                            *v = u64_at(body, q).ok_or_else(|| at("ZIP64 field"))?;
                            q += 8;
                        }
                        Ok(())
                    };
                    take(&mut usize_)?;
                    take(&mut csize)?;
                    take(&mut local_offset)?;
                }
                x += 4 + len;
            }

            p = p.checked_add(46 + nlen + xlen + clen).ok_or_else(|| bad("central directory size overflow"))?;
            if name.ends_with('/') {
                continue; // directory entry
            }
            if entries.iter().any(|e: &Entry| e.name.eq_ignore_ascii_case(&name)) {
                return Err(bad(format!("duplicate entry {name:?}")));
            }
            entries.push(Entry { name, method, flags, crc, csize, usize: usize_, local_offset });
        }
        Ok(Archive { data, entries })
    }

    #[cfg(test)]
    pub(crate) fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Case-insensitive lookup (OPC part names are case-insensitive).
    pub(crate) fn find(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name.eq_ignore_ascii_case(name))
    }

    /// Inflates one entry, charging its size against `budget` (remaining decompressed
    /// bytes allowed for this archive).
    pub(crate) fn read(&self, e: &Entry, budget: &mut u64, limits: &Limits) -> Result<Vec<u8>> {
        let ctx = |d: String| bad(format!("entry {:?}: {d}", e.name));
        if e.flags & 1 != 0 {
            return Err(MeshIoError::Unsupported { format: FORMAT, detail: format!("encrypted entry {:?}", e.name) });
        }
        if e.usize > *budget {
            return Err(limit("total decompressed size (bytes)", e.usize, limits.max_decompressed_bytes));
        }
        // Zip-bomb ratio check on the declared sizes.
        if e.usize > 1 << 20 && e.usize / e.csize.max(1) > limits.max_compression_ratio {
            return Err(limit("compression ratio", e.usize / e.csize.max(1), limits.max_compression_ratio));
        }
        let lo = usize::try_from(e.local_offset).map_err(|_| ctx("offset out of range".into()))?;
        if u32_at(self.data, lo) != Some(SIG_LOCAL) {
            return Err(ctx("local header not found".into()));
        }
        let nlen = usize::from(u16_at(self.data, lo + 26).ok_or_else(|| ctx("truncated local header".into()))?);
        let xlen = usize::from(u16_at(self.data, lo + 28).ok_or_else(|| ctx("truncated local header".into()))?);
        let start = lo.checked_add(30 + nlen + xlen).ok_or_else(|| ctx("offset overflow".into()))?;
        let end = usize::try_from(e.csize)
            .ok()
            .and_then(|c| start.checked_add(c))
            .ok_or_else(|| ctx("size overflow".into()))?;
        let raw = self.data.get(start..end).ok_or_else(|| ctx("compressed data is truncated".into()))?;
        let want = usize::try_from(e.usize).map_err(|_| ctx("size out of range".into()))?;
        let out = match e.method {
            0 => {
                if raw.len() != want {
                    return Err(ctx("stored size mismatch".into()));
                }
                raw.to_vec()
            }
            8 => {
                // Never inflate more than the declared size (+1 to detect lies).
                let out =
                    miniz_oxide::inflate::decompress_to_vec_with_limit(raw, want.saturating_add(1)).map_err(|err| {
                        match err.status {
                            miniz_oxide::inflate::TINFLStatus::HasMoreOutput => {
                                ctx("inflates to more than its declared size (possible zip bomb)".into())
                            }
                            _ => ctx("corrupt deflate stream".into()),
                        }
                    })?;
                if out.len() != want {
                    return Err(ctx(format!("inflated to {} bytes but declares {want}", out.len())));
                }
                out
            }
            m => {
                return Err(MeshIoError::Unsupported {
                    format: FORMAT,
                    detail: format!("compression method {m} in entry {:?} (only stored and deflate)", e.name),
                });
            }
        };
        if crc32(&out) != e.crc {
            return Err(ctx("CRC-32 mismatch (corrupt data)".into()));
        }
        *budget -= e.usize;
        Ok(out)
    }
}

// ---------------------------------------------------------------------------------------
// writer

/// Writes a deterministic ZIP archive with `entries` in the given order. Each entry is
/// deflated (level 6) unless storing is smaller.
pub(crate) fn write_zip(entries: &[(&str, &[u8])], out: &mut dyn Write) -> Result<()> {
    const DOS_TIME: u16 = 0; // 00:00:00
    const DOS_DATE: u16 = (1 << 5) | 1; // 1980-01-01
    let too_big = || MeshIoError::Unsupported { format: FORMAT, detail: "3MF parts larger than 4 GiB".into() };

    let mut central = Vec::new();
    let mut offset: u64 = 0;
    for (name, data) in entries {
        let crc = crc32(data);
        let deflated = miniz_oxide::deflate::compress_to_vec(data, 6);
        let (method, payload): (u16, &[u8]) = if deflated.len() < data.len() { (8, &deflated) } else { (0, data) };
        let usize_ = u32::try_from(data.len()).map_err(|_| too_big())?;
        let csize = u32::try_from(payload.len()).map_err(|_| too_big())?;
        let off32 = u32::try_from(offset).map_err(|_| too_big())?;
        let nlen = u16::try_from(name.len()).map_err(|_| too_big())?;

        let mut local = Vec::with_capacity(30 + name.len());
        local.extend_from_slice(&SIG_LOCAL.to_le_bytes());
        local.extend_from_slice(&20u16.to_le_bytes()); // version needed
        local.extend_from_slice(&0u16.to_le_bytes()); // flags
        local.extend_from_slice(&method.to_le_bytes());
        local.extend_from_slice(&DOS_TIME.to_le_bytes());
        local.extend_from_slice(&DOS_DATE.to_le_bytes());
        local.extend_from_slice(&crc.to_le_bytes());
        local.extend_from_slice(&csize.to_le_bytes());
        local.extend_from_slice(&usize_.to_le_bytes());
        local.extend_from_slice(&nlen.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes()); // extra length
        local.extend_from_slice(name.as_bytes());
        out.write_all(&local)?;
        out.write_all(payload)?;

        central.extend_from_slice(&SIG_CENTRAL.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // version made by (MS-DOS, 2.0)
        central.extend_from_slice(&20u16.to_le_bytes()); // version needed
        central.extend_from_slice(&0u16.to_le_bytes()); // flags
        central.extend_from_slice(&method.to_le_bytes());
        central.extend_from_slice(&DOS_TIME.to_le_bytes());
        central.extend_from_slice(&DOS_DATE.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&csize.to_le_bytes());
        central.extend_from_slice(&usize_.to_le_bytes());
        central.extend_from_slice(&nlen.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra
        central.extend_from_slice(&0u16.to_le_bytes()); // comment
        central.extend_from_slice(&0u16.to_le_bytes()); // disk
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        central.extend_from_slice(&off32.to_le_bytes());
        central.extend_from_slice(name.as_bytes());

        offset += (local.len() + payload.len()) as u64;
    }
    let n = u16::try_from(entries.len()).map_err(|_| too_big())?;
    let cd_size = u32::try_from(central.len()).map_err(|_| too_big())?;
    let cd_off = u32::try_from(offset).map_err(|_| too_big())?;
    out.write_all(&central)?;
    let mut eocd = Vec::with_capacity(22);
    eocd.extend_from_slice(&SIG_EOCD.to_le_bytes());
    eocd.extend_from_slice(&0u16.to_le_bytes());
    eocd.extend_from_slice(&0u16.to_le_bytes());
    eocd.extend_from_slice(&n.to_le_bytes());
    eocd.extend_from_slice(&n.to_le_bytes());
    eocd.extend_from_slice(&cd_size.to_le_bytes());
    eocd.extend_from_slice(&cd_off.to_le_bytes());
    eocd.extend_from_slice(&0u16.to_le_bytes()); // comment length
    out.write_all(&eocd)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_known_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn roundtrip_and_names() {
        let mut buf = Vec::new();
        let big = vec![b'a'; 10_000];
        write_zip(&[("a.txt", b"hello"), ("dir/b.bin", &big)], &mut buf).unwrap();
        let lim = Limits::default();
        let ar = Archive::parse(&buf, &lim).unwrap();
        assert_eq!(ar.entries().len(), 2);
        let mut budget = lim.max_decompressed_bytes;
        let e = ar.find("DIR/B.BIN").unwrap().clone();
        assert_eq!(ar.read(&e, &mut budget, &lim).unwrap(), big);
        assert_eq!(budget, lim.max_decompressed_bytes - 10_000);
    }

    #[test]
    fn rejects_bad_names() {
        for n in ["/abs", "../x", "a/../b", "a\\b", "C:/x", "a//b", "./a", ""] {
            assert!(check_entry_name(n).is_err(), "{n}");
        }
        check_entry_name("3D/3dmodel.model").unwrap();
        check_entry_name("_rels/").unwrap();
    }
}
