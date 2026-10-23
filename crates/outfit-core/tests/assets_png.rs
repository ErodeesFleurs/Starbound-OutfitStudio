use outfit_core::*;
use serde_json::{Value, json};

fn png_bytes(
    size: (u32, u32),
    color: png::ColorType,
    depth: png::BitDepth,
    bytes: &[u8],
    palette: Option<Vec<u8>>,
    transparency: Option<Vec<u8>>,
    animated: bool,
) -> Vec<u8> {
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, size.0, size.1);
        encoder.set_color(color);
        encoder.set_depth(depth);
        if let Some(palette) = palette {
            encoder.set_palette(palette);
        }
        if let Some(transparency) = transparency {
            encoder.set_trns(transparency);
        }
        if animated {
            encoder.set_animated(1, 0).unwrap();
        }
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(bytes).unwrap();
        writer.finish().unwrap();
    }
    output
}

#[test]
fn rgba_decode_preserves_hidden_rgb_and_partial_alpha_bytes() {
    let pixels = [
        254, 253, 252, 0, 18, 52, 86, 128, 17, 34, 51, 255, 0, 255, 0, 1,
    ];
    let bytes = png_bytes(
        (2, 2),
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        &pixels,
        None,
        None,
        false,
    );
    let decoded = decode_png(&bytes).unwrap();
    assert_eq!((decoded.width, decoded.height), (2, 2));
    assert_eq!(decoded.rgba, pixels);
}

#[test]
fn indexed_transparency_and_gray_rgb_normalize_without_premultiplication() {
    let indexed = png_bytes(
        (2, 1),
        png::ColorType::Indexed,
        png::BitDepth::One,
        &[0b01000000],
        Some(vec![19, 87, 203, 17, 34, 51]),
        Some(vec![0, 128]),
        false,
    );
    assert_eq!(
        decode_png(&indexed).unwrap().rgba,
        [19, 87, 203, 0, 17, 34, 51, 128]
    );
    let rgb = png_bytes(
        (2, 1),
        png::ColorType::Rgb,
        png::BitDepth::Eight,
        &[1, 2, 3, 4, 5, 6],
        None,
        Some(vec![0, 1, 0, 2, 0, 3]),
        false,
    );
    assert_eq!(decode_png(&rgb).unwrap().rgba, [1, 2, 3, 0, 4, 5, 6, 255]);
    let gray = png_bytes(
        (2, 1),
        png::ColorType::Grayscale,
        png::BitDepth::One,
        &[0b01000000],
        None,
        Some(vec![0, 0]),
        false,
    );
    assert_eq!(
        decode_png(&gray).unwrap().rgba,
        [0, 0, 0, 0, 255, 255, 255, 255]
    );
    let ga = png_bytes(
        (2, 1),
        png::ColorType::GrayscaleAlpha,
        png::BitDepth::Eight,
        &[23, 0, 87, 128],
        None,
        None,
        false,
    );
    assert_eq!(
        decode_png(&ga).unwrap().rgba,
        [23, 23, 23, 0, 87, 87, 87, 128]
    );
    let opaque = png_bytes(
        (1, 1),
        png::ColorType::Rgb,
        png::BitDepth::Eight,
        &[19, 87, 203],
        None,
        None,
        false,
    );
    assert_eq!(decode_png(&opaque).unwrap().rgba, [19, 87, 203, 255]);
}

#[test]
fn sixteen_bit_samples_use_high_bytes_and_bad_pngs_are_errors() {
    let bytes = png_bytes(
        (1, 1),
        png::ColorType::Rgba,
        png::BitDepth::Sixteen,
        &[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0, 0],
        None,
        None,
        false,
    );
    assert_eq!(decode_png(&bytes).unwrap().rgba, [0x12, 0x56, 0x9a, 0]);
    for bytes in [&[][..], b"not a PNG", &bytes[..bytes.len() / 2]] {
        assert!(decode_png(bytes).is_err());
    }
    let animated = png_bytes(
        (1, 1),
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        &[1, 2, 3, 255],
        None,
        None,
        true,
    );
    assert!(decode_png(&animated).is_err());
}

#[test]
fn production_descriptors_keep_shell_paths_flags_and_root_order() {
    for (garment, name, icon, description) in [
        (
            Garment::Pants,
            "florantier6apants",
            "pants.png:idle.2",
            "Custom Pants",
        ),
        (
            Garment::HidingPants,
            "froggmerchantlegs",
            "pantsm.png:idle.1",
            "Custom Pants",
        ),
        (
            Garment::Chest,
            "aviantier6schest",
            "fsleeve.png:idle.1",
            "Custom Sleeves",
        ),
        (
            Garment::Back,
            "tigertailback",
            "back.png:idle.1",
            "Custom Back",
        ),
    ] {
        let item = descriptor(garment, "?replace;0010=f00");
        assert_eq!(item["name"], name);
        assert_eq!(item["count"], 1);
        assert_eq!(item["parameters"]["inventoryIcon"], icon);
        assert_eq!(item["parameters"]["shortdescription"], description);
        assert_eq!(
            item["parameters"]["description"],
            "Made using the Outfit Generator."
        );
        assert_eq!(item["parameters"]["directives"], "?replace;0010=f00");
        assert_eq!(
            item["parameters"].get("hideBody"),
            if garment == Garment::HidingPants {
                Some(&Value::Bool(true))
            } else {
                None
            }
        );
        assert_eq!(
            item.as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["name", "count", "parameters"]
        );
        assert!(!item.to_string().contains("/monsters/"));
        assert_eq!(
            descriptor(garment, "new")["parameters"]["directives"],
            "new"
        );
        assert_eq!(item["parameters"]["directives"], "?replace;0010=f00");
    }
}

#[test]
fn command_double_escaping_and_single_quote_semantics_do_not_mutate_input() {
    let item =
        json!({"name":"thing","count":1,"parameters":{"description":"O'Brien \"quote\" \\path"}});
    let original = item.clone();
    let expected = r#"/spawnitem thing 1 '{"description":"O\'Brien \\\"quote\\\" \\\\path"}'"#;
    assert_eq!(spawn_command(&item).unwrap(), expected);
    assert_eq!(spawn_command(&item).unwrap(), expected);
    assert_eq!(item, original);
    let arrays = json!({"parameters":{"array":["a\"b",{"line":"c\\d\n"}]}});
    assert_eq!(
        spawn_command(&arrays).unwrap(),
        r#"/spawnitem perfectlygenericitem 1 '{"array":["a\\\"b",{"line":"c\\\\d\n"}]}'"#
    );
    assert_eq!(
        spawn_command(&json!({"name":23,"count":"2"})).unwrap(),
        "/spawnitem perfectlygenericitem 1"
    );
}

#[test]
fn command_uses_js_number_rendering_and_integer_property_order() {
    let item: Value = serde_json::from_str(r#"{"name":"item","count":1.0,"parameters":{"z":1.0,"2":2,"01":3,"1":4,"+0":5,"tiny":1e-7,"large":1e21,"zero":-0.0}}"#).unwrap();
    assert_eq!(
        spawn_command(&item).unwrap(),
        r#"/spawnitem item 1 '{"1":4,"2":2,"z":1,"01":3,"+0":5,"tiny":1e-7,"large":1e+21,"zero":0}'"#
    );
    for item in [
        Value::Null,
        json!([]),
        json!({"parameters":null}),
        json!({"parameters":{"nested":[null]}}),
        json!({"parameters":"invalid"}),
    ] {
        assert!(spawn_command(&item).is_err());
    }
}

#[test]
fn results_serialize_with_flattened_legacy_camel_case_metadata() {
    let result = select(
        candidates(&[], "", true, Strategy::Text).unwrap(),
        &[16],
        Strategy::Text,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        json!({"directives":"?replace","baselineBytes":8,"outputBytes":8,"pixelCount":0,"colorCount":0,"paletteApplied":false,"factoredColorCount":0,"strategy":"text","estimatedDeflateBytes":16,"candidateCount":1})
    );
}
