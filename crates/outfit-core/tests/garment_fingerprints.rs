use outfit_core::{
    FrameLayout, Garment, Layer, Profile, RgbaImage, Sheet, encode, layout, template,
};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
struct NativeData {
    garments: Vec<NativeGarment>,
}
#[derive(Deserialize)]
struct NativeGarment {
    garment: Garment,
    sources: Vec<NativeSource>,
    fingerprints: Vec<NativeFingerprint>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeSource {
    source_path: String,
    sheet: Sheet,
    column: usize,
    row: usize,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeFingerprint {
    source_path: String,
    vanilla: String,
    opensb: String,
}
fn color(hex: &str) -> [u8; 4] {
    let mut expanded = if hex.len() <= 4 {
        hex.chars().flat_map(|c| [c, c]).collect::<String>()
    } else {
        hex.to_owned()
    };
    if expanded.len() == 6 {
        expanded.push_str("ff");
    }
    u32::from_str_radix(&expanded, 16).unwrap().to_be_bytes()
}
fn mapping(operation: &str) -> HashMap<[u8; 4], [u8; 4]> {
    operation
        .split(';')
        .skip(1)
        .map(|pair| {
            let (from, to) = pair.split_once('=').unwrap();
            (color(from), color(to))
        })
        .collect()
}
fn artwork(frames: &FrameLayout) -> RgbaImage {
    let width = frames.iter().map(Vec::len).max().unwrap() * 43;
    let height = frames.len() * 43;
    let mut image = RgbaImage {
        width,
        height,
        rgba: vec![0; width * height * 4],
    };
    for (row, cells) in frames.iter().enumerate() {
        for (column, id) in cells.iter().enumerate().filter(|(_, id)| !id.is_empty()) {
            let id = u8::from_str_radix(id, 16).unwrap();
            for (x, y, rgba) in [
                (0, 42, [id, 91, 137, 255]),
                (17, 19, [39, id, 201, 128]),
                (42, 0, [17, 23, id, 1]),
                (6, 12, [231, 172, 99, 0]),
            ] {
                let offset = ((row * 43 + y) * width + column * 43 + x) * 4;
                image.rgba[offset..offset + 4].copy_from_slice(&rgba);
            }
        }
    }
    image
}

// Start with the RGBA actually captured immediately before native frame
// recognition. Use the prefix's simultaneous replacement and fixed canvas
// parameters, not the metadata's claimed ID. The helper RGB endpoints were
// independently read from packed.pak: vertical yellow/white, horizontal
// cyan/white. Their alpha is immaterial after the prefix clears it.
fn canvas(seed: [u8; 4], prefix: &str) -> impl Fn(usize, usize) -> [u8; 4] {
    let operations: Vec<_> = prefix.split('?').filter(|s| !s.is_empty()).collect();
    let multiplier = operations
        .iter()
        .find_map(|op| op.strip_prefix("multiply="))
        .unwrap();
    let multiplier = color(multiplier);
    let scale = operations
        .iter()
        .rev()
        .find_map(|op| op.strip_prefix("scale="))
        .unwrap();
    let scale: u32 = scale.parse().unwrap();
    let crop: Vec<usize> = operations
        .last()
        .unwrap()
        .strip_prefix("crop;")
        .unwrap()
        .split(';')
        .map(|n| n.parse().unwrap())
        .collect();
    let corners = [
        [0, seed[1], 0, seed[3]],
        [seed[0], seed[1], 0, seed[3]],
        [0, seed[1], seed[2], seed[3]],
        seed,
    ]
    .map(|rgba| {
        std::array::from_fn::<_, 4, _>(|channel| {
            (u16::from(rgba[channel]) * u16::from(multiplier[channel]) / 255) as u8
        })
    });
    // Exact rational interpolation isolates recognition and address mapping.
    // Native engine gates separately exercise their floating-point arithmetic.
    move |x, y| {
        let px = (x + crop[0]) as u32;
        let py = (y + crop[1]) as u32;
        std::array::from_fn(|channel| {
            let bottom =
                u32::from(corners[0][channel]) * (scale - px) + u32::from(corners[1][channel]) * px;
            let top =
                u32::from(corners[2][channel]) * (scale - px) + u32::from(corners[3][channel]) * px;
            ((bottom * (scale - py) + top * py) / (scale * scale)) as u8
        })
    }
}

#[test]
fn captured_native_samples_reach_existing_addresses_and_real_painted_pixels() {
    let data: NativeData =
        serde_json::from_str(include_str!("../data/garment-fingerprints.json")).unwrap();
    for garment in data.garments {
        let mut sheets = Vec::new();
        for source in &garment.sources {
            if !sheets.contains(&source.sheet) {
                sheets.push(source.sheet);
            }
        }
        let images: Vec<_> = sheets.iter().map(|&sheet| artwork(layout(sheet))).collect();
        let layers: Vec<_> = sheets
            .iter()
            .zip(&images)
            .map(|(&sheet, image)| Layer {
                image,
                frames: layout(sheet),
            })
            .collect();
        for profile in [Profile::Original, Profile::Conservative, Profile::Compact] {
            let prefix = template(garment.garment, profile);
            let recognition = mapping(
                prefix
                    .split('?')
                    .find(|op| op.starts_with("replace;"))
                    .unwrap(),
            );
            for palette in [false, true] {
                let encoded = encode(&layers, prefix, palette).unwrap();
                let paints: Vec<_> = encoded.directives[prefix.len()..]
                    .split('?')
                    .filter(|op| !op.is_empty())
                    .map(mapping)
                    .collect();
                for captured in &garment.fingerprints {
                    let source = garment
                        .sources
                        .iter()
                        .find(|source| source.source_path == captured.source_path)
                        .unwrap();
                    let frames = layout(source.sheet);
                    let id = u8::from_str_radix(&frames[source.row][source.column], 16).unwrap();
                    let image = &images[sheets
                        .iter()
                        .position(|&sheet| sheet == source.sheet)
                        .unwrap()];
                    for native in [&captured.vanilla, &captured.opensb] {
                        let sampled = color(native);
                        let seed = recognition.get(&sampled).copied().unwrap_or(sampled);
                        let address_at = canvas(seed, prefix);
                        for y in 0..43 {
                            for x in 0..43 {
                                let address = address_at(x, y);
                                assert_eq!(
                                    address,
                                    [x as u8, id, y as u8, 0],
                                    "{} sample {native}",
                                    source.source_path
                                );
                                let actual = paints.iter().fold(address, |pixel, map| {
                                    map.get(&pixel).copied().unwrap_or(pixel)
                                });
                                let offset = ((source.row * 43 + 42 - y) * image.width
                                    + source.column * 43
                                    + x)
                                    * 4;
                                let expected: [u8; 4] =
                                    image.rgba[offset..offset + 4].try_into().unwrap();
                                assert_eq!(
                                    actual,
                                    if expected[3] == 0 { address } else { expected },
                                    "{} ({x},{y}) sample {native}",
                                    source.source_path
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn historical_rgba_keys_still_map_to_their_original_address_seeds() {
    let legacy: serde_json::Value =
        serde_json::from_str(include_str!("../data/legacy.json")).unwrap();
    for garment in [
        Garment::Pants,
        Garment::HidingPants,
        Garment::Chest,
        Garment::Back,
    ] {
        let original: String = legacy["templates"][garment.key()]
            .as_array()
            .unwrap()
            .iter()
            .map(|part| part.as_str().unwrap())
            .collect();
        let old = mapping(
            original
                .split('?')
                .find(|op| op.starts_with("replace;"))
                .unwrap(),
        );
        for profile in [Profile::Original, Profile::Conservative, Profile::Compact] {
            let shared = mapping(
                template(garment, profile)
                    .split('?')
                    .find(|op| op.starts_with("replace;"))
                    .unwrap(),
            );
            for (sample, seed) in &old {
                assert_eq!(shared.get(sample), Some(seed));
            }
        }
    }
}

#[test]
fn floran_jump_two_native_red_rounding_still_paints_frame_d2() {
    // Independently captured by the genuine vanilla server and the affected
    // OpenSB package, respectively. The sample differs in RED, not frame ID.
    let samples = [[43, 36, 54, 104], [44, 36, 54, 104]];
    let legacy: serde_json::Value =
        serde_json::from_str(include_str!("../data/legacy.json")).unwrap();
    let old: String = legacy["templates"]["pants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|part| part.as_str().unwrap())
        .collect();
    let old_recognition = mapping(
        old.split('?')
            .find(|op| op.starts_with("replace;"))
            .unwrap(),
    );
    assert_eq!(old_recognition.get(&samples[0]), Some(&[255, 210, 255, 0]));
    assert!(!old_recognition.contains_key(&samples[1]));

    let frames = vec![vec!["d2".to_owned()]];
    let mut image = RgbaImage {
        width: 43,
        height: 43,
        rgba: vec![0; 43 * 43 * 4],
    };
    let offset = 42 * 43 * 4;
    image.rgba[offset..offset + 4].copy_from_slice(&[17, 34, 51, 128]);
    for profile in [Profile::Original, Profile::Conservative, Profile::Compact] {
        let prefix = template(Garment::Pants, profile);
        let recognition = mapping(
            prefix
                .split('?')
                .find(|op| op.starts_with("replace;"))
                .unwrap(),
        );
        let encoded = encode(
            &[Layer {
                image: &image,
                frames: &frames,
            }],
            prefix,
            false,
        )
        .unwrap();
        let paints: Vec<_> = encoded.directives[prefix.len()..]
            .split('?')
            .filter(|op| !op.is_empty())
            .map(mapping)
            .collect();
        for sampled in samples {
            let seed = recognition.get(&sampled).copied().unwrap_or(sampled);
            let address_at = canvas(seed, prefix);
            for (x, y, expected) in [
                (0, 0, [17, 34, 51, 128]),
                (42, 42, [42, 210, 42, 0]),
                (17, 23, [17, 210, 23, 0]),
            ] {
                let address = address_at(x, y);
                assert_eq!(address, [x as u8, 210, y as u8, 0]);
                let actual = paints.iter().fold(address, |pixel, map| {
                    map.get(&pixel).copied().unwrap_or(pixel)
                });
                assert_eq!(actual, expected);
            }
        }
    }
}
