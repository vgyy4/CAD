//! 3D mouse (SpaceMouse) input (DESIGN.md §1, UX defaults). On Linux and the BSDs the free
//! `spacenavd` daemon serves the device on a Unix socket; a client reads fixed packets of
//! eight little-endian `i32`: the event type (0 motion, 1 button press, 2 release), then for
//! motion the translation x y z, the rotation rx ry rz and the time since the last motion
//! (ms). A reader thread forwards them; the app turns motion into orbit, pan and zoom each
//! frame. Windows and macOS (the vendor's driver, HID) are not read yet.

use std::sync::mpsc::{Receiver, TryRecvError};

/// One event from the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Translation (x, y, z) and rotation (rx, ry, rz), device units (about ±350 at full
    /// deflection).
    Motion {
        t: [i32; 3],
        r: [i32; 3],
    },
    Button {
        index: i32,
        pressed: bool,
    },
}

/// Parses one 32-byte packet.
#[must_use]
pub fn parse(p: &[u8; 32]) -> Option<Event> {
    let v: Vec<i32> = p.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap_or([0; 4]))).collect();
    match v.as_slice() {
        [0, x, y, z, rx, ry, rz, _] => Some(Event::Motion { t: [*x, *y, *z], r: [*rx, *ry, *rz] }),
        [1, b, ..] => Some(Event::Button { index: *b, pressed: true }),
        [2, b, ..] => Some(Event::Button { index: *b, pressed: false }),
        _ => None,
    }
}

/// One frame's input: summed motion (translation, rotation), if any, and button changes.
pub type Drained = (Option<([i32; 3], [i32; 3])>, Vec<(i32, bool)>);

/// A connection to spacenavd, read on a thread of its own.
#[derive(Debug)]
pub struct SpaceMouse {
    rx: Receiver<Event>,
}

impl SpaceMouse {
    /// Connects to the daemon's socket (`path`, default `/var/run/spnav.sock`); `None` when
    /// there is no daemon (the usual case: no device).
    #[must_use]
    pub fn connect(
        path: Option<&std::path::Path>,
        wake: Option<std::sync::Arc<crate::link::Waker>>,
    ) -> Option<SpaceMouse> {
        #[cfg(unix)]
        {
            use std::io::Read;
            let path =
                path.map_or_else(|| std::path::PathBuf::from("/var/run/spnav.sock"), std::path::Path::to_path_buf);
            let mut s = std::os::unix::net::UnixStream::connect(path).ok()?;
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::Builder::new()
                .name("spacemouse".into())
                .spawn(move || {
                    let mut buf = [0u8; 32];
                    while s.read_exact(&mut buf).is_ok() {
                        if let Some(e) = parse(&buf) {
                            if tx.send(e).is_err() {
                                break;
                            }
                            if let Some(w) = &wake {
                                w.wake();
                            }
                        }
                    }
                })
                .ok()?;
            Some(SpaceMouse { rx })
        }
        #[cfg(not(unix))]
        {
            let _ = (path, wake);
            None
        }
    }

    /// Everything that arrived since the last call, motion summed. `None` once the daemon
    /// went away.
    pub fn drain(&self) -> Option<Drained> {
        let (mut motion, mut buttons) = (None::<([i32; 3], [i32; 3])>, Vec::new());
        loop {
            match self.rx.try_recv() {
                Ok(Event::Motion { t, r }) => {
                    let m = motion.get_or_insert(([0; 3], [0; 3]));
                    for (a, b) in m.0.iter_mut().zip(t) {
                        *a = a.saturating_add(b);
                    }
                    for (a, b) in m.1.iter_mut().zip(r) {
                        *a = a.saturating_add(b);
                    }
                }
                Ok(Event::Button { index, pressed }) => buttons.push((index, pressed)),
                Err(TryRecvError::Empty) => return Some((motion, buttons)),
                // What arrived before the daemon went away still counts; then nothing.
                Err(TryRecvError::Disconnected) => {
                    return (motion.is_some() || !buttons.is_empty()).then_some((motion, buttons));
                }
            }
        }
    }
}

/// Moves `camera` by one frame's summed motion: rotation orbits (ry turns about the up
/// axis, rx tilts), x/y translation pans and z zooms. `viewport_height` in pixels.
pub fn apply(camera: &mut gusset_render::Camera, t: [i32; 3], r: [i32; 3], viewport_height: f64) {
    const ROT: f64 = 1.0 / 20_000.0; // radians per device unit and event
    const PAN: f64 = 1.0 / 25.0; // pixels per device unit and event
    let [tx, ty, tz] = t.map(f64::from);
    let [rx, ry, _] = r.map(f64::from);
    if rx != 0.0 || ry != 0.0 {
        camera.orbit(-ry * ROT, -rx * ROT);
    }
    if tx != 0.0 || ty != 0.0 {
        camera.pan_pixels(tx * PAN, -ty * PAN, viewport_height);
    }
    if tz != 0.0 {
        camera.zoom((tz / 10_000.0).exp());
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    fn packet(v: [i32; 8]) -> [u8; 32] {
        let mut p = [0u8; 32];
        for (c, x) in p.chunks_exact_mut(4).zip(v) {
            c.copy_from_slice(&x.to_le_bytes());
        }
        p
    }

    #[test]
    fn packets_parse() {
        assert_eq!(parse(&packet([0, 1, 2, 3, 4, 5, 6, 16])), Some(Event::Motion { t: [1, 2, 3], r: [4, 5, 6] }));
        assert_eq!(parse(&packet([1, 0, 0, 0, 0, 0, 0, 0])), Some(Event::Button { index: 0, pressed: true }));
        assert_eq!(parse(&packet([2, 1, 0, 0, 0, 0, 0, 0])), Some(Event::Button { index: 1, pressed: false }));
        assert_eq!(parse(&packet([7, 0, 0, 0, 0, 0, 0, 0])), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_daemon_on_a_socket_moves_the_camera() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("gusset-spnav-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join("spnav.sock");
        let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        let server = std::thread::spawn(move || {
            let (mut c, _) = listener.accept().unwrap();
            c.write_all(&packet([0, 0, 0, 0, 0, 300, 0, 16])).unwrap();
            c.write_all(&packet([0, 0, 0, 0, 0, 300, 0, 16])).unwrap();
            c.write_all(&packet([1, 0, 0, 0, 0, 0, 0, 0])).unwrap();
        });
        assert!(SpaceMouse::connect(Some(&dir.join("none.sock")), None).is_none());
        let m = SpaceMouse::connect(Some(&sock), None).unwrap();
        server.join().unwrap();
        // The reader thread may hand the two packets over in separate drains: sum them.
        let mut motion = ([0i32; 3], [0i32; 3]);
        for _ in 0..200 {
            match m.drain() {
                Some((Some((t, r)), _)) => {
                    motion.0.iter_mut().zip(t).for_each(|(a, b)| *a += b);
                    motion.1.iter_mut().zip(r).for_each(|(a, b)| *a += b);
                    if motion.1 == [0, 600, 0] {
                        break;
                    }
                }
                Some(_) => std::thread::sleep(std::time::Duration::from_millis(10)),
                None => break,
            }
        }
        assert_eq!(motion, ([0; 3], [0, 600, 0]));
        // A turn about the up axis changes where the camera looks from.
        let mut cam = gusset_render::Camera::default();
        let before = cam.forward;
        apply(&mut cam, motion.0, motion.1, 600.0);
        assert!(gusset_render::math::length(gusset_render::math::sub(cam.forward, before)) > 1e-3);
        let _ = std::fs::remove_dir_all(dir);
    }
}
