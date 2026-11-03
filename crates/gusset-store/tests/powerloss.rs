//! Power loss on a real block device (ADR 0007, review M14). CI runs these on ext4 over a
//! dm-flakey device (scripts/powerloss.sh): `write` saves generation after generation with
//! `durable_write` and prints each acknowledged one; the script switches the device to
//! drop every later write (what the disk keeps when the power goes), stops the writer and
//! remounts; `check` then requires a whole file of the last generation acknowledged before
//! the switch, or a newer one. Without `GUSSET_POWERLOSS_DIR` both do nothing.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use gusset_store::checksum::crc32c;
use gusset_store::durable::durable_write;
use gusset_store::fs::{Fs, RealFs};
use std::io::Write as _;
use std::path::PathBuf;

const FILE: &str = "project.gus";
/// Payload size: several pages, so a torn write shows.
const PAYLOAD: usize = 64 * 1024;

fn dir() -> Option<PathBuf> {
    std::env::var_os("GUSSET_POWERLOSS_DIR").map(PathBuf::from)
}

/// Generation `g`: `g`, a payload derived from it, and a CRC over both.
fn image(g: u64) -> Vec<u8> {
    let mut v = g.to_le_bytes().to_vec();
    let mut x = g.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    for _ in 0..PAYLOAD {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        v.push(x as u8);
    }
    let crc = crc32c(&v);
    v.extend_from_slice(&crc.to_le_bytes());
    v
}

/// The generation of a whole image, or why it is not one.
fn decode(b: &[u8]) -> Result<u64, String> {
    if b.len() != 8 + PAYLOAD + 4 {
        return Err(format!("{} bytes", b.len()));
    }
    let (body, crc) = b.split_at(b.len() - 4);
    if crc32c(body).to_le_bytes() != crc {
        return Err("checksum mismatch".into());
    }
    let g = u64::from_le_bytes(body[..8].try_into().unwrap());
    if image(g) != b {
        return Err(format!("generation {g} has the wrong payload"));
    }
    Ok(g)
}

#[test]
#[ignore = "run by scripts/powerloss.sh on a dm-flakey device"]
fn write() {
    let Some(dir) = dir() else { return };
    let fs = RealFs::private();
    let path = dir.join(FILE);
    let start = fs.read(&path).ok().and_then(|b| decode(&b).ok()).map_or(1, |g| g + 1);
    let mut out = std::io::stdout();
    for g in start..start + 1_000_000 {
        let check = |b: &[u8]| decode(b).map(|_| ());
        if durable_write(&fs, &path, &image(g), &check).is_err() {
            // The device went away under us (the script stops us right after).
            break;
        }
        writeln!(out, "{g}").unwrap();
        out.flush().unwrap();
    }
}

#[test]
#[ignore = "run by scripts/powerloss.sh on a dm-flakey device"]
fn check() {
    let Some(dir) = dir() else { return };
    let min: u64 = std::env::var("GUSSET_POWERLOSS_MIN").ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    let bytes = std::fs::read(dir.join(FILE)).unwrap_or_else(|e| panic!("the project file is gone: {e}"));
    let g = decode(&bytes).unwrap_or_else(|e| panic!("the project file is damaged: {e}"));
    assert!(g >= min, "generation {g} is older than the last acknowledged save {min}");
    println!("ok: generation {g} (acknowledged {min})");
}

#[test]
fn images_round_trip_and_damage_is_found() {
    let a = image(7);
    assert_eq!(decode(&a), Ok(7));
    let mut b = a.clone();
    b[100] ^= 1;
    assert!(decode(&b).is_err());
    assert!(decode(&a[..a.len() - 1]).is_err());
}
