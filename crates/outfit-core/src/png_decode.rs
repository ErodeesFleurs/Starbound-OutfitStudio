use crate::{Error, RgbaImage};
use png::{BitDepth, ColorType, Transformations};
use std::io::Cursor;

/// Decode without a browser canvas or premultiplied-alpha intermediate. In
/// particular, an RGBA PNG's RGB bytes survive even when its alpha is zero.
pub fn decode_png(bytes: &[u8]) -> Result<RgbaImage, Error> {
    fn invalid(error: impl std::fmt::Display) -> Error {
        Error::new(format!("Invalid PNG: {error}"))
    }
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(Transformations::EXPAND | Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(invalid)?;
    if reader.info().animation_control.is_some() {
        return Err(Error::new("Expected static PNG, not APNG"));
    }
    let width = usize::try_from(reader.info().width).map_err(invalid)?;
    let height = usize::try_from(reader.info().height).map_err(invalid)?;
    let rgba_size = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| invalid("RGBA dimensions overflow"))?;
    let decoded_size = reader
        .output_buffer_size()
        .ok_or_else(|| invalid("decoded dimensions overflow"))?;
    // Reserve the final RGBA capacity once. Non-RGBA samples expand backwards
    // in this allocation, so no full second image buffer or pixel clone exists.
    let mut rgba = Vec::new();
    rgba.try_reserve_exact(rgba_size).map_err(invalid)?;
    rgba.resize(decoded_size, 0);
    let output = reader.next_frame(&mut rgba).map_err(invalid)?;
    if output.bit_depth != BitDepth::Eight
        || output.width as usize != width
        || output.height as usize != height
    {
        return Err(invalid("unexpected decoded frame format"));
    }
    reader.finish().map_err(invalid)?;
    let channels = match output.color_type {
        ColorType::Rgba => 4,
        ColorType::Rgb => 3,
        ColorType::GrayscaleAlpha => 2,
        ColorType::Grayscale => 1,
        ColorType::Indexed => return Err(invalid("indexed pixels were not expanded")),
    };
    let pixel_count = width
        .checked_mul(height)
        .ok_or_else(|| invalid("pixel count overflow"))?;
    if output.buffer_size() != pixel_count * channels || output.buffer_size() != decoded_size {
        return Err(invalid("decoded buffer size does not match dimensions"));
    }
    if channels != 4 {
        rgba.resize(rgba_size, 0);
        for pixel in (0..pixel_count).rev() {
            let source = pixel * channels;
            let target = pixel * 4;
            let red = rgba[source];
            let (green, blue, alpha) = match channels {
                3 => (rgba[source + 1], rgba[source + 2], 255),
                2 => (red, red, rgba[source + 1]),
                _ => (red, red, 255),
            };
            rgba[target..target + 4].copy_from_slice(&[red, green, blue, alpha]);
        }
    }
    Ok(RgbaImage {
        width,
        height,
        rgba,
    })
}
