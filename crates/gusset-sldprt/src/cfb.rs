//! A bounded reader for OLE2 compound files ([MS-CFB], the container of SolidWorks files
//! up to about 2014). Only what is needed to list and read streams. Every chain is walked
//! with a step bound (cycle detection), counts and sizes are capped by [`Limits`].

use crate::{Limits, SldprtError};

pub const SIGNATURE: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
const END: u32 = 0xFFFF_FFFE;
const MAX_REGULAR: u32 = 0xFFFF_FFFA;
const DIR_ENTRY: usize = 128;

fn bad(m: &str) -> SldprtError {
    SldprtError::Malformed(format!("OLE2 container: {m}"))
}

fn u16_at(b: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(off..off.checked_add(2)?)?.try_into().ok()?))
}
fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(off..off.checked_add(4)?)?.try_into().ok()?))
}
fn u64_at(b: &[u8], off: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(off..off.checked_add(8)?)?.try_into().ok()?))
}

/// A stream's directory entry.
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    start: u32,
    size: u64,
}

impl Entry {
    /// The stream's size in bytes, as the directory states it.
    pub fn size(&self) -> u64 {
        self.size
    }
}

#[derive(Debug)]
pub struct Cfb<'a> {
    bytes: &'a [u8],
    sector: usize,
    mini_sector: usize,
    cutoff: u64,
    fat: Vec<u32>,
    minifat: Vec<u32>,
    mini_stream: Vec<u8>,
    pub streams: Vec<Entry>,
}

impl<'a> Cfb<'a> {
    pub fn open(bytes: &'a [u8], limits: &Limits) -> Result<Self, SldprtError> {
        if bytes.get(..8) != Some(&SIGNATURE[..]) {
            return Err(bad("signature missing"));
        }
        let shift = u16_at(bytes, 30).ok_or_else(|| bad("short header"))?;
        let mini_shift = u16_at(bytes, 32).ok_or_else(|| bad("short header"))?;
        if !(shift == 9 || shift == 12) || mini_shift != 6 {
            return Err(bad("unsupported sector size"));
        }
        let sector = 1usize << shift;
        let mini_sector = 1usize << mini_shift;
        let nfat = u32_at(bytes, 44).ok_or_else(|| bad("short header"))? as usize;
        let dir_start = u32_at(bytes, 48).ok_or_else(|| bad("short header"))?;
        let cutoff = u64::from(u32_at(bytes, 56).ok_or_else(|| bad("short header"))?);
        let mini_start = u32_at(bytes, 60).ok_or_else(|| bad("short header"))?;
        let difat_start = u32_at(bytes, 68).ok_or_else(|| bad("short header"))?;
        let ndifat = u32_at(bytes, 72).ok_or_else(|| bad("short header"))? as usize;
        let total_sectors = (bytes.len() / sector).saturating_sub(1);
        if nfat == 0 || nfat > total_sectors || ndifat > total_sectors {
            return Err(bad("sector counts exceed the file"));
        }
        let per = sector / 4;
        // DIFAT: 109 entries in the header, then a chain of sectors ending in a link.
        let mut difat: Vec<u32> = (0..109usize).filter_map(|i| u32_at(bytes, 76 + 4 * i)).collect();
        let mut next = difat_start;
        for _ in 0..ndifat {
            if next > MAX_REGULAR {
                break;
            }
            let s = Self::raw(bytes, sector, next).ok_or_else(|| bad("DIFAT sector outside the file"))?;
            difat.extend((0..per.saturating_sub(1)).filter_map(|i| u32_at(s, 4 * i)));
            next = u32_at(s, 4 * per.saturating_sub(1)).unwrap_or(END);
        }
        let mut fat = Vec::new();
        for &f in difat.iter().filter(|&&f| f <= MAX_REGULAR).take(nfat) {
            let s = Self::raw(bytes, sector, f).ok_or_else(|| bad("FAT sector outside the file"))?;
            fat.extend((0..per).filter_map(|i| u32_at(s, 4 * i)));
        }
        let mut me = Cfb {
            bytes,
            sector,
            mini_sector,
            cutoff,
            fat,
            minifat: Vec::new(),
            mini_stream: Vec::new(),
            streams: Vec::new(),
        };
        let dir_cap = limits.max_streams.saturating_mul(DIR_ENTRY);
        let dir = me.chain(dir_start, dir_cap)?;
        if mini_start <= MAX_REGULAR {
            let mf = me.chain(mini_start, limits.max_stream_bytes)?;
            me.minifat = (0..mf.len() / 4).filter_map(|i| u32_at(&mf, 4 * i)).collect();
        }
        let mut entries = dir.chunks_exact(DIR_ENTRY);
        let root = entries.next().ok_or_else(|| bad("empty directory"))?;
        let root_start = u32_at(root, 116).unwrap_or(END);
        if root_start <= MAX_REGULAR {
            me.mini_stream = me.chain(root_start, limits.max_stream_bytes)?;
        }
        for e in entries {
            if e.get(66) != Some(&2) {
                continue;
            }
            let nlen = usize::from(u16_at(e, 64).unwrap_or(0)).min(64);
            let units: Vec<u16> = (0..nlen / 2).filter_map(|i| u16_at(e, 2 * i)).take_while(|&u| u != 0).collect();
            let name = String::from_utf16_lossy(&units);
            let start = u32_at(e, 116).unwrap_or(END);
            // Version 3 files use only the low 32 bits of the size.
            let size = if shift == 9 { u64::from(u32_at(e, 120).unwrap_or(0)) } else { u64_at(e, 120).unwrap_or(0) };
            me.streams.push(Entry { name, start, size });
        }
        Ok(me)
    }

    /// True when the sector table uses sectors past the end of the file (a cut copy).
    pub fn cut_short(&self) -> bool {
        let have = (self.bytes.len() / self.sector).saturating_sub(1);
        self.fat.iter().skip(have).any(|e| *e <= MAX_REGULAR || *e == END)
    }

    fn raw(bytes: &[u8], sector: usize, n: u32) -> Option<&[u8]> {
        let off = usize::try_from(n).ok()?.checked_add(1)?.checked_mul(sector)?;
        bytes.get(off..off.checked_add(sector)?)
    }

    /// Reads a sector chain, at most `cap` bytes; cycles and over-long chains are errors.
    fn chain(&self, start: u32, cap: usize) -> Result<Vec<u8>, SldprtError> {
        let mut out = Vec::new();
        let mut s = start;
        let mut steps = 0usize;
        while s <= MAX_REGULAR {
            steps += 1;
            if steps > self.fat.len() {
                return Err(bad("sector chain loops"));
            }
            if out.len() > cap {
                return Err(SldprtError::TooLarge("a stream is larger than the limit".into()));
            }
            out.extend_from_slice(Self::raw(self.bytes, self.sector, s).ok_or_else(|| bad("sector outside the file"))?);
            s = self.fat.get(s as usize).copied().unwrap_or(END);
        }
        Ok(out)
    }

    fn mini_chain(&self, start: u32, want: usize) -> Result<Vec<u8>, SldprtError> {
        let mut out = Vec::new();
        let mut s = start;
        let mut steps = 0usize;
        while s <= MAX_REGULAR && out.len() < want {
            steps += 1;
            if steps > self.minifat.len() {
                return Err(bad("mini sector chain loops"));
            }
            let off = (s as usize).checked_mul(self.mini_sector).ok_or_else(|| bad("mini sector offset"))?;
            out.extend_from_slice(
                self.mini_stream
                    .get(off..off.checked_add(self.mini_sector).ok_or_else(|| bad("mini sector offset"))?)
                    .ok_or_else(|| bad("mini sector outside the mini stream"))?,
            );
            s = self.minifat.get(s as usize).copied().unwrap_or(END);
        }
        Ok(out)
    }

    /// The bytes of stream `i` (an index into [`Cfb::streams`]).
    pub fn read(&self, i: usize, limits: &Limits) -> Result<Vec<u8>, SldprtError> {
        let e = self.streams.get(i).ok_or_else(|| bad("no such stream"))?;
        let size = usize::try_from(e.size).map_err(|_| SldprtError::TooLarge("stream size".into()))?;
        if size > limits.max_stream_bytes {
            return Err(SldprtError::TooLarge(format!("stream '{}' is {size} bytes", e.name)));
        }
        let mut data = if e.size < self.cutoff { self.mini_chain(e.start, size)? } else { self.chain(e.start, size)? };
        if data.len() < size {
            return Err(bad("a stream is shorter than its size"));
        }
        data.truncate(size);
        Ok(data)
    }
}
