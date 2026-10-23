use crate::{Result, fail, packed::PackedAssets, verify};
use outfit_core::{self as core, HatMetadata, HatRarity, RgbaImage, Strategy};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Write;

const HELPER: &str = "/interface/ai/xsmall.png";
const HEADS: [(&str, &str); 2] = [
    (
        "/items/armors/decorative/hats/eyepatch/eyepatch.head",
        "eyepatchhead",
    ),
    (
        "/items/armors/biome/swamp/froggsuit/frogg.head",
        "frogghead",
    ),
];

pub(crate) struct Verification {
    pub cases: Vec<Value>,
    pub files: Vec<(String, Vec<u8>)>,
    pub report: Value,
}
struct Sample {
    name: &'static str,
    image: RgbaImage,
}
// This geometry and expected-pixel model intentionally do not call core hat helpers.
fn geometry(image: &RgbaImage) -> Result<(usize, usize, usize)> {
    if image.width == 0
        || image.height == 0
        || image.width > 256
        || image.height > 256
        || image.rgba.len() != image.width * image.height * 4
    {
        return fail("Invalid independent hat model dimensions/buffer");
    }
    Ok(if (image.width, image.height) == (86, 215) {
        (43, 43, 43)
    } else {
        (image.width, image.height, 0)
    })
}
fn pixel(image: &RgbaImage, x: usize, bottom_y: usize) -> [u8; 4] {
    let offset = ((image.height - 1 - bottom_y) * image.width + x) * 4;
    image.rgba[offset..offset + 4]
        .try_into()
        .expect("validated RGBA image")
}
fn expected(image: &RgbaImage) -> Result<RgbaImage> {
    let (width, height, left) = geometry(image)?;
    let input = |x: usize, y: usize| -> [u8; 4] {
        let offset = ((height - 1 - y) * image.width + left + x) * 4;
        image.rgba[offset..offset + 4]
            .try_into()
            .expect("validated source geometry")
    };
    let mut rgba = Vec::with_capacity(width * height * 4);
    for top_y in 0..height {
        let y = height - 1 - top_y;
        for x in 0..width {
            let mut value = input(x, y);
            if value[3] == 0 {
                value = [x as u8, 0, y as u8, 0];
            }
            rgba.extend_from_slice(&value);
        }
    }
    Ok(RgbaImage {
        width,
        height,
        rgba,
    })
}
#[cfg(test)]
fn prefix(width: usize, height: usize) -> String {
    format!(
        "?setcolor=fff?replace;fff0=fff?crop;0;0;2;2?blendmult={HELPER};0;3?replace;e3e3e3=f000;fff0=00f0;000=f0f0?scale=512;512?crop;1;1;{};{}?scalenearest=0.498046875",
        (512 * width + 127) / 255 + 1,
        (512 * height + 127) / 255 + 1
    )
}
fn replacements(operation: &str) -> Result<HashMap<u32, u32>> {
    let mut fields = operation.split(';');
    if fields.next() != Some("replace") {
        return fail("Expected replacement phase");
    }
    let mut map = HashMap::new();
    for field in fields {
        let (source, target) = field.split_once('=').ok_or("Malformed hat replacement")?;
        if map
            .insert(verify::decode_color(source)?, verify::decode_color(target)?)
            .is_some()
        {
            return fail("Duplicate canonical source in hat replacement phase");
        }
    }
    Ok(map)
}
fn rectangle(operation: &str) -> Result<[usize; 4]> {
    let mut fields = operation
        .strip_prefix("crop;")
        .ok_or("Expected hat crop")?
        .split(';');
    let mut rectangle = [0; 4];
    for value in &mut rectangle {
        *value = fields
            .next()
            .ok_or("Missing hat crop coordinate")?
            .parse()?;
    }
    if fields.next().is_some() {
        return fail("Hat crop must have four coordinates");
    }
    Ok(rectangle)
}

fn scale_factors(value: &str) -> Result<[f32; 2]> {
    let mut fields = value.split(';');
    let x: f32 = fields.next().ok_or("Missing hat scale factor")?.parse()?;
    let y = fields.next().map(str::parse).transpose()?.unwrap_or(x);
    if fields.next().is_some() {
        return fail("Hat scale must have one or two factors");
    }
    let scale = [x, y];
    if scale.iter().any(|s| !s.is_finite() || *s <= 0.0) {
        return fail("Invalid hat scale");
    }
    Ok(scale)
}

// Independently execute the actual helper, replacement and byte-space bilinear
// arithmetic. Do not infer an ideal [x, 0, y, 0] canvas from a matching prefix.
// These captured helper bytes are separately checked against the legal pak.
fn canvas(
    directives: &str,
    width: usize,
    height: usize,
    refined_estimate: bool,
) -> Result<(Vec<u32>, &str)> {
    let mut parts = directives
        .strip_prefix('?')
        .ok_or("Missing hat directive separator")?
        .splitn(9, '?');
    let mut operations = [""; 8];
    for operation in &mut operations {
        *operation = parts.next().ok_or("Incomplete hat canvas operations")?;
    }
    let set = verify::decode_color(
        operations[0]
            .strip_prefix("setcolor=")
            .ok_or("Expected hat setcolor")?,
    )?
    .to_be_bytes();
    let opaque = replacements(operations[1])?;
    if rectangle(operations[2])? != [0, 0, 2, 2] {
        return fail("Independent hat model requires the two-by-two helper seed");
    }
    let mut blend = operations[3]
        .strip_prefix("blendmult=")
        .ok_or("Expected hat blend")?
        .split(';');
    if blend.next() != Some(HELPER)
        || blend.next() != Some("0")
        || blend.next() != Some("3")
        || blend.next().is_some()
    {
        return fail("Independent hat model has no pixels for that helper sample");
    }
    let corner_map = replacements(operations[4])?;
    let helper = [
        [0u8, 0, 0, 0],
        [227, 227, 227, 255],
        [255, 255, 255, 0],
        [0, 0, 0, 255],
    ];
    let mut corners = [[0u8; 4]; 4];
    for (i, helper_pixel) in helper.into_iter().enumerate() {
        // Both alpha values occurring in the two real vanilla bases must seed
        // the same canvas. Test them at every corner rather than assuming that
        // setcolor changes alpha or that hidden RGB is discarded.
        let mut outputs = [[0u8; 4]; 2];
        for (output, alpha) in outputs.iter_mut().zip([0, 255]) {
            let seed = u32::from_be_bytes([set[0], set[1], set[2], alpha]);
            let seed = opaque.get(&seed).copied().unwrap_or(seed).to_be_bytes();
            let blended = u32::from_be_bytes(std::array::from_fn(|channel| {
                ((seed[channel] as f32 / 255.0) * (helper_pixel[channel] as f32 / 255.0) * 255.0)
                    as u8
            }));
            *output = corner_map
                .get(&blended)
                .copied()
                .unwrap_or(blended)
                .to_be_bytes();
        }
        if outputs[0] != outputs[1] {
            return fail("Hat canvas depends on the original base alpha");
        }
        corners[i] = outputs[0];
    }
    let scale = scale_factors(
        operations[5]
            .strip_prefix("scale=")
            .or_else(|| operations[5].strip_prefix("scalebilinear="))
            .ok_or("Expected bilinear hat scale")?,
    )?;
    let crop = rectangle(operations[6])?;
    let cropped_width = crop[2]
        .checked_sub(crop[0])
        .ok_or("Invalid hat crop width")?;
    let cropped_height = crop[3]
        .checked_sub(crop[1])
        .ok_or("Invalid hat crop height")?;
    let nearest = scale_factors(
        operations[7]
            .strip_prefix("scalenearest=")
            .ok_or("Expected nearest hat sampling")?,
    )?;
    if cropped_width == 0
        || cropped_height == 0
        || crop[2] > (2.0 * scale[0]).round() as usize
        || crop[3] > (2.0 * scale[1]).round() as usize
        || (cropped_width as f32 * nearest[0]).round().max(1.0) as usize != width
        || (cropped_height as f32 * nearest[1]).round().max(1.0) as usize != height
    {
        return fail("Hat canvas sampling does not produce the requested geometry");
    }
    // The exact failing OpenSB binary uses RCPPS and one residual correction,
    // not an IEEE divide. A coarse low estimate reproduces its scale=256
    // boundary loss, while ordinary division models the reference path.
    let coordinate = |n: usize, scale: f32| {
        let n = n as f32;
        if refined_estimate {
            let estimate = scale.recip() * (1.0 - 1.0 / 4096.0);
            let quotient = n * estimate;
            quotient + (n - quotient * scale) * estimate
        } else {
            n / scale
        }
    };
    let mut values = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let sx = (coordinate(x, nearest[0]).round() as usize).min(cropped_width - 1) + crop[0];
            let sy = (coordinate(y, nearest[1]).round() as usize).min(cropped_height - 1) + crop[1];
            let px = coordinate(sx, scale[0]);
            let py = coordinate(sy, scale[1]);
            let ix = px.floor() as usize;
            let iy = py.floor() as usize;
            let tx = px - ix as f32;
            let ty = py - iy as f32;
            let fetch = |dx: usize, dy: usize| corners[(iy + dy).min(1) * 2 + (ix + dx).min(1)];
            let [a, b, c, d] = [fetch(0, 0), fetch(1, 0), fetch(0, 1), fetch(1, 1)];
            values.push(u32::from_be_bytes(std::array::from_fn(|channel| {
                let bottom = a[channel] as f32 * (1.0 - tx) + b[channel] as f32 * tx;
                let top = c[channel] as f32 * (1.0 - tx) + d[channel] as f32 * tx;
                (bottom * (1.0 - ty) + top * ty) as u8
            })));
        }
    }
    Ok((values, parts.next().unwrap_or("")))
}

// Execute each replacement phase simultaneously without cascading within a phase.
fn verify_encoding(desired: &RgbaImage, directives: &str) -> Result<usize> {
    let mut phases = 0;
    for refined_estimate in [false, true] {
        let (mut values, operations) =
            canvas(directives, desired.width, desired.height, refined_estimate)?;
        for operation in operations.split('?').filter(|s| !s.is_empty()) {
            if operation.starts_with("replace") {
                let map = replacements(operation)?;
                for value in &mut values {
                    if let Some(target) = map.get(value) {
                        *value = *target;
                    }
                }
                if !refined_estimate {
                    phases += 1;
                }
            } else {
                return fail("Non-replace hat pixel operation");
            }
        }
        for y in 0..desired.height {
            for x in 0..desired.width {
                let want = u32::from_be_bytes(pixel(desired, x, y));
                let actual = values[y * desired.width + x];
                if actual != want {
                    return fail(format!(
                        "Hat pixel ({x},{y}) expected {want:08x}, got {actual:08x}, refinedEstimate={refined_estimate}"
                    ));
                }
            }
        }
    }
    Ok(phases)
}
fn references(pak: &PackedAssets) -> Result<(Vec<Value>, Vec<String>)> {
    let mut report = Vec::new();
    let mut sources = Vec::new();
    for (index, (path, name)) in HEADS.into_iter().enumerate() {
        let item = pak.json(path)?;
        if item["itemName"] != name
            || item["maleFrames"] != "head.png"
            || item["femaleFrames"] != "head.png"
            || item["mask"] != "mask.png"
            || (index == 1 && item["hideBody"] != true)
        {
            return fail(format!("Actual vanilla hat configuration changed: {path}"));
        }
        let base = path.rsplit_once('/').ok_or("Invalid hat path")?.0;
        let image_path = format!("{base}/head.png");
        let image = verify::read_image(pak, &image_path)?;
        if (image.width, image.height) != (86, 215) {
            return fail("Actual hat head sheet must be 86x215");
        }
        if image
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|rgba| !matches!(rgba[3], 0 | 255))
        {
            return fail("Independent hat seed model requires the real bases' binary alpha");
        }
        let mask_path = format!("{base}/mask.png");
        let mask = verify::read_image(pak, &mask_path)?;
        report.push(json!({"name":name,"itemPath":path,"config":item,"path":image_path,
            "width":image.width,"height":image.height,"pngBytes":pak.entries[&image_path.to_lowercase()].size,
            "decodedRgbaBytes":image.rgba.len(),"maskPath":mask_path,"maskWidth":mask.width,"maskHeight":mask.height}));
        sources.push(image_path);
    }
    let helper = verify::read_image(pak, HELPER)?;
    if (helper.width, helper.height) != (7, 8) {
        return fail("Actual xsmall dimensions changed");
    }
    let wanted = [
        [0, 0, 0, 0],
        [227, 227, 227, 255],
        [255, 255, 255, 0],
        [0, 0, 0, 255],
    ];
    for (i, want) in wanted.into_iter().enumerate() {
        if pixel(&helper, i % 2, 3 + i / 2) != want {
            return fail("Actual xsmall bilinear corner prerequisite changed");
        }
    }
    report.push(json!({"name":"xsmall","path":HELPER,"width":helper.width,"height":helper.height,
        "pngBytes":pak.entries[HELPER].size,"decodedRgbaBytes":helper.rgba.len(),
        "sampleBottomLeft":[0,3],
        "pixelsBottomUp":(0..helper.height).map(|y|(0..helper.width).map(|x|pixel(&helper,x,y)).collect::<Vec<_>>()).collect::<Vec<_>>()}));
    Ok((report, sources))
}
fn samples() -> Vec<Sample> {
    let palette = [
        [18, 52, 86, 255],
        [197, 83, 149, 128],
        [13, 29, 47, 1],
        [255, 255, 255, 255],
        [0, 0, 0, 255],
        [0, 0, 17, 255],
        [71, 93, 119, 0],
    ];
    let mut result = Vec::new();
    for (name, width, height, dense) in [
        ("one", 1, 1, true),
        ("thin-row", 85, 1, true),
        ("thin-column", 1, 85, true),
        ("ordinary-43", 43, 43, true),
        ("rectangle-boundaries", 65, 17, true),
        ("boundary-85", 85, 85, true),
        ("maximum-256-corners", 256, 256, false),
        ("maximum-256-dense-palette", 256, 256, true),
        ("automatic-top-right-frame", 86, 215, true),
        ("all-transparent-hidden-rgb", 33, 9, false),
        ("low-alpha-sparse", 65, 9, false),
        ("maximum-256-row", 256, 1, true),
        ("maximum-256-column", 1, 256, true),
        ("maximum-256-edge-strips", 256, 256, false),
        ("maximum-256-transparent", 256, 256, false),
    ] {
        let mut rgba = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                let value = if matches!(
                    name,
                    "all-transparent-hidden-rgb" | "maximum-256-transparent"
                ) {
                    [71, 93, 119, 0]
                } else if name == "one" {
                    [13, 29, 47, 1]
                } else if name == "low-alpha-sparse" {
                    if x == 0 && y == height - 1 {
                        [1, 0, 1, 1]
                    } else if x == 32 && y == height - 1 {
                        [197, 83, 149, 128]
                    } else if x == 64 && y == height - 1 {
                        [18, 52, 86, 255]
                    } else {
                        [71, 93, 119, 0]
                    }
                } else if matches!(name, "maximum-256-row" | "maximum-256-column")
                    || (name == "maximum-256-edge-strips"
                        && (x == 0 || x == width - 1 || y == 0 || y == height - 1))
                {
                    palette[(x + y) % palette.len()]
                } else if dense {
                    palette[(x / 3 + y / 2) % palette.len()]
                } else if (x == 0 || x == width - 1) && (y == 0 || y == height - 1) {
                    palette[usize::from(x != 0) + 2 * usize::from(y != 0)]
                } else {
                    [71, 93, 119, 0]
                };
                rgba.extend_from_slice(&value);
            }
        }
        result.push(Sample {
            name,
            image: RgbaImage {
                width,
                height,
                rgba,
            },
        });
    }
    result
}
fn descriptors(directives: &str) -> Vec<Value> {
    let default = HatMetadata::default();
    [
        default.clone(),
        HatMetadata {
            remove_hair: true,
            ..default.clone()
        },
        HatMetadata {
            hide_body: true,
            ..default.clone()
        },
        HatMetadata {
            shortdescription: "^orange;A \"quoted\" \\ hat\n星".into(),
            description: "It's local: \"RGBA\" \\ alpha=1\nNo pre-escaping".into(),
            rarity: HatRarity::Legendary,
            remove_hair: true,
            hide_body: true,
        },
    ]
    .iter()
    .map(|metadata| core::hat_descriptor(directives, metadata))
    .collect()
}
pub(crate) fn run(pak: &PackedAssets) -> Result<Verification> {
    let (references, source_paths) = references(pak)?;
    let mut cases = Vec::new();
    let mut files = Vec::new();
    let mut measurements = Vec::new();
    let mut verified_candidates = 0usize;
    let mut verified_coordinates = 0usize;
    for sample in samples() {
        let desired = expected(&sample.image)?;
        let mut checked = HashMap::new();
        let mut variants = Vec::new();
        let mut selected = HashSet::new();
        for (name, palette, strategy) in [
            ("direct", false, Strategy::Text),
            ("text", true, Strategy::Text),
            ("compressed", true, Strategy::Compressed),
            ("compressed-no-palette", false, Strategy::Compressed),
            ("passes", true, Strategy::Passes),
        ] {
            let candidates = core::hat_candidates(&sample.image, palette, strategy)?;
            for candidate in &candidates {
                if !checked.contains_key(&candidate.directives) {
                    let phases = verify_encoding(&desired, &candidate.directives)?;
                    checked.insert(candidate.directives.clone(), phases);
                    verified_candidates += 1;
                    verified_coordinates += desired.width * desired.height;
                }
            }
            let result = core::optimize_hat_native(&sample.image, palette, strategy)?;
            let encoding = &result.encoding;
            if !candidates
                .iter()
                .any(|candidate| candidate.directives == encoding.directives)
            {
                return fail("Hat optimizer selected a non-candidate");
            }
            let phases = *checked
                .get(&encoding.directives)
                .ok_or("Unverified selected hat candidate")?;
            if encoding.output_bytes != encoding.directives.len()
                || result.estimated_deflate_bytes
                    != verify::measured_deflate_bytes(&encoding.directives)?
                || ((!palette || strategy == Strategy::Passes) && encoding.palette_applied)
                || (strategy == Strategy::Passes && phases != 1)
            {
                return fail("Hat strategy measurement/palette contract failed");
            }
            measurements.push(json!({"fixture":sample.name,"variant":name,
                "candidateCount":result.candidate_count,"fullDirectiveBytes":encoding.output_bytes,
                "baselineBytes":encoding.baseline_bytes,"estimatedDeflateBytes":result.estimated_deflate_bytes,
                "replacePassCount":phases,"paletteApplied":encoding.palette_applied,
                "pixelCount":encoding.pixel_count,"colorCount":encoding.color_count}));
            if selected.insert(encoding.directives.clone()) {
                variants.push(json!({"name":name,"directives":encoding.directives}));
            }
        }
        let expected_name = format!("hat-expected-{}.png", cases.len());
        let mut colors = BTreeSet::new();
        let mut transparent = Vec::new();
        for y in 0..desired.height {
            for x in 0..desired.width {
                let rgba = pixel(&desired, x, y);
                if rgba[3] != 0 {
                    colors.insert(format!("{:08x}", u32::from_be_bytes(rgba)));
                } else if transparent.len() < 3
                    && !transparent
                        .iter()
                        .any(|probe: &Value| probe["rgba"] == json!(rgba))
                {
                    transparent.push(json!({"x":x,"y":y,"rgba":rgba}));
                }
            }
        }
        // Preserve every original hidden-address probe, then append at most two
        // distinct extreme addresses. OpenSB still compares all RGBA channels.
        let original_probes = transparent.len();
        let edges = [
            (desired.width - 1, desired.height - 1),
            (desired.width - 1, 0),
            (0, desired.height - 1),
            (0, 0),
        ];
        for (x, y) in edges {
            let rgba = pixel(&desired, x, y);
            if rgba[3] == 0
                && !transparent
                    .iter()
                    .any(|probe: &Value| probe["rgba"] == json!(rgba))
            {
                transparent.push(json!({"x":x,"y":y,"rgba":rgba}));
                if transparent.len() == original_probes + 2 {
                    break;
                }
            }
        }
        let direct = variants[0]["directives"]
            .as_str()
            .ok_or("Missing direct hat variant")?;
        cases.push(json!({"name":sample.name,
            "width":desired.width,"height":desired.height,"expectedPath":format!("/{expected_name}"),
            "sourcePaths":source_paths,"colors":colors,"transparent":transparent,
            "descriptors":descriptors(direct),"variants":variants}));
        files.push((expected_name, encode_png(&desired)?));
    }
    let report = json!({"fixtureCount":cases.len(),"verifiedCandidates":verified_candidates,
        "verifiedCoordinates":verified_coordinates,"references":references,"measurements":measurements,
        "evidence":"Independent desired RGBA image model and parsed helper/blend/replacement/byte-bilinear/crop/nearest execution followed by simultaneous replacement for every distinct optimization candidate. Both correctly rounded division and reciprocal-estimate residual refinement are modeled, including nearest source coordinates and output dimensions. Helper corner bytes are checked against the actual pak; this CPU arithmetic model is not a native engine gate. Pure-white RGB and alpha1 are retained; transparent pixels retain canvas address RGB rather than hidden input RGB. No vanilla-binary claim."});
    Ok(Verification {
        cases,
        files,
        report,
    })
}
// Tiny RGBA8 PNG writer for our own synthetic expectations; no commercial bytes are emitted.
pub(crate) fn encode_png(image: &RgbaImage) -> Result<Vec<u8>> {
    let mut rows = Vec::with_capacity(image.rgba.len() + image.height);
    for row in image.rgba.chunks_exact(image.width * 4) {
        rows.push(0);
        rows.extend_from_slice(row);
    }
    let mut compressor = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
    compressor.write_all(&rows)?;
    let compressed = compressor.finish()?;
    let mut output = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&(image.width as u32).to_be_bytes());
    header.extend_from_slice(&(image.height as u32).to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    for (kind, bytes) in [
        (b"IHDR", header.as_slice()),
        (b"IDAT", compressed.as_slice()),
        (b"IEND", &[]),
    ] {
        output.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        output.extend_from_slice(kind);
        output.extend_from_slice(bytes);
        let mut crc = u32::MAX;
        for &byte in kind.iter().chain(bytes) {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
            }
        }
        output.extend_from_slice(&(!crc).to_be_bytes());
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_retains_alpha_one_and_address_rgb_not_hidden_input() {
        let image = RgbaImage {
            width: 2,
            height: 1,
            rgba: vec![13, 29, 47, 1, 71, 93, 119, 0],
        };
        let desired = expected(&image).unwrap();
        assert_eq!(desired.rgba, vec![13, 29, 47, 1, 1, 0, 0, 0]);
        let good = format!("{}?replace;00000000=0d1d2f01", prefix(2, 1));
        verify_encoding(&desired, &good).unwrap();
        assert!(verify_encoding(&desired, &format!("{good}?replace;0d1d2f01=fff")).is_err());
    }
    #[test]
    fn independent_replacements_are_simultaneous_and_detect_cascades() {
        let image = RgbaImage {
            width: 1,
            height: 1,
            rgba: vec![18, 52, 86, 255],
        };
        let direct = format!("{}?replace;00000000=123456;123456=fff", prefix(1, 1));
        verify_encoding(&image, &direct).unwrap();
        assert!(
            verify_encoding(
                &image,
                &direct.replace(";123456=fff", "?replace;123456=fff")
            )
            .is_err()
        );
    }
    #[test]
    fn model_preserves_white_and_low_alpha_samples_without_recoloring() {
        let image = RgbaImage {
            width: 2,
            height: 1,
            rgba: vec![255, 255, 255, 255, 71, 93, 119, 0],
        };
        let desired = expected(&image).unwrap();
        assert_eq!(desired.rgba, vec![255, 255, 255, 255, 1, 0, 0, 0]);
        let sample = samples()
            .into_iter()
            .find(|sample| sample.name == "low-alpha-sparse")
            .unwrap();
        let desired = expected(&sample.image).unwrap();
        assert_eq!(pixel(&desired, 0, 0), [1, 0, 1, 1]);
        assert_eq!(pixel(&desired, 32, 0), [197, 83, 149, 128]);
        assert_eq!(pixel(&desired, 64, 0), [18, 52, 86, 255]);
    }
    #[test]
    fn top_right_crop_and_png_round_trip_preserve_unpremultiplied_rgba() {
        let mut image = RgbaImage {
            width: 86,
            height: 215,
            rgba: vec![0; 86 * 215 * 4],
        };
        image.rgba[43 * 4..44 * 4].copy_from_slice(&[13, 29, 47, 1]);
        let desired = expected(&image).unwrap();
        assert_eq!((desired.width, desired.height), (43, 43));
        assert_eq!(&desired.rgba[..4], &[13, 29, 47, 1]);
        assert_eq!(
            core::decode_png(&encode_png(&desired).unwrap()).unwrap(),
            desired
        );
    }
    #[test]
    fn every_synthetic_strategy_candidate_matches_independent_model() {
        let samples = samples();
        for sample in samples {
            let desired = expected(&sample.image).unwrap();
            let mut checked = HashSet::new();
            for strategy in [Strategy::Text, Strategy::Compressed, Strategy::Passes] {
                for palette in [false, true] {
                    for candidate in core::hat_candidates(&sample.image, palette, strategy).unwrap()
                    {
                        if checked.insert(candidate.directives.clone()) {
                            verify_encoding(&desired, &candidate.directives).unwrap();
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn independent_model_detects_the_approximate_division_endpoint_loss() {
        let image = RgbaImage {
            width: 256,
            height: 256,
            rgba: vec![0; 256 * 256 * 4],
        };
        let desired = expected(&image).unwrap();
        let valid = prefix(256, 256);
        verify_encoding(&desired, &valid).unwrap();
        let seed = valid.split_once("?scale=").unwrap().0;
        let old_sampling = format!("{seed}?scale=256;256?crop;1;1;257;257?scalenearest=1");
        assert_eq!(
            canvas(&old_sampling, 256, 256, false).unwrap().0[255],
            0xff00_0000
        );
        assert_eq!(
            canvas(&old_sampling, 256, 256, true).unwrap().0[255],
            0xfe00_0000
        );
        assert!(verify_encoding(&desired, &old_sampling).is_err());
    }
    #[test]
    fn cli_hat_exports_all_formats_and_keeps_actual_metadata_strings() {
        let directory =
            std::env::temp_dir().join(format!("outfit-hat-export-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let input = directory.join("input.png");
        let image = RgbaImage {
            width: 1,
            height: 1,
            rgba: vec![13, 29, 47, 1],
        };
        std::fs::write(&input, encode_png(&image).unwrap()).unwrap();
        let name = "A \\\"hat\\\" \\\\ 星";
        for format in ["json", "command", "directives"] {
            let out = directory.join(format);
            let args = [
                "--garment",
                "hat",
                "--input",
                input.to_str().unwrap(),
                "--name",
                name,
                "--description",
                "Line 1\nLine 2",
                "--rarity",
                "essential",
                "--remove-hair",
                "--hide-body",
                "--format",
                format,
                "--out",
                out.to_str().unwrap(),
            ]
            .map(str::to_owned);
            crate::encode(&args).unwrap();
            let output = std::fs::read_to_string(out).unwrap();
            let result = core::optimize_hat_native(&image, true, Strategy::Text).unwrap();
            let descriptor = core::hat_descriptor(
                &result.encoding.directives,
                &HatMetadata {
                    shortdescription: name.into(),
                    description: "Line 1\nLine 2".into(),
                    rarity: HatRarity::Essential,
                    remove_hair: true,
                    hide_body: true,
                },
            );
            match format {
                "json" => {
                    assert_eq!(serde_json::from_str::<Value>(&output).unwrap(), descriptor)
                }
                "command" => {
                    assert_eq!(output.trim_end(), core::spawn_command(&descriptor).unwrap())
                }
                _ => assert_eq!(output.trim_end(), result.encoding.directives),
            }
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
