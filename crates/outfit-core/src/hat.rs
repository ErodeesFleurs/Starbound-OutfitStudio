//! Stable Hatter canvas (CC-BY-3.0).
//! The pixel stage preserves decoded PNG RGBA, including alpha 1.
use crate::{
    Encoding, Error, HatGeometry, OptimizedEncoding, RgbaImage, Strategy,
    encoder::{self, ScanBuilder},
    optimizer,
};

/// Validate without copying; vanilla 86 × 215 head sheets select the top-right frame.
pub fn hat_geometry(image: &RgbaImage) -> Result<HatGeometry, Error> {
    if !(1..=256).contains(&image.width) || !(1..=256).contains(&image.height) {
        return Err(Error::new(
            "Hat width and height must each be between 1 and 256 pixels",
        ));
    }
    let expected = image.width * image.height * 4;
    if image.rgba.len() != expected {
        return Err(Error::new(format!(
            "Hat RGBA buffer must contain exactly {expected} bytes"
        )));
    }
    let cropped = image.width == 86 && image.height == 215;
    Ok(HatGeometry {
        width: if cropped { 43 } else { image.width },
        height: if cropped { 43 } else { image.height },
        offset_x: if cropped { 43 } else { 0 },
        offset_y: 0,
        cropped,
    })
}

fn pixel(image: &RgbaImage, geometry: HatGeometry, x: usize, y: usize) -> [u8; 4] {
    // Directives use bottom-left coordinates; decoded PNG rows run top-down.
    let offset =
        ((geometry.offset_y + geometry.height - 1 - y) * image.width + geometry.offset_x + x) * 4;
    image.rgba[offset..offset + 4]
        .try_into()
        .expect("validated RGBA pixel")
}

fn canvas_prefix(geometry: HatGeometry) -> String {
    let (width, height) = (geometry.width, geometry.height);
    // A refined SIMD reciprocal can put scale=256's endpoint just below one.
    // Sample a wider dyadic canvas at 1 + round(512*x/255), then use nearest
    // to retain those samples without another interpolating pass. This keeps
    // every address inside its byte bin, including under FMA contraction.
    // Round the intermediate extent, not the requested final dimensions.
    let sample_width = (512 * width + 127) / 255;
    let sample_height = (512 * height + 127) / 255;
    format!(
        "?setcolor=fff?replace;fff0=fff?crop;0;0;2;2\
         ?blendmult=/interface/ai/xsmall.png;0;3\
         ?replace;e3e3e3=f000;fff0=00f0;000=f0f0\
         ?scale=512;512?crop;1;1;{};{}?scalenearest=0.498046875",
        sample_width + 1,
        sample_height + 1,
    )
}

fn scan(image: &RgbaImage, geometry: HatGeometry) -> encoder::Scan {
    let mut scan = ScanBuilder::new();
    // Deliberately x-major, bottom-up: this is not outfit sheet/Object.keys order.
    for x in 0..geometry.width {
        for y in 0..geometry.height {
            let rgba = pixel(image, geometry, x, y);
            if rgba[3] == 0 {
                continue;
            }
            scan.count_pixels(1);
            scan.pair(
                encoder::Source::Address((x as u32) << 24 | (y as u32) << 8),
                u32::from_be_bytes(rgba),
            );
        }
    }
    scan.finish()
}

pub fn hat_candidates(
    image: &RgbaImage,
    palette: bool,
    strategy: Strategy,
) -> Result<Vec<Encoding>, Error> {
    let geometry = hat_geometry(image)?;
    let prefix = canvas_prefix(geometry);
    let scan = scan(image, geometry);
    Ok(optimizer::scan_candidates(
        &scan, &prefix, palette, strategy,
    ))
}

/// Native complete-string zlib estimates use the same selector as outfit encoding.
pub fn optimize_hat_native(
    image: &RgbaImage,
    palette: bool,
    strategy: Strategy,
) -> Result<OptimizedEncoding, Error> {
    optimizer::optimize_candidates_native(hat_candidates(image, palette, strategy)?, strategy)
}
