use outfit_core::*;
use serde_json::{Value, json};
use std::collections::HashMap;

fn blank(part: CharacterPart) -> RgbaImage {
    let layout = character_layout(part);
    RgbaImage {
        width: layout.width,
        height: layout.height,
        rgba: vec![0; layout.width * layout.height * 4],
    }
}
fn paint(image: &mut RgbaImage, x: usize, y: usize, rgba: [u8; 4]) {
    let i = (y * image.width + x) * 4;
    image.rgba[i..i + 4].copy_from_slice(&rgba);
}
fn color(hex: &str) -> [u8; 4] {
    let mut text = if hex.len() <= 4 {
        hex.chars().flat_map(|c| [c, c]).collect::<String>()
    } else {
        hex.to_owned()
    };
    if text.len() == 6 {
        text.push_str("ff");
    }
    u32::from_str_radix(&text, 16).unwrap().to_be_bytes()
}
fn suffix_stages(encoding: &Encoding, part: CharacterPart) -> Vec<HashMap<[u8; 4], [u8; 4]>> {
    encoding.directives[character_prefix(part).len()..]
        .split('?')
        .filter(|s| !s.is_empty())
        .map(|stage| {
            stage
                .split(';')
                .skip(1)
                .map(|pair| {
                    let (a, b) = pair.split_once('=').unwrap();
                    (color(a), color(b))
                })
                .collect()
        })
        .collect()
}
fn render_suffix(encoding: &Encoding, part: CharacterPart, rgba: [u8; 4]) -> [u8; 4] {
    suffix_stages(encoding, part)
        .iter()
        .fold(rgba, |rgba, stage| {
            stage.get(&rgba).copied().unwrap_or(rgba)
        })
}
fn baseline() -> Value {
    json!({"name":"Original 雪", "species":"human", "gender":"female", "personality":{"idle":"idle.4","headOffset":[1,2]}, "hairGroup":"hair", "hairType":"male1", "hairDirectives":"?replace;fff=12345680", "facialHairGroup":"", "facialHairType":"", "facialHairDirectives":"", "facialMaskGroup":"", "facialMaskType":"", "facialMaskDirectives":"", "bodyDirectives":"?replace;fff=000", "emoteDirectives":"", "color":[12,23,34,255]})
}

#[test]
fn unrenderable_cells_and_conflicting_shared_frames_are_rejected() {
    let part = CharacterPart::BackArm;
    let mut image = blank(part);
    paint(&mut image, 0, 0, [255, 255, 255, 1]);
    assert!(
        validate_character_layer(CharacterLayer {
            part,
            image: &image
        })
        .is_err()
    );
    paint(&mut image, 0, 0, [255, 255, 255, 0]);
    let frames = &character_layout(part).frames;
    let (a, b) = frames
        .iter()
        .find_map(|a| {
            frames
                .iter()
                .find(|b| a.name != b.name && a.id == b.id)
                .map(|b| (a, b))
        })
        .unwrap();
    paint(&mut image, a.x + 1, a.y + 1, [4, 7, 9, 128]);
    assert!(
        validate_character_layer(CharacterLayer {
            part,
            image: &image
        })
        .is_err()
    );
    for frame in frames.iter().filter(|f| f.id == a.id) {
        paint(&mut image, frame.x + 1, frame.y + 1, [4, 7, 9, 128]);
    }
    validate_character_layer(CharacterLayer {
        part,
        image: &image,
    })
    .unwrap();
    // Transparent RGB is not an invisible conflict, and must not be scrubbed.
    paint(&mut image, b.x + 2, b.y + 2, [1, 3, 5, 0]);
    validate_character_layer(CharacterLayer {
        part,
        image: &image,
    })
    .unwrap();
    assert_eq!(
        &image.rgba[((b.y + 2) * image.width + b.x + 2) * 4..][..4],
        &[1, 3, 5, 0]
    );
}

#[test]
fn distinct_head_poses_keep_independent_visible_artwork() {
    let part = CharacterPart::Head;
    let mut image = blank(part);
    let frames = &character_layout(part).frames;
    // Derive artwork from the pose's position, NOT its fingerprint ID: an
    // accidental future ID merge must reject this independently drawn sheet.
    let artwork = |index: usize| [index as u8, 211, 255, [1, 128, 254, 255][index % 4]];
    for (index, frame) in frames.iter().enumerate() {
        for x in 0..43 {
            paint(&mut image, frame.x + x, frame.y + 19, artwork(index));
        }
    }
    for strategy in [Strategy::Text, Strategy::Compressed, Strategy::Passes] {
        for palette in [false, true] {
            for encoding in character_candidates(
                &[CharacterLayer {
                    part,
                    image: &image,
                }],
                palette,
                strategy,
            )
            .unwrap()
            {
                let stages = suffix_stages(&encoding, part);
                let render = |pixel| {
                    stages.iter().fold(pixel, |rgba, stage| {
                        stage.get(&rgba).copied().unwrap_or(rgba)
                    })
                };
                for (index, frame) in frames.iter().enumerate() {
                    for x in 0..43 {
                        assert_eq!(
                            render([x, frame.id, 23, 0]),
                            artwork(index),
                            "{} x={x}",
                            frame.name
                        );
                    }
                    let unpainted = [21, frame.id, 24, 0];
                    assert_eq!(
                        render(unpainted),
                        unpainted,
                        "{} unpainted address",
                        frame.name
                    );
                }
            }
        }
    }
}

#[test]
fn all_optimization_targets_preserve_visible_rgba_and_transparent_addresses() {
    let part = CharacterPart::Head;
    let mut image = blank(part);
    for frame in &character_layout(part).frames {
        for x in 0..43 {
            paint(
                &mut image,
                frame.x + x,
                frame.y + 1,
                [frame.id, 7, 250, if x % 2 == 0 { 1 } else { 128 }],
            );
        }
        paint(&mut image, frame.x + 3, frame.y + 5, [41, 52, 63, 0]);
    }
    for strategy in [Strategy::Text, Strategy::Compressed, Strategy::Passes] {
        for palette in [false, true] {
            for encoded in character_candidates(
                &[CharacterLayer {
                    part,
                    image: &image,
                }],
                palette,
                strategy,
            )
            .unwrap()
            {
                let stages = suffix_stages(&encoded, part);
                for frame in &character_layout(part).frames {
                    for y in 0..43 {
                        for x in 0..43 {
                            let address = [x as u8, frame.id, (42 - y) as u8, 0];
                            let offset = ((frame.y + y) * image.width + frame.x + x) * 4;
                            let expected: [u8; 4] =
                                image.rgba[offset..offset + 4].try_into().unwrap();
                            let actual = stages.iter().fold(address, |rgba, stage| {
                                stage.get(&rgba).copied().unwrap_or(rgba)
                            });
                            assert_eq!(actual, if expected[3] == 0 { address } else { expected });
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn shared_body_fields_require_the_three_separate_render_layers() {
    let body = blank(CharacterPart::Body);
    assert!(
        character_candidates(
            &[CharacterLayer {
                part: CharacterPart::Body,
                image: &body
            }],
            true,
            Strategy::Text
        )
        .is_err()
    );
    let front = blank(CharacterPart::FrontArm);
    let back = blank(CharacterPart::BackArm);
    let layers = [
        CharacterLayer {
            part: CharacterPart::Body,
            image: &body,
        },
        CharacterLayer {
            part: CharacterPart::FrontArm,
            image: &front,
        },
        CharacterLayer {
            part: CharacterPart::BackArm,
            image: &back,
        },
    ];
    let encoded = character_candidates(&layers, true, Strategy::Text).unwrap();
    // Erased native head uses an unpainted ID, distinct from every body/arm.
    for x in [0, 21, 42] {
        assert_eq!(
            render_suffix(&encoded[0], CharacterPart::Body, [x, 0, 12, 0]),
            [x, 0, 12, 0]
        );
    }
}

#[test]
fn full_redraw_preserves_non_art_identity_and_untouched_hair_resource() {
    let original = baseline();
    let patch = identity_patch(&original, Some("?body"), Some("?head"), None).unwrap();
    assert!(patch.get("gender").is_none());
    assert!(patch.get("species").is_none());
    assert_eq!(patch["hairGroup"], "hair");
    assert_eq!(patch["hairType"], "1");
    assert_eq!(
        patch["hairDirectives"],
        "?multiply=00000000?replace;0000=fff?blendmult=/humanoid/human/hair/male1.png:normal?replace;fff=12345680"
    );
    let merged = apply_identity_patch(&original, &patch).unwrap();
    for key in [
        "name",
        "species",
        "gender",
        "personality",
        "color",
        "facialHairType",
        "facialMaskType",
    ] {
        assert_eq!(merged[key], original[key]);
    }
    assert_eq!(merged["imagePath"], "floran");
    assert!(identity_patch(&original, Some("?body"), None, None).is_err());
    assert!(identity_patch(&original, None, Some("?head"), None).is_err());
}

#[test]
fn repeated_body_edits_leave_unchanged_layers_and_nullable_metadata_intact() {
    let mut original = baseline();
    original["nullableMetadata"] = Value::Null;
    let first = apply_identity_patch(
        &original,
        &identity_patch(&original, Some("?body-first"), Some("?head-first"), None).unwrap(),
    )
    .unwrap();
    let patch = identity_patch(&first, Some("?body-next"), Some("?head-next"), None).unwrap();
    let merged = apply_identity_patch(&first, &patch).unwrap();
    for field in [
        "hairGroup",
        "hairType",
        "hairDirectives",
        "facialHairGroup",
        "facialHairType",
        "facialHairDirectives",
        "facialMaskGroup",
        "facialMaskType",
        "facialMaskDirectives",
    ] {
        assert!(
            patch.get(field).is_none(),
            "{field} was unnecessarily changed"
        );
        assert_eq!(merged[field], first[field]);
    }
    assert_eq!(merged["bodyDirectives"], "?body-next");
    assert_eq!(merged["emoteDirectives"], "?head-next");
    assert_eq!(merged.get("nullableMetadata"), Some(&Value::Null));
}

#[test]
fn hair_only_changes_no_other_fields_and_requires_an_existing_resource() {
    let original = baseline();
    let patch = identity_patch(&original, None, None, Some("?hair")).unwrap();
    assert_eq!(patch, json!({"hairDirectives":"?hair"}));
    let mut no_hair = original.clone();
    no_hair["hairType"] = "".into();
    assert!(identity_patch(&no_hair, None, None, Some("?hair")).is_err());
    assert!(identity_patch(&json!({}), None, None, Some("?hair")).is_err());
}

#[test]
fn original_image_root_and_facial_layers_are_preserved_without_traversal() {
    let mut original = baseline();
    original["imagePath"] = "apex".into();
    original["facialHairGroup"] = "beardmale".into();
    original["facialHairType"] = "1".into();
    original["facialHairDirectives"] = "?multiply=abcdef".into();
    let patch = identity_patch(&original, Some("?body"), Some("?head"), Some("?hair")).unwrap();
    assert_eq!(patch["facialHairType"], "1");
    assert_eq!(patch["facialHairGroup"], "hair");
    assert_eq!(
        patch["facialHairDirectives"],
        "?multiply=00000000?replace;0000=fff?blendmult=/humanoid/apex/beardmale/1.png:normal?multiply=abcdef"
    );
    original["facialHairGroup"] = "../apex/beardmale".into();
    assert!(identity_patch(&original, Some("?body"), Some("?head"), None).is_err());
}

#[test]
fn atomic_local_command_quotes_lua_and_restores_absent_optional_image_path() {
    let patch = json!({"hairDirectives":"?replace;a=雪\"\\\n\u{0001}9", "imagePath":null});
    let command = identity_command(&patch).unwrap();
    assert_eq!(
        command,
        "/run local identity=player.humanoidIdentity(); identity[\"hairDirectives\"]=\"?replace;a=雪\\\"\\\\\\010\\0019\"; identity[\"imagePath\"]=nil; player.setHumanoidIdentity(identity)"
    );
    let original = json!({"name":"untouched", "imagePath":"floran"});
    let restored = apply_identity_patch(&original, &patch).unwrap();
    assert!(restored.get("imagePath").is_none());
    assert_eq!(restored["name"], "untouched");
    assert!(identity_command(&json!({"gender":"male"})).is_err());
    assert!(identity_command(&json!({"hairDirectives":null})).is_err());
}

#[test]
fn distinct_body_and_arm_poses_keep_independent_visible_artwork() {
    let parts = [
        CharacterPart::Body,
        CharacterPart::FrontArm,
        CharacterPart::BackArm,
    ];
    let artwork = |part_index: usize, frame_index: usize, name: &str| {
        // Only these actually identical native back-arm sources must agree.
        // All other artwork is pose-position-derived, never fingerprint-ID-derived.
        let index = if parts[part_index] == CharacterPart::BackArm
            && matches!(name, "walk.1" | "walk.2" | "walk.3" | "run.1" | "run.2")
        {
            255
        } else {
            frame_index as u8
        };
        [
            index,
            211 - part_index as u8,
            255,
            [1, 128, 254, 255][index as usize % 4],
        ]
    };
    let mut images = parts.map(blank);
    for (part_index, (&part, image)) in parts.iter().zip(&mut images).enumerate() {
        for (frame_index, frame) in character_layout(part).frames.iter().enumerate() {
            for x in 0..43 {
                paint(
                    image,
                    frame.x + x,
                    frame.y + 19,
                    artwork(part_index, frame_index, &frame.name),
                );
            }
        }
    }
    let layers = std::array::from_fn::<_, 3, _>(|index| CharacterLayer {
        part: parts[index],
        image: &images[index],
    });
    for strategy in [Strategy::Text, Strategy::Compressed, Strategy::Passes] {
        for palette in [false, true] {
            for encoding in character_candidates(&layers, palette, strategy).unwrap() {
                let stages = suffix_stages(&encoding, CharacterPart::Body);
                let render = |pixel| {
                    stages.iter().fold(pixel, |rgba, stage| {
                        stage.get(&rgba).copied().unwrap_or(rgba)
                    })
                };
                for (part_index, &part) in parts.iter().enumerate() {
                    for (frame_index, frame) in character_layout(part).frames.iter().enumerate() {
                        for x in 0..43 {
                            assert_eq!(
                                render([x, frame.id, 23, 0]),
                                artwork(part_index, frame_index, &frame.name),
                                "{part:?} {} x={x}",
                                frame.name
                            );
                        }
                        let unpainted = [21, frame.id, 24, 0];
                        assert_eq!(
                            render(unpainted),
                            unpainted,
                            "{part:?} {} unpainted address",
                            frame.name
                        );
                    }
                }
                assert_eq!(render([21, 0, 23, 0]), [21, 0, 23, 0]);
            }
        }
    }
}
