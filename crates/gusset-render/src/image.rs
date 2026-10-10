//! RGBA8 images, PNG encode/decode and a tolerant image diff for screenshot tests.

use std::fmt;

/// An 8-bit sRGB RGBA image, rows top to bottom, no padding.
#[derive(Clone, PartialEq, Eq)]
pub struct RgbaImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes.
    pub data: Vec<u8>,
}

impl fmt::Debug for RgbaImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RgbaImage({}x{})", self.width, self.height)
    }
}

/// PNG encode/decode errors.
#[derive(Debug)]
pub enum ImageError {
    /// PNG encoding failed.
    Encode(String),
    /// PNG decoding failed or the PNG is not 8-bit RGB/RGBA.
    Decode(String),
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encode(e) => write!(f, "PNG encoding failed: {e}"),
            Self::Decode(e) => write!(f, "PNG decoding failed: {e}"),
        }
    }
}

impl std::error::Error for ImageError {}

/// Largest image `decode_png` accepts (pixels), to bound memory on hostile input.
pub const MAX_DECODE_PIXELS: u64 = 64 * 1024 * 1024;

impl RgbaImage {
    /// A filled image.
    #[must_use]
    pub fn new(width: u32, height: u32, fill: [u8; 4]) -> RgbaImage {
        let n = width as usize * height as usize;
        RgbaImage { width, height, data: fill.repeat(n) }
    }

    /// Pixel at `(x, y)`.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        let p = self.data.get(i..i + 4)?;
        Some([*p.first()?, *p.get(1)?, *p.get(2)?, *p.get(3)?])
    }

    /// Encode as PNG (8-bit RGBA, sRGB chunk).
    ///
    /// # Errors
    /// The encoder failed (e.g. inconsistent dimensions).
    pub fn to_png(&self) -> Result<Vec<u8>, ImageError> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, self.width, self.height);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
            enc.set_compression(png::Compression::High);
            let mut w = enc.write_header().map_err(|e| ImageError::Encode(e.to_string()))?;
            w.write_image_data(&self.data).map_err(|e| ImageError::Encode(e.to_string()))?;
            w.finish().map_err(|e| ImageError::Encode(e.to_string()))?;
        }
        Ok(out)
    }

    /// Decode an 8-bit RGB or RGBA PNG.
    ///
    /// # Errors
    /// Not a PNG, unsupported format, or larger than [`MAX_DECODE_PIXELS`].
    pub fn from_png(bytes: &[u8]) -> Result<RgbaImage, ImageError> {
        let err = |e: &dyn fmt::Display| ImageError::Decode(e.to_string());
        let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
        dec.set_transformations(png::Transformations::EXPAND);
        let mut reader = dec.read_info().map_err(|e| err(&e))?;
        let (w, h) = (reader.info().width, reader.info().height);
        if u64::from(w) * u64::from(h) > MAX_DECODE_PIXELS {
            return Err(ImageError::Decode(format!("{w}x{h} exceeds the decode limit")));
        }
        let size = reader.output_buffer_size().ok_or_else(|| ImageError::Decode("image too large".into()))?;
        let mut buf = vec![0; size];
        let info = reader.next_frame(&mut buf).map_err(|e| err(&e))?;
        buf.truncate(info.buffer_size());
        if info.bit_depth != png::BitDepth::Eight {
            return Err(ImageError::Decode("only 8-bit PNGs are supported".into()));
        }
        let data = match info.color_type {
            png::ColorType::Rgba => buf,
            png::ColorType::Rgb => buf
                .chunks_exact(3)
                .flat_map(|c| if let [r, g, b] = c { [*r, *g, *b, 255] } else { [0, 0, 0, 255] })
                .collect(),
            other => return Err(ImageError::Decode(format!("unsupported color type {other:?}"))),
        };
        Ok(RgbaImage { width: info.width, height: info.height, data })
    }
}

/// Tolerances for [`diff`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiffTolerance {
    /// A pixel differs when any channel differs by more than this.
    pub per_channel: u8,
    /// The images match when at most this fraction of pixels differ.
    pub max_differing_ratio: f64,
}

impl DiffTolerance {
    /// Default for GPU screenshot tests across drivers: small per-channel noise, and
    /// up to 1 % of pixels (edge rasterization, MSAA and shading-precision
    /// differences between lavapipe, WARP and llvmpipe versions).
    pub const SCREENSHOT: DiffTolerance = DiffTolerance { per_channel: 24, max_differing_ratio: 0.01 };

    /// Exact match.
    pub const EXACT: DiffTolerance = DiffTolerance { per_channel: 0, max_differing_ratio: 0.0 };
}

/// Result of [`diff`].
#[derive(Debug, Clone, PartialEq)]
pub struct DiffReport {
    /// Pixels over the per-channel threshold.
    pub differing: u64,
    /// Total pixels compared.
    pub total: u64,
    /// Largest channel difference seen.
    pub max_channel_delta: u8,
    /// Whether the images are within tolerance (and have the same size).
    pub passed: bool,
    /// A visualization: differing pixels in magenta over a dimmed copy of `actual`.
    pub diff_image: Option<RgbaImage>,
}

impl DiffReport {
    /// Fraction of differing pixels.
    #[must_use]
    pub fn ratio(&self) -> f64 {
        if self.total == 0 { 0.0 } else { self.differing as f64 / self.total as f64 }
    }
}

impl fmt::Display for DiffReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} of {} pixels differ ({:.3} %), max channel delta {}: {}",
            self.differing,
            self.total,
            self.ratio() * 100.0,
            self.max_channel_delta,
            if self.passed { "pass" } else { "FAIL" }
        )
    }
}

/// Compare two images with a tolerance. Different sizes never pass.
#[must_use]
pub fn diff(expected: &RgbaImage, actual: &RgbaImage, tol: DiffTolerance) -> DiffReport {
    if expected.width != actual.width || expected.height != actual.height || expected.data.len() != actual.data.len() {
        return DiffReport {
            differing: u64::from(actual.width) * u64::from(actual.height),
            total: u64::from(actual.width) * u64::from(actual.height),
            max_channel_delta: 255,
            passed: false,
            diff_image: None,
        };
    }
    let mut differing = 0_u64;
    let mut max_delta = 0_u8;
    let mut vis = Vec::with_capacity(actual.data.len());
    for (e, a) in expected.data.chunks_exact(4).zip(actual.data.chunks_exact(4)) {
        let d = e.iter().zip(a).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0);
        max_delta = max_delta.max(d);
        if d > tol.per_channel {
            differing += 1;
            vis.extend_from_slice(&[255, 0, 255, 255]);
        } else {
            vis.extend(a.iter().take(3).map(|c| c / 3));
            vis.push(255);
        }
    }
    let total = (expected.data.len() / 4) as u64;
    let passed = differing as f64 <= tol.max_differing_ratio * total as f64;
    DiffReport {
        differing,
        total,
        max_channel_delta: max_delta,
        passed,
        diff_image: Some(RgbaImage { width: actual.width, height: actual.height, data: vis }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_round_trip() {
        let mut img = RgbaImage::new(5, 3, [10, 20, 30, 255]);
        img.data[4 * 7] = 200;
        let png = img.to_png().expect("encode");
        let back = RgbaImage::from_png(&png).expect("decode");
        assert_eq!(back, img);
        assert_eq!(back.pixel(2, 1), Some([200, 20, 30, 255]));
        assert_eq!(back.pixel(5, 0), None);
        assert!(RgbaImage::from_png(b"not a png").is_err());
    }

    #[test]
    fn diff_tolerances() {
        let a = RgbaImage::new(10, 10, [100, 100, 100, 255]);
        let mut b = a.clone();
        b.data[0] = 110; // within 24
        b.data[4] = 200; // one differing pixel
        let r = diff(&a, &b, DiffTolerance::SCREENSHOT);
        assert_eq!(r.differing, 1);
        assert_eq!(r.max_channel_delta, 100);
        assert!(r.passed, "{r}");
        let r = diff(&a, &b, DiffTolerance { per_channel: 24, max_differing_ratio: 0.0 });
        assert!(!r.passed);
        assert!(diff(&a, &a, DiffTolerance::EXACT).passed);
        assert!(!diff(&a, &RgbaImage::new(10, 9, [0; 4]), DiffTolerance::SCREENSHOT).passed);
    }
}
