use outfit_core::{RecolorLayer, RgbaImage, recolor_directives};
use std::collections::HashMap;

fn image(width: usize, pixels: &[[u8; 4]]) -> RgbaImage {
    RgbaImage {
        width,
        height: pixels.len() / width,
        rgba: pixels.iter().flatten().copied().collect(),
    }
}

// Independent consumer of the game's simultaneous RGB3/RGBA4/RGB6/RGBA8 map.
fn decode_color(hex: &str) -> [u8; 4] {
    let mut rgba = [0, 0, 0, 255];
    match hex.len() {
        3 | 4 => {
            for (channel, digit) in hex.bytes().enumerate() {
                rgba[channel] = (digit as char).to_digit(16).unwrap() as u8 * 17;
            }
        }
        6 | 8 => {
            for channel in 0..hex.len() / 2 {
                rgba[channel] = u8::from_str_radix(&hex[channel * 2..channel * 2 + 2], 16).unwrap();
            }
        }
        _ => panic!("invalid color spelling {hex}"),
    }
    rgba
}

fn apply(image: &RgbaImage, directives: &str) -> Vec<u8> {
    let mappings: HashMap<_, _> = if directives.is_empty() {
        HashMap::new()
    } else {
        directives
            .strip_prefix("?replace;")
            .unwrap()
            .split(';')
            .map(|pair| {
                let (source, target) = pair.split_once('=').unwrap();
                (decode_color(source), decode_color(target))
            })
            .collect()
    };
    image
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|color| *mappings.get(color).unwrap_or(color))
        .collect()
}

#[test]
fn simultaneous_swaps_and_many_to_one_targets_use_one_replace() {
    let source = image(
        4,
        &[
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255, 0, 0, 255],
        ],
    );
    let target = image(
        4,
        &[
            [0, 255, 0, 255],
            [255, 0, 0, 255],
            [255, 0, 0, 255],
            [0, 255, 0, 255],
        ],
    );
    let directives = recolor_directives(&[RecolorLayer {
        source: &source,
        target: &target,
    }])
    .unwrap();
    assert_eq!(directives, "?replace;f00=0f0;0f0=f00;00f=f00");
    assert_eq!(directives.matches("?replace").count(), 1);
    assert_eq!(apply(&source, &directives), target.rgba);
}

#[test]
fn joint_layers_include_every_pixel_and_allow_different_pair_sizes() {
    let first_source = image(2, &[[17, 0, 34, 0], [18, 52, 86, 1]]);
    let first_target = image(2, &[[34, 51, 68, 17], [254, 253, 252, 128]]);
    let second_source = image(1, &[[17, 0, 34, 0], [17, 34, 51, 255], [9, 8, 7, 0]]);
    let second_target = image(1, &[[34, 51, 68, 17], [18, 52, 86, 255], [9, 8, 7, 0]]);
    let directives = recolor_directives(&[
        RecolorLayer {
            source: &first_source,
            target: &first_target,
        },
        RecolorLayer {
            source: &second_source,
            target: &second_target,
        },
    ])
    .unwrap();
    assert_eq!(
        directives,
        "?replace;1020=2341;12345601=fefdfc80;123=123456"
    );
    assert_eq!(apply(&first_source, &directives), first_target.rgba);
    assert_eq!(apply(&second_source, &directives), second_target.rgba);
    assert_eq!(first_source.rgba, [17, 0, 34, 0, 18, 52, 86, 1]);
    assert!(!directives.contains("09080700"));
}

#[test]
fn transparent_to_opaque_and_opaque_to_transparent_are_not_silently_forbidden() {
    let source = image(2, &[[255, 255, 255, 0], [18, 52, 86, 255]]);
    let target = image(2, &[[255, 255, 255, 255], [18, 52, 86, 0]]);
    let directives = recolor_directives(&[RecolorLayer {
        source: &source,
        target: &target,
    }])
    .unwrap();
    assert_eq!(directives, "?replace;fff0=fff;123456=12345600");
    assert_eq!(apply(&source, &directives), target.rgba);
}

#[test]
fn all_channel_and_alpha_bytes_round_trip_without_color_normalization() {
    let originals = (0..=255u8)
        .map(|channel| [channel, channel.wrapping_mul(17), 255 - channel, channel])
        .collect::<Vec<_>>();
    let edits = originals
        .iter()
        .map(|rgba| [rgba[2], rgba[0], rgba[1], 255 - rgba[3]])
        .collect::<Vec<_>>();
    let source = image(16, &originals);
    let target = image(16, &edits);
    let layers = [RecolorLayer {
        source: &source,
        target: &target,
    }];
    let directives = recolor_directives(&layers).unwrap();
    assert_eq!(apply(&source, &directives), target.rgba);
    assert_eq!(recolor_directives(&layers).unwrap(), directives);
}

#[test]
fn identical_images_return_empty_directives_even_with_hidden_rgb_and_alpha_one() {
    let source = image(3, &[[254, 253, 252, 0], [0, 0, 0, 0], [1, 2, 3, 1]]);
    let directives = recolor_directives(&[RecolorLayer {
        source: &source,
        target: &source,
    }])
    .unwrap();
    assert_eq!(directives, "");
    assert_eq!(apply(&source, &directives), source.rgba);
}

#[test]
fn unchanged_occurrences_constrain_changed_occurrences_and_report_both_locations() {
    let first = image(2, &[[17, 34, 51, 0], [1, 2, 3, 1]]);
    let second = image(1, &[[9, 8, 7, 255], [17, 34, 51, 0]]);
    let edited = image(1, &[[9, 8, 7, 255], [255, 0, 0, 255]]);
    let error = recolor_directives(&[
        RecolorLayer {
            source: &first,
            target: &first,
        },
        RecolorLayer {
            source: &second,
            target: &edited,
        },
    ])
    .unwrap_err()
    .to_string();
    for part in [
        "Layer 2 pixel (0, 1)",
        "layer 1 pixel (0, 0)",
        "11223300",
        "ff0000ff",
        "multiple targets",
    ] {
        assert!(error.contains(part), "{error} lacks {part}");
    }
    let repeated = image(2, &[[1, 2, 3, 1], [1, 2, 3, 1]]);
    let split = image(2, &[[4, 5, 6, 1], [7, 8, 9, 1]]);
    let error = recolor_directives(&[RecolorLayer {
        source: &repeated,
        target: &split,
    }])
    .unwrap_err()
    .to_string();
    assert!(error.contains("Layer 1 pixel (1, 0)"));
    assert!(error.contains("layer 1 pixel (0, 0)"));
    assert!(error.contains("01020301"));
}

#[test]
fn alpha_zero_rgb_is_not_normalized_or_exempted_from_conflicts() {
    let source = image(2, &[[1, 2, 3, 0], [4, 5, 6, 0]]);
    let target = image(2, &[[1, 2, 3, 0], [7, 8, 9, 0]]);
    let directives = recolor_directives(&[RecolorLayer {
        source: &source,
        target: &target,
    }])
    .unwrap();
    assert_eq!(directives, "?replace;04050600=07080900");
    assert_eq!(apply(&source, &directives), target.rgba);
}

#[test]
fn rejects_empty_mismatched_malformed_and_overflowing_buffers_without_panicking() {
    assert!(
        recolor_directives(&[])
            .unwrap_err()
            .to_string()
            .contains("At least one")
    );
    let valid = image(1, &[[0, 0, 0, 0]]);
    for (invalid, message) in [
        (
            RgbaImage {
                width: 0,
                height: 1,
                rgba: vec![],
            },
            "nonzero",
        ),
        (
            RgbaImage {
                width: 1,
                height: 0,
                rgba: vec![],
            },
            "nonzero",
        ),
        (
            RgbaImage {
                width: usize::MAX,
                height: 2,
                rgba: vec![],
            },
            "overflow",
        ),
        (
            RgbaImage {
                width: usize::MAX / 4 + 1,
                height: 1,
                rgba: vec![],
            },
            "overflow",
        ),
        (
            RgbaImage {
                width: 1,
                height: 1,
                rgba: vec![0; 3],
            },
            "exactly 4 bytes, got 3",
        ),
        (
            RgbaImage {
                width: 1,
                height: 1,
                rgba: vec![0; 5],
            },
            "exactly 4 bytes, got 5",
        ),
        (image(2, &[[0; 4], [0; 4]]), "dimensions must match"),
    ] {
        for (source, target, role) in [(&invalid, &valid, "source"), (&valid, &invalid, "target")] {
            let error = recolor_directives(&[RecolorLayer { source, target }])
                .unwrap_err()
                .to_string();
            assert!(error.contains("Layer 1"), "{error}");
            assert!(error.contains(message), "{error}");
            if message != "dimensions must match" {
                assert!(error.contains(role), "{error}");
            }
        }
    }
}
