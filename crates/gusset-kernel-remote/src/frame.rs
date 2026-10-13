//! Frames on a byte stream: magic, little-endian `u64` length, payload, BLAKE3 of the
//! payload. A reader skips bytes until the next magic, so stray library output on the
//! worker's stdout cannot desynchronise the stream; a corrupted frame fails its hash.

use std::io::{self, Read, Write};

pub(crate) const REQUEST_MAGIC: &[u8; 8] = b"GKRQ1\0\0\0";
pub(crate) const RESPONSE_MAGIC: &[u8; 8] = b"\0GKRS1\0\0";
/// Largest payload accepted (a mesh or an exported file of a very large model).
pub const MAX_FRAME: u64 = 4 << 30;
/// Junk tolerated before a frame's magic (stray output from libraries).
const MAX_SKIP: usize = 1 << 20;

/// Writes one frame and flushes.
pub(crate) fn write_frame(w: &mut dyn Write, magic: &[u8; 8], payload: &[u8]) -> io::Result<()> {
    let len = u64::try_from(payload.len()).map_err(|_| io::Error::other("frame too large"))?;
    w.write_all(magic)?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(payload)?;
    w.write_all(blake3::hash(payload).as_bytes())?;
    w.flush()
}

/// Reads the next frame. `Ok(None)` at a clean end of stream before any byte of a frame.
pub(crate) fn read_frame(r: &mut dyn Read, magic: &[u8; 8]) -> io::Result<Option<Vec<u8>>> {
    // Find the magic, one byte at a time (frames are large; the scan only ever runs over
    // stray output).
    let mut window = [0u8; 8];
    let mut filled = 0usize;
    let mut skipped = 0usize;
    loop {
        let mut b = [0u8; 1];
        if r.read(&mut b)? == 0 {
            return if filled == 0 && skipped == 0 {
                Ok(None)
            } else {
                Err(io::Error::new(io::ErrorKind::UnexpectedEof, "stream ended inside a frame"))
            };
        }
        let [byte] = b;
        if filled < 8 {
            if let Some(slot) = window.get_mut(filled) {
                *slot = byte;
            }
            filled += 1;
        } else {
            window.rotate_left(1);
            if let Some(last) = window.last_mut() {
                *last = byte;
            }
            skipped += 1;
            if skipped > MAX_SKIP {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "no frame in the worker's output"));
            }
        }
        if filled == 8 && &window == magic {
            break;
        }
    }
    let mut len = [0u8; 8];
    r.read_exact(&mut len)?;
    let len = u64::from_le_bytes(len);
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("frame of {len} bytes is too large")));
    }
    let len = usize::try_from(len).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "frame too large"))?;
    let mut payload = Vec::new();
    r.take(len as u64).read_to_end(&mut payload)?;
    if payload.len() != len {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "stream ended inside a frame"));
    }
    let mut hash = [0u8; 32];
    r.read_exact(&mut hash)?;
    if blake3::hash(&payload).as_bytes() != &hash {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame checksum mismatch"));
    }
    Ok(Some(payload))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    #[test]
    fn frames_survive_junk_and_detect_corruption() {
        let mut buf = b"OCCT says hello\n".to_vec();
        write_frame(&mut buf, REQUEST_MAGIC, b"abc").unwrap();
        buf.extend_from_slice(b"more noise");
        write_frame(&mut buf, REQUEST_MAGIC, b"").unwrap();
        let mut r = buf.as_slice();
        assert_eq!(read_frame(&mut r, REQUEST_MAGIC).unwrap().unwrap(), b"abc");
        assert_eq!(read_frame(&mut r, REQUEST_MAGIC).unwrap().unwrap(), b"");
        assert!(read_frame(&mut r, REQUEST_MAGIC).unwrap().is_none());
        // A flipped payload byte fails the hash; a truncated frame is an error.
        let mut bad = Vec::new();
        write_frame(&mut bad, REQUEST_MAGIC, b"payload").unwrap();
        bad[17] ^= 1;
        assert!(read_frame(&mut bad.as_slice(), REQUEST_MAGIC).is_err());
        let mut short = Vec::new();
        write_frame(&mut short, REQUEST_MAGIC, b"payload").unwrap();
        short.truncate(20);
        assert!(read_frame(&mut short.as_slice(), REQUEST_MAGIC).is_err());
        // A huge length is refused before allocating.
        let mut huge = REQUEST_MAGIC.to_vec();
        huge.extend_from_slice(&u64::MAX.to_le_bytes());
        assert!(read_frame(&mut huge.as_slice(), REQUEST_MAGIC).is_err());
    }
}
