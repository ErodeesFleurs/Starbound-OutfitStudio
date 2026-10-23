use outfit_core::*;
use std::collections::{HashMap, HashSet};

fn sheet(frames: &[&[&str]]) -> (RgbaImage, FrameLayout) {
    let width = frames[0].len() * 43;
    let height = frames.len() * 43;
    (
        RgbaImage {
            width,
            height,
            rgba: vec![0; width * height * 4],
        },
        frames
            .iter()
            .map(|row| row.iter().map(|id| id.to_string()).collect())
            .collect(),
    )
}
fn paint(image: &mut RgbaImage, x: usize, y: usize, rgba: [u8; 4]) {
    let offset = (y * image.width + x) * 4;
    image.rgba[offset..offset + 4].copy_from_slice(&rgba);
}
fn run(image: &RgbaImage, frames: &FrameLayout, palette: bool) -> Encoding {
    encode(&[Layer { image, frames }], "", palette).unwrap()
}
fn decode(hex: &str) -> u32 {
    let expanded = if hex.len() <= 4 {
        hex.chars().flat_map(|c| [c, c]).collect()
    } else {
        hex.to_owned()
    };
    let expanded = if expanded.len() == 6 {
        expanded + "ff"
    } else {
        expanded
    };
    assert_eq!(expanded.len(), 8);
    u32::from_str_radix(&expanded, 16).unwrap()
}
fn render(source: u32, directives: &str) -> u32 {
    let mut value = source;
    for stage in directives.split('?').filter(|stage| !stage.is_empty()) {
        let mut fields = stage.split(';');
        assert_eq!(fields.next(), Some("replace"));
        let table: HashMap<_, _> = fields
            .map(|pair| {
                let (source, target) = pair.split_once('=').unwrap();
                (decode(source), decode(target))
            })
            .collect();
        if let Some(&target) = table.get(&value) {
            value = target;
        }
    }
    value
}
/// Frame-first independent address model. Visible duplicates overwrite in
/// layer order; omitted pixels retain their original hidden coordinate RGB.
fn expected(layers: &[Layer<'_>]) -> HashMap<u32, u32> {
    let mut expected = HashMap::new();
    for layer in layers {
        for (row, cells) in layer.frames.iter().enumerate() {
            for (column, id) in cells.iter().enumerate() {
                if id.is_empty() {
                    continue;
                }
                let id = u32::from_str_radix(id, 16).unwrap();
                for y in 0..43 {
                    for x in 0..43 {
                        let source = ((x as u32) << 24) | (id << 16) | (((42 - y) as u32) << 8);
                        let offset = ((row * 43 + y) * layer.image.width + column * 43 + x) * 4;
                        let pixel = &layer.image.rgba[offset..offset + 4];
                        if pixel[3] == 0 {
                            expected.entry(source).or_insert(source);
                        } else {
                            expected.insert(source, u32::from_be_bytes(pixel.try_into().unwrap()));
                        }
                    }
                }
            }
        }
    }
    expected
}
fn assert_semantics(layers: &[Layer<'_>], directives: &str) {
    // Parse each stage once, independently from the production implementation.
    let tables: Vec<HashMap<u32, u32>> = directives
        .split('?')
        .filter(|s| !s.is_empty())
        .map(|stage| {
            let mut fields = stage.split(';');
            assert_eq!(fields.next(), Some("replace"));
            fields
                .map(|pair| {
                    let (a, b) = pair.split_once('=').unwrap();
                    (decode(a), decode(b))
                })
                .collect()
        })
        .collect();
    for (source, desired) in expected(layers) {
        let actual = tables
            .iter()
            .fold(source, |pixel, table| *table.get(&pixel).unwrap_or(&pixel));
        assert_eq!(actual, desired, "address {source:08x}");
    }
}

#[test]
fn exact_orientation_alpha_and_hex_spelling() {
    let (mut image, frames) = sheet(&[&["a1"]]);
    for (x, y, color) in [
        (0, 42, [255, 0, 0, 255]),
        (1, 42, [0, 255, 0, 255]),
        (0, 41, [0, 0, 255, 128]),
        (2, 42, [17, 34, 51, 68]),
        (3, 42, [18, 52, 86, 255]),
        (4, 42, [254, 253, 252, 0]),
    ] {
        paint(&mut image, x, y, color);
    }
    let result = run(&image, &frames, false);
    assert_eq!(
        result.directives,
        "?replace;00a10100=0000ff80;00a10000=f00;01a10000=0f0;02a10000=1234;03a10000=123456"
    );
    assert_eq!((result.pixel_count, result.color_count), (5, 5));
    assert_semantics(
        &[Layer {
            image: &image,
            frames: &frames,
        }],
        &result.directives,
    );
}

#[test]
fn object_keys_orders_indices_per_layer() {
    let (mut first, first_frames) = sheet(&[&["a1"]]);
    let (mut second, second_frames) = sheet(&[&["11"]]);
    paint(&mut first, 0, 0, [255, 0, 0, 255]);
    paint(&mut second, 0, 0, [0, 255, 0, 255]);
    paint(&mut second, 17, 25, [0, 0, 255, 255]);
    let layers = [
        Layer {
            image: &first,
            frames: &first_frames,
        },
        Layer {
            image: &second,
            frames: &second_frames,
        },
    ];
    let result = encode(&layers, "", false).unwrap();
    assert_eq!(
        result.directives,
        "?replace;00a12a00=f00;1110=00f;00112a00=0f0"
    );
    assert_semantics(&layers, &result.directives);
}

#[test]
fn repeated_ids_count_pixels_but_keep_last_visible_property() {
    let (mut image, frames) = sheet(&[&["11", "11"]]);
    paint(&mut image, 0, 0, [255, 0, 0, 255]);
    paint(&mut image, 43, 0, [0, 255, 0, 255]);
    paint(&mut image, 1, 0, [0, 0, 255, 255]);
    paint(&mut image, 44, 0, [254, 253, 252, 0]);
    let result = run(&image, &frames, false);
    assert_eq!(result.pixel_count, 3);
    assert_eq!(result.color_count, 2);
    assert_eq!(result.directives, "?replace;00112a00=0f0;01112a00=00f");
    assert_semantics(
        &[Layer {
            image: &image,
            frames: &frames,
        }],
        &result.directives,
    );
}

#[test]
fn strict_savings_boundaries_and_utf8_prefix() {
    for count in 0..=5 {
        let (mut image, frames) = sheet(&[&["a1"]]);
        for x in 0..count {
            paint(&mut image, x, 0, [19, 87, 203, 128]);
        }
        let direct = run(&image, &frames, false);
        let result = encode(
            &[Layer {
                image: &image,
                frames: &frames,
            }],
            "衣装🧵",
            true,
        )
        .unwrap();
        assert_eq!(result.palette_applied, count == 5);
        assert_eq!(result.baseline_bytes, "衣装🧵".len() + direct.output_bytes);
        assert_eq!(result.output_bytes, result.directives.len());
        assert_semantics(
            &[Layer {
                image: &image,
                frames: &frames,
            }],
            result.directives.strip_prefix("衣装🧵").unwrap(),
        );
    }
    let (mut image, frames) = sheet(&[&["a1"]]);
    for x in 0..17 {
        paint(&mut image, x, 0, [17, 34, 51, 136]);
    }
    assert_eq!(run(&image, &frames, true), run(&image, &frames, false));
}

#[test]
fn shared_chest_palette_and_final_color_collisions() {
    let sheets = [Sheet::Body, Sheet::FrontSleeves, Sheet::BackSleeves];
    let mut images: Vec<_> = sheets
        .iter()
        .map(|&kind| {
            let frames = layout(kind);
            RgbaImage {
                width: frames[0].len() * 43,
                height: frames.len() * 43,
                rgba: vec![0; frames[0].len() * 43 * frames.len() * 43 * 4],
            }
        })
        .collect();
    for (index, image) in images.iter_mut().enumerate() {
        paint(image, 43, 0, [19, 87, 203, 128]);
        paint(image, 44, 0, [19, 87, 203, 128]);
        assert!(!run(image, layout(sheets[index]), true).palette_applied);
    }
    let layers: Vec<_> = images
        .iter()
        .zip(sheets)
        .map(|(image, sheet)| Layer {
            image,
            frames: layout(sheet),
        })
        .collect();
    let result = encode(&layers, "", true).unwrap();
    assert_eq!(result.factored_color_count, 1);
    assert_eq!(result.pixel_count, 6);
    assert_semantics(&layers, &result.directives);

    let (mut image, frames) = sheet(&[&["a1"]]);
    for x in 0..40 {
        paint(&mut image, x, 0, [18, 52, 86, 128]);
    }
    for (x, color) in [
        (40, [0, 0, 0, 255]),
        (41, [0, 0, 17, 255]),
        (42, [255, 255, 255, 255]),
    ] {
        paint(&mut image, x, 0, color);
    }
    let result = run(&image, &frames, true);
    assert!(result.directives.ends_with("?replace;002=12345680"));
    assert_semantics(
        &[Layer {
            image: &image,
            frames: &frames,
        }],
        &result.directives,
    );
}

#[test]
fn exhausted_and_scarce_alias_capacity_is_lossless_and_ranked() {
    for occupied in [4095, 4096] {
        let (mut image, frames) = sheet(&[&["a1", "a2", "a3"]]);
        for i in 0..occupied {
            let color = [
                ((i >> 8) * 17) as u8,
                (((i >> 4) & 15) * 17) as u8,
                ((i & 15) * 17) as u8,
                255,
            ];
            let width = image.width;
            paint(&mut image, i % width, i / width, color);
        }
        for i in occupied..occupied + 50 {
            let width = image.width;
            paint(
                &mut image,
                i % width,
                i / width,
                if i < occupied + 10 {
                    [18, 52, 86, 128]
                } else {
                    [87, 65, 43, 128]
                },
            );
        }
        let result = run(&image, &frames, true);
        assert_eq!(result.palette_applied, occupied == 4095);
        if occupied == 4095 {
            assert!(result.directives.ends_with("?replace;fff=57412b80"));
        } else {
            assert_eq!(result, run(&image, &frames, false));
        }
        assert_semantics(
            &[Layer {
                image: &image,
                frames: &frames,
            }],
            &result.directives,
        );
    }
}

#[test]
fn empty_cells_and_empty_layers_are_identity() {
    let result = encode(&[], "", true).unwrap();
    assert_eq!(result.directives, "?replace");
    assert_eq!(
        (result.pixel_count, result.color_count, result.output_bytes),
        (0, 0, 8)
    );
    assert_eq!(render(0x11a12a00, &result.directives), 0x11a12a00);
    let (mut image, frames) = sheet(&[&["", "a1"]]);
    paint(&mut image, 0, 0, [255, 255, 255, 255]);
    assert_eq!(run(&image, &frames, true), result);
}

#[test]
fn invalid_consumer_layouts_and_buffers_report_context() {
    let (image, valid) = sheet(&[&["a1"]]);
    for frames in [
        vec![],
        vec![vec![]],
        vec![vec!["a1".into()], vec![]],
        vec![vec!["100".into()]],
        vec![vec!["gg".into()]],
        vec![vec!["é".into()]],
    ] {
        assert!(
            encode(
                &[Layer {
                    image: &image,
                    frames: &frames
                }],
                "",
                true
            )
            .unwrap_err()
            .to_string()
            .starts_with("Layer 1: frame")
        );
    }
    for length in [7392, 7400] {
        let image = RgbaImage {
            width: 43,
            height: 43,
            rgba: vec![0; length],
        };
        assert!(
            encode(
                &[Layer {
                    image: &image,
                    frames: &valid
                }],
                "",
                false
            )
            .unwrap_err()
            .to_string()
            .contains("exactly 7396 bytes")
        );
    }
    let wrong = RgbaImage {
        width: 42,
        height: 43,
        rgba: vec![],
    };
    assert!(
        encode(
            &[Layer {
                image: &wrong,
                frames: &valid
            }],
            "",
            false
        )
        .unwrap_err()
        .to_string()
        .contains("expected a 43 × 43")
    );
    for value in ["unknown", "toString", "__proto__", ""] {
        assert!(value.parse::<Garment>().is_err());
        assert!(value.parse::<Profile>().is_err());
        assert!(value.parse::<Strategy>().is_err());
        assert!(value.parse::<Sheet>().is_err());
    }
}

#[test]
fn compressed_candidates_are_stable_bounded_and_exact() {
    let (mut image, frames) = sheet(&[&["a1", "11"], &["a2", ""]]);
    for y in 0..image.height {
        for x in 0..image.width {
            let colors = [
                [18, 52, 86, 128],
                [17, 34, 51, 136],
                [0, 0, 0, 255],
                [0, 0, 17, 255],
                [255, 255, 255, 255],
                [1, 2, 3, 1],
            ];
            paint(
                &mut image,
                x,
                y,
                if (x + y) % 7 == 0 {
                    [254, 253, 252, 0]
                } else {
                    colors[(x + y * 3) % 6]
                },
            );
        }
    }
    let layers = [Layer {
        image: &image,
        frames: &frames,
    }];
    let prefix = "衣装🧵?replace;deadbeef=0123?replace;0123=4567";
    for palette in [false, true] {
        let alternatives = candidates(&layers, prefix, palette, Strategy::Compressed).unwrap();
        assert!(alternatives.len() <= if palette { 8 } else { 4 });
        assert_eq!(alternatives[0], encode(&layers, prefix, false).unwrap());
        let auto = encode(&layers, prefix, palette).unwrap();
        if auto.palette_applied {
            assert_eq!(alternatives[1], auto);
        }
        let mut unique = HashSet::new();
        for candidate in &alternatives {
            assert!(unique.insert(candidate.directives.as_str()));
            assert_eq!(candidate.output_bytes, candidate.directives.len());
            assert_semantics(&layers, candidate.directives.strip_prefix(prefix).unwrap());
        }
        let text = candidates(&layers, prefix, palette, Strategy::Text).unwrap();
        assert_eq!(text, vec![auto]);
        let passes = candidates(&layers, prefix, palette, Strategy::Passes).unwrap();
        assert_eq!(passes, vec![encode(&layers, prefix, false).unwrap()]);
        let scores: Vec<_> = alternatives
            .iter()
            .enumerate()
            .map(|(index, _)| 1000 - index)
            .collect();
        let expected = alternatives.last().unwrap().clone();
        let chosen = select(alternatives, &scores, Strategy::Compressed).unwrap();
        assert_eq!(chosen.encoding, expected);
        assert_eq!(chosen.candidate_count, scores.len());
    }
}

#[test]
fn duplicate_source_stages_cannot_be_reordered() {
    let (mut first, frames) = sheet(&[&["11", "11"]]);
    let (mut second, second_frames) = sheet(&[&["11"]]);
    for x in 0..43 {
        paint(&mut first, x, 0, [18, 52, 86, 128]);
        paint(&mut first, x + 43, 0, [87, 65, 43, 128]);
    }
    paint(&mut second, 0, 0, [0, 0, 0, 255]);
    paint(&mut second, 17, 25, [0, 255, 0, 255]);
    let layers = [
        Layer {
            image: &first,
            frames: &frames,
        },
        Layer {
            image: &second,
            frames: &second_frames,
        },
    ];
    for palette in [false, true] {
        let bases: Vec<_> = [false, palette]
            .iter()
            .map(|&palette| {
                encode(&layers, "", palette)
                    .unwrap()
                    .directives
                    .split('?')
                    .nth(1)
                    .unwrap()
                    .to_owned()
            })
            .collect();
        for candidate in candidates(&layers, "", palette, Strategy::Compressed).unwrap() {
            assert!(
                bases
                    .iter()
                    .any(|base| base == candidate.directives.split('?').nth(1).unwrap())
            );
            assert_semantics(&layers, &candidate.directives);
        }
    }
}

#[test]
fn selection_ties_scores_lengths_and_errors_are_consumer_visible() {
    let mut first = encode(&[], "first", false).unwrap();
    let mut second = first.clone();
    first.directives = "first".into();
    first.output_bytes = 5;
    second.directives = "other".into();
    second.output_bytes = 5;
    assert_eq!(
        select(
            vec![first.clone(), second.clone()],
            &[10, 10],
            Strategy::Compressed
        )
        .unwrap()
        .encoding,
        first
    );
    second.output_bytes = 4;
    assert_eq!(
        select(
            vec![first.clone(), second.clone()],
            &[10, 10],
            Strategy::Compressed
        )
        .unwrap()
        .encoding,
        second
    );
    assert_eq!(
        select(vec![first.clone(), second], &[9, 10], Strategy::Compressed)
            .unwrap()
            .encoding,
        first
    );
    assert!(
        select(vec![], &[], Strategy::Text)
            .unwrap_err()
            .to_string()
            .contains("at least one")
    );
    assert!(
        select(vec![first], &[], Strategy::Text)
            .unwrap_err()
            .to_string()
            .contains("one DEFLATE estimate per candidate")
    );
}

#[test]
fn native_estimates_are_real_level_six_zlib_sizes() {
    use std::io::Write;
    let (mut image, frames) = sheet(&[&["a1"]]);
    for x in 0..40 {
        paint(&mut image, x, 0, [18, 52, 86, 128]);
    }
    let layers = [Layer {
        image: &image,
        frames: &frames,
    }];
    for strategy in [Strategy::Text, Strategy::Compressed, Strategy::Passes] {
        let alternatives = candidates(&layers, "衣装🧵", true, strategy).unwrap();
        let scores: Vec<_> = alternatives
            .iter()
            .map(|candidate| {
                let mut deflate =
                    flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
                deflate.write_all(candidate.directives.as_bytes()).unwrap();
                deflate.finish().unwrap().len()
            })
            .collect();
        let expected = select(alternatives, &scores, strategy).unwrap();
        assert_eq!(
            optimize_native(&layers, "衣装🧵", true, strategy).unwrap(),
            expected
        );
    }
}

#[test]
fn reordered_candidate_order_and_dedup_are_exact() {
    let (mut image, frames) = sheet(&[&["a1"]]);
    for (x, color) in [
        (0, [0, 255, 0, 255]),
        (1, [255, 0, 0, 255]),
        (2, [0, 0, 255, 255]),
    ] {
        paint(&mut image, x, 0, color);
    }
    let layers = [Layer {
        image: &image,
        frames: &frames,
    }];
    let alternatives = candidates(&layers, "", false, Strategy::Compressed).unwrap();
    assert_eq!(
        alternatives
            .iter()
            .map(|candidate| candidate.directives.as_str())
            .collect::<Vec<_>>(),
        [
            "?replace;00a12a00=0f0;01a12a00=f00;02a12a00=00f",
            "?replace;02a12a00=00f;00a12a00=0f0;01a12a00=f00",
            "?replace;01a12a00=f00;00a12a00=0f0;02a12a00=00f",
        ]
    );
    assert_eq!(
        select(alternatives, &[100, 100, 100], Strategy::Compressed)
            .unwrap()
            .encoding,
        encode(&layers, "", false).unwrap()
    );
    assert_eq!(
        candidates(&[], "衣", true, Strategy::Compressed)
            .unwrap()
            .len(),
        1
    );
}
