use outfit_core::*;
use std::collections::HashMap;

fn image(width: usize, height: usize) -> RgbaImage {
    RgbaImage {
        width,
        height,
        rgba: vec![0; width * height * 4],
    }
}

fn paint(image: &mut RgbaImage, x: usize, y: usize, rgba: [u8; 4]) {
    let offset = (y * image.width + x) * 4;
    image.rgba[offset..offset + 4].copy_from_slice(&rgba);
}

fn decode(hex: &str) -> [u8; 4] {
    let expanded: String = if hex.len() <= 4 {
        hex.chars().flat_map(|ch| [ch, ch]).collect()
    } else {
        hex.to_owned()
    };
    let expanded = if expanded.len() == 6 {
        expanded + "ff"
    } else {
        expanded
    };
    u32::from_str_radix(&expanded, 16).unwrap().to_be_bytes()
}

fn replacements(operation: &str) -> HashMap<[u8; 4], [u8; 4]> {
    operation
        .split(';')
        .skip(1)
        .map(|pair| {
            let (source, target) = pair.split_once('=').unwrap();
            (decode(source), decode(target))
        })
        .collect()
}

fn modern_suffix(directives: &str) -> (&str, &str) {
    let sampling = directives.find("?scalenearest=").unwrap() + 1;
    let end = directives[sampling..]
        .find('?')
        .map_or(directives.len(), |offset| sampling + offset);
    directives.split_at(end)
}

fn assert_modern_pixels(source: &RgbaImage, encoding: &Encoding) {
    let geometry = hat_geometry(source).unwrap();
    let (_, suffix) = modern_suffix(&encoding.directives);
    let stages: Vec<_> = suffix
        .split('?')
        .filter(|op| !op.is_empty())
        .map(|op| {
            assert!(op == "replace" || op.starts_with("replace;"));
            replacements(op)
        })
        .collect();
    for y in 0..geometry.height {
        for x in 0..geometry.width {
            let address = [x as u8, 0, y as u8, 0];
            let actual = stages.iter().fold(address, |color, stage| {
                stage.get(&color).copied().unwrap_or(color)
            });
            let offset = ((geometry.offset_y + geometry.height - 1 - y) * source.width
                + geometry.offset_x
                + x)
                * 4;
            let input: [u8; 4] = source.rgba[offset..offset + 4].try_into().unwrap();
            let expected = if input[3] == 0 { address } else { input };
            assert_eq!(actual, expected, "modern ({x},{y})");
        }
    }
    assert_eq!(encoding.output_bytes, encoding.directives.len());
}

#[test]
fn validates_bounds_buffer_and_exact_auto_crop() {
    for (width, height) in [(0, 1), (1, 0), (257, 1), (1, 257), (usize::MAX, 1)] {
        let input = RgbaImage {
            width,
            height,
            rgba: Vec::new(),
        };
        assert!(hat_geometry(&input).is_err());
        assert!(hat_candidates(&input, true, Strategy::Compressed).is_err());
    }
    for length in [3, 5] {
        assert!(
            hat_geometry(&RgbaImage {
                width: 1,
                height: 1,
                rgba: vec![0; length]
            })
            .is_err()
        );
    }
    for (width, height) in [(1, 1), (1, 256), (256, 1), (256, 256), (85, 215), (86, 214)] {
        assert_eq!(
            hat_geometry(&image(width, height)).unwrap(),
            HatGeometry {
                width,
                height,
                offset_x: 0,
                offset_y: 0,
                cropped: false,
            }
        );
    }
    assert_eq!(
        hat_geometry(&image(86, 215)).unwrap(),
        HatGeometry {
            width: 43,
            height: 43,
            offset_x: 43,
            offset_y: 0,
            cropped: true,
        }
    );
}

#[test]
fn modern_scan_orientation_and_exact_rgba() {
    let mut input = image(2, 3);
    paint(&mut input, 0, 2, [18, 52, 86, 1]);
    paint(&mut input, 0, 1, [17, 34, 51, 68]);
    paint(&mut input, 0, 0, [255, 255, 255, 255]);
    paint(&mut input, 1, 2, [19, 87, 203, 128]);
    paint(&mut input, 1, 1, [17, 34, 51, 255]);
    paint(&mut input, 1, 0, [99, 88, 77, 0]);
    let result = hat_candidates(&input, false, Strategy::Text)
        .unwrap()
        .remove(0);
    assert_eq!((result.pixel_count, result.color_count), (5, 5));
    assert_modern_pixels(&input, &result);
}

#[test]
fn modern_boundaries_and_crop_preserve_all_pixel_stages() {
    for (width, height) in [
        (1, 1),
        (1, 256),
        (256, 1),
        (43, 43),
        (85, 85),
        (256, 256),
        (86, 215),
    ] {
        let mut input = image(width, height);
        let geometry = hat_geometry(&input).unwrap();
        for (x, y, rgba) in [
            (0, 0, [255, 255, 255, 1]),
            (geometry.width - 1, geometry.height - 1, [18, 52, 86, 128]),
        ] {
            paint(
                &mut input,
                geometry.offset_x + x,
                geometry.offset_y + y,
                rgba,
            );
        }
        if geometry.cropped {
            paint(&mut input, 0, 0, [255, 0, 0, 255]);
            paint(&mut input, 85, 43, [0, 255, 0, 255]);
        }
        let direct = hat_candidates(&input, false, Strategy::Text)
            .unwrap()
            .remove(0);
        let (prefix, _) = modern_suffix(&direct.directives);
        for strategy in [Strategy::Text, Strategy::Compressed, Strategy::Passes] {
            for encoding in hat_candidates(&input, true, strategy).unwrap() {
                assert!(encoding.directives.starts_with(prefix));
                assert_modern_pixels(&input, &encoding);
                if strategy == Strategy::Passes {
                    assert!(!encoding.palette_applied);
                    assert_eq!(
                        modern_suffix(&encoding.directives)
                            .1
                            .matches("?replace")
                            .count(),
                        1
                    );
                }
            }
        }
    }
}

#[test]
fn modern_palette_avoids_final_color_and_transparent_address_collisions() {
    let mut input = image(43, 3);
    for pixel in input.rgba.as_chunks_mut::<4>().0 {
        *pixel = [19, 87, 203, 128];
    }
    for (x, rgba) in [
        (0, [0, 0, 0, 255]),
        (1, [0, 0, 17, 255]),
        (2, [0, 0, 34, 255]),
        (3, [13, 42, 99, 0]),
        (4, [1, 0, 1, 1]),
    ] {
        paint(&mut input, x, 0, rgba);
    }
    let direct = hat_candidates(&input, false, Strategy::Text)
        .unwrap()
        .remove(0);
    let automatic = hat_candidates(&input, true, Strategy::Text)
        .unwrap()
        .remove(0);
    assert!(automatic.palette_applied);
    assert!(automatic.output_bytes < direct.output_bytes);
    assert_eq!(automatic.baseline_bytes, direct.output_bytes);
    for candidate in hat_candidates(&input, true, Strategy::Compressed).unwrap() {
        assert_modern_pixels(&input, &candidate);
    }
    assert_modern_pixels(&input, &automatic);
    let chosen = optimize_hat_native(&input, true, Strategy::Compressed).unwrap();
    assert_modern_pixels(&input, &chosen.encoding);
}

#[test]
fn modern_palette_exhaustion_never_reuses_a_final_rgb3() {
    let mut input = image(128, 33);
    for (index, rgba) in input.rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        *rgba = if index < 4096 {
            [
                ((index >> 8) as u8) * 17,
                (((index >> 4) & 15) as u8) * 17,
                ((index & 15) as u8) * 17,
                255,
            ]
        } else {
            [19, 87, 203, 128]
        };
    }
    let automatic = hat_candidates(&input, true, Strategy::Text)
        .unwrap()
        .remove(0);
    assert!(!automatic.palette_applied);
    assert_eq!(automatic.output_bytes, automatic.baseline_bytes);
    assert_modern_pixels(&input, &automatic);
}

#[test]
fn empty_canvas_and_low_alpha_colors_preserve_pixel_addresses() {
    for (width, height) in [(1, 1), (256, 1), (1, 256), (256, 256), (86, 215)] {
        let mut input = image(width, height);
        for candidate in hat_candidates(&input, true, Strategy::Compressed).unwrap() {
            assert_eq!((candidate.pixel_count, candidate.color_count), (0, 0));
            assert_modern_pixels(&input, &candidate);
        }
        let geometry = hat_geometry(&input).unwrap();
        paint(
            &mut input,
            geometry.offset_x,
            geometry.offset_y,
            [1, 0, 1, 1],
        );
        if geometry.width > 32 {
            paint(
                &mut input,
                geometry.offset_x + 32,
                geometry.offset_y,
                [13, 25, 37, 255],
            );
        }
        for strategy in [Strategy::Text, Strategy::Compressed, Strategy::Passes] {
            for candidate in hat_candidates(&input, true, strategy).unwrap() {
                assert_modern_pixels(&input, &candidate);
            }
        }
    }
}

#[test]
fn descriptor_fields_metadata_and_chat_escaping_are_independent_of_encoding() {
    let defaults = HatMetadata::default();
    assert_eq!(defaults.shortdescription, "^orange;Custom Hat");
    assert_eq!(defaults.description, "A custom hat created by me!");
    assert_eq!(defaults.rarity, HatRarity::Common);
    assert!(!defaults.remove_hair && !defaults.hide_body);
    let default_item = hat_descriptor("?replace;0000=f00", &defaults);
    assert_eq!(default_item["name"], "eyepatchhead");
    assert_eq!(default_item["parameters"]["mask"], "mask.png");
    for rarity in [
        HatRarity::Common,
        HatRarity::Uncommon,
        HatRarity::Rare,
        HatRarity::Legendary,
        HatRarity::Essential,
    ] {
        assert_eq!(rarity.key().parse::<HatRarity>().unwrap(), rarity);
        for remove_hair in [false, true] {
            for hide_body in [false, true] {
                let metadata = HatMetadata {
                    shortdescription: "quoted \"hat\" \\ path ' 中文".to_owned(),
                    description: "line one\nline two\t\\\"".to_owned(),
                    rarity,
                    remove_hair,
                    hide_body,
                };
                let descriptor = hat_descriptor("?replace", &metadata);
                assert_eq!(
                    descriptor["name"],
                    if hide_body {
                        "frogghead"
                    } else {
                        "eyepatchhead"
                    }
                );
                let parameters = &descriptor["parameters"];
                assert_eq!(parameters["shortdescription"], metadata.shortdescription);
                assert_eq!(parameters["description"], metadata.description);
                assert_eq!(parameters["rarity"], rarity.key());
                assert_eq!(
                    parameters["mask"],
                    if remove_hair {
                        "?submask=/items/armors/decorative/hats/eyepatch/mask.png"
                    } else {
                        "mask.png"
                    }
                );
                assert_eq!(parameters.as_object().unwrap().len(), 12);
                for field in ["femaleFrames", "inventoryIcon", "maleFrames"] {
                    assert_eq!(parameters[field], "head.png");
                }
                assert_eq!(parameters["maxStack"], 1);
                assert_eq!(parameters["price"], 0);
                assert_eq!(parameters["statusEffects"], serde_json::json!([]));
                assert_eq!(parameters["tooltipKind"], "armor");
                let command = spawn_command(&descriptor).unwrap();
                assert!(command.contains("\\'"));
                assert!(command.starts_with(if hide_body {
                    "/spawnitem frogghead 1 '"
                } else {
                    "/spawnitem eyepatchhead 1 '"
                }));
                assert_eq!(
                    descriptor["parameters"]["shortdescription"],
                    metadata.shortdescription
                );
            }
        }
    }
    assert!("mythic".parse::<HatRarity>().is_err());
}
