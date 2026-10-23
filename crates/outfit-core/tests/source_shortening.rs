use outfit_core::*;
use std::{collections::HashMap, fmt::Write as _, io::Write as _};

fn rgba(hex: &str) -> u32 {
    let mut expanded = if hex.len() <= 4 {
        hex.chars().flat_map(|c| [c, c]).collect::<String>()
    } else {
        hex.to_owned()
    };
    if expanded.len() == 6 {
        expanded.push_str("ff");
    }
    u32::from_str_radix(&expanded, 16).unwrap()
}

fn shortest_len(color: u32) -> usize {
    let bytes = color.to_be_bytes();
    let channels = if bytes[3] == 255 { 3 } else { 4 };
    channels
        * if bytes.iter().all(|byte| byte >> 4 == byte & 15) {
            1
        } else {
            2
        }
}

fn stages<'a>(encoding: &'a Encoding, prefix: &str) -> Vec<Vec<(&'a str, &'a str)>> {
    encoding
        .directives
        .strip_prefix(prefix)
        .unwrap()
        .split('?')
        .filter(|stage| !stage.is_empty())
        .map(|stage| {
            assert_eq!(stage.split(';').next(), Some("replace"));
            stage
                .split(';')
                .skip(1)
                .map(|pair| pair.split_once('=').unwrap())
                .collect()
        })
        .collect()
}

fn assert_pixels(encoding: &Encoding, prefix: &str, expected: &[(u32, u32)]) {
    let stages: Vec<HashMap<_, _>> = stages(encoding, prefix)
        .iter()
        .map(|stage| {
            let mut map = HashMap::new();
            for &(source, target) in stage {
                map.entry(rgba(source)).or_insert(rgba(target));
            }
            map
        })
        .collect();
    for &(address, expected) in expected {
        let actual = stages.iter().fold(address, |color, stage| {
            stage.get(&color).copied().unwrap_or(color)
        });
        assert_eq!(actual, expected, "source RGBA {address:08x}");
    }
    assert_eq!(encoding.output_bytes, encoding.directives.len());
}

fn assert_shortest(encoding: &Encoding, prefix: &str) {
    let mut shortened = 0;
    for stage in stages(encoding, prefix) {
        for (source, target) in stage {
            assert_eq!(source.len(), shortest_len(rgba(source)));
            assert_eq!(target.len(), shortest_len(rgba(target)));
            shortened += usize::from(source.len() == 4);
        }
    }
    assert!(
        shortened > 0,
        "fixture must exercise abbreviated RGBA addresses"
    );
}

// Recreate the previous family independently: only the first-stage address
// spelling changes; aliases, targets, phase boundaries and pair order do not.
fn legacy_family(direct: &Encoding, automatic: &Encoding, prefix: &str) -> Vec<String> {
    let mut bases = Vec::new();
    for encoding in [direct, automatic] {
        let mut base: Vec<Vec<(String, String)>> = stages(encoding, prefix)
            .iter()
            .map(|stage| {
                stage
                    .iter()
                    .map(|&(source, target)| (source.to_owned(), target.to_owned()))
                    .collect()
            })
            .collect();
        for pair in &mut base[0] {
            pair.0 = format!("{:08x}", rgba(&pair.0));
        }
        bases.push(base);
    }
    let mut family = Vec::new();
    for base in bases {
        for order in 0..4 {
            let mut directives = prefix.to_owned();
            for stage in &base {
                let mut pairs = stage.clone();
                match order {
                    1 => pairs.sort_by(|a, b| a.0.cmp(&b.0)),
                    2 => pairs.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0))),
                    3 => pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0))),
                    _ => {}
                }
                directives.push_str("?replace");
                for (source, target) in pairs {
                    write!(directives, ";{source}={target}").unwrap();
                }
            }
            if !family.contains(&directives) {
                family.push(directives);
            }
        }
    }
    family
}

fn deflate(directives: &str) -> usize {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
    encoder.write_all(directives.as_bytes()).unwrap();
    encoder.finish().unwrap().len()
}

fn assert_family(
    prefix: &str,
    direct: &Encoding,
    automatic: &Encoding,
    alternatives: &[Encoding],
    chosen: &OptimizedEncoding,
    expected: &[(u32, u32)],
) {
    assert_shortest(direct, prefix);
    assert_shortest(automatic, prefix);
    let legacy = legacy_family(direct, automatic, prefix);
    for previous in &legacy {
        assert!(
            alternatives
                .iter()
                .any(|candidate| candidate.directives == *previous),
            "lost a previous fixed-eight-digit candidate"
        );
    }
    for candidate in alternatives {
        assert_pixels(candidate, prefix, expected);
    }
    assert!(
        alternatives
            .iter()
            .any(|candidate| candidate.directives == direct.directives)
    );
    assert!(
        alternatives
            .iter()
            .any(|candidate| candidate.directives == automatic.directives)
    );
    assert!((1..=16).contains(&alternatives.len()));
    let best = alternatives
        .iter()
        .map(|candidate| (deflate(&candidate.directives), candidate.output_bytes))
        .min()
        .unwrap();
    assert_eq!(
        (chosen.estimated_deflate_bytes, chosen.encoding.output_bytes),
        best
    );
    assert!(
        best.0
            <= legacy
                .iter()
                .map(|candidate| deflate(candidate))
                .min()
                .unwrap()
    );
    assert_pixels(&chosen.encoding, prefix, expected);
}

fn artwork(x: usize, y: usize) -> [u8; 4] {
    [
        [17, 34, 51, 255],
        [17, 34, 51, 68],
        [18, 52, 86, 255],
        [18, 52, 86, 128],
        [1, 0, 1, 1],
        [99, 88, 77, 0],
    ][(x + 3 * y) % 6]
}

#[test]
fn hat_short_sources_keep_hidden_addresses_alpha_one_and_every_legacy_compression_candidate() {
    for size in [43, 128] {
        let mut image = RgbaImage {
            width: size,
            height: size,
            rgba: vec![0; size * size * 4],
        };
        let mut expected = Vec::new();
        for y in 0..size {
            for x in 0..size {
                let pixel = artwork(x, y);
                let offset = ((size - 1 - y) * size + x) * 4;
                image.rgba[offset..offset + 4].copy_from_slice(&pixel);
                let address = u32::from_be_bytes([x as u8, 0, y as u8, 0]);
                expected.push((
                    address,
                    if pixel[3] == 0 {
                        address
                    } else {
                        u32::from_be_bytes(pixel)
                    },
                ));
            }
        }
        let direct = hat_candidates(&image, false, Strategy::Text)
            .unwrap()
            .remove(0);
        let automatic = hat_candidates(&image, true, Strategy::Text)
            .unwrap()
            .remove(0);
        let (prefix, _) = direct.directives.rsplit_once("?replace;").unwrap();
        for palette in [false, true] {
            let candidates = hat_candidates(&image, palette, Strategy::Compressed).unwrap();
            let chosen = optimize_hat_native(&image, palette, Strategy::Compressed).unwrap();
            assert_family(
                prefix,
                &direct,
                if palette { &automatic } else { &direct },
                &candidates,
                &chosen,
                &expected,
            );
            let passes = hat_candidates(&image, palette, Strategy::Passes)
                .unwrap()
                .remove(0);
            assert_shortest(&passes, prefix);
            assert_pixels(&passes, prefix, &expected);
            assert_eq!(stages(&passes, prefix).len(), 1);
        }
    }
}

#[test]
fn character_frame_ids_use_shortest_exact_sources_without_changing_candidate_phases() {
    let part = CharacterPart::Head;
    let layout = character_layout(part);
    let mut image = RgbaImage {
        width: layout.width,
        height: layout.height,
        rgba: vec![0; layout.width * layout.height * 4],
    };
    let mut expected = Vec::new();
    for frame in &layout.frames {
        for y in 0..43 {
            for x in 0..43 {
                let pixel = if [0, 1, 17, 34, 42].contains(&x) && [0, 1, 17, 34, 42].contains(&y) {
                    artwork(x, y)
                } else {
                    [13, 29, 47, 0]
                };
                let offset = ((frame.y + 42 - y) * image.width + frame.x + x) * 4;
                image.rgba[offset..offset + 4].copy_from_slice(&pixel);
                let address = u32::from_be_bytes([x as u8, frame.id, y as u8, 0]);
                expected.push((
                    address,
                    if pixel[3] == 0 {
                        address
                    } else {
                        u32::from_be_bytes(pixel)
                    },
                ));
            }
        }
    }
    let layers = [CharacterLayer {
        part,
        image: &image,
    }];
    let prefix = character_prefix(part);
    let direct = character_candidates(&layers, false, Strategy::Text)
        .unwrap()
        .remove(0);
    let automatic = character_candidates(&layers, true, Strategy::Text)
        .unwrap()
        .remove(0);
    for palette in [false, true] {
        let candidates = character_candidates(&layers, palette, Strategy::Compressed).unwrap();
        let chosen = optimize_character_native(&layers, palette, Strategy::Compressed).unwrap();
        assert_family(
            prefix,
            &direct,
            if palette { &automatic } else { &direct },
            &candidates,
            &chosen,
            &expected,
        );
        let passes = character_candidates(&layers, palette, Strategy::Passes)
            .unwrap()
            .remove(0);
        assert_shortest(&passes, prefix);
        assert_pixels(&passes, prefix, &expected);
        assert_eq!(stages(&passes, prefix).len(), 1);
    }
}
