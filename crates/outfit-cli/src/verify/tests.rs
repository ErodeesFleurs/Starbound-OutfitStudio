use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

fn model_fixture(visible: Option<[u8; 4]>) -> Fixture {
    let mut rgba = vec![0; FRAME_SIZE * FRAME_SIZE * 4];
    // Invisible source RGB is intentionally nonzero and must not leak into expectations.
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel.copy_from_slice(&[19, 37, 53, 0]);
    }
    if let Some(pixel) = visible {
        rgba[..4].copy_from_slice(&pixel);
    }
    Fixture {
        name: "model",
        kind: Garment::Pants,
        layers: vec![Arc::new(LoadedLayer {
            path: "/model.png".into(),
            frames_path: "/model.frames".into(),
            image: RgbaImage {
                width: FRAME_SIZE,
                height: FRAME_SIZE,
                rgba,
            },
            frames: core::layout(Sheet::Pants),
            poses: vec![Pose {
                name: "idle".into(),
                id: 7,
                column: 0,
                row: 0,
            }],
        })],
    }
}
#[test]
fn independent_model_uses_one_simultaneous_lookup_per_pass() {
    let fixture = model_fixture(Some([0x12, 0x34, 0x56, 0xff]));
    let passes = replace_passes("?replace;00072a00=123456;123456=ff00ff").unwrap();
    assert_eq!(
        verify_pixels(&fixture, &passes).unwrap(),
        (FRAME_SIZE * FRAME_SIZE, FRAME_SIZE * FRAME_SIZE - 1)
    );
    let chained = replace_passes("?replace;00072a00=123456?replace;123456=ff00ff").unwrap();
    assert!(verify_pixels(&fixture, &chained).is_err());
}
#[test]
fn every_alpha_zero_coordinate_preserves_its_exact_address_not_source_hidden_rgb() {
    let fixture = model_fixture(None);
    assert_eq!(
        verify_pixels(&fixture, &[]).unwrap(),
        (FRAME_SIZE * FRAME_SIZE, FRAME_SIZE * FRAME_SIZE)
    );
    let erased = replace_passes("?replace;00072a00=0000").unwrap();
    assert!(verify_pixels(&fixture, &erased).is_err());
    let leaked = replace_passes("?replace;00072a00=13253500").unwrap();
    assert!(verify_pixels(&fixture, &leaked).is_err());
}
#[test]
fn directive_decoder_accepts_all_engine_color_forms_and_rejects_ambiguous_input() {
    for (text, value) in [
        ("abc", 0xaabbccff),
        ("abcd", 0xaabbccdd),
        ("ABCDEF", 0xabcdefff),
        ("01234567", 0x01234567),
    ] {
        assert_eq!(decode_color(text).unwrap(), value);
    }
    for text in ["", "a", "abcde", "abcdefg", "gggg", " abc", "éff"] {
        assert!(decode_color(text).is_err());
    }
    for suffix in [
        "replace;abc=def",
        "?multiply=fff",
        "?replace;abc",
        "?replace;abc=def=000",
        "?replace;abc=def;aabbcc=000",
    ] {
        assert!(replace_passes(suffix).is_err());
    }
}

static SERIAL: AtomicU64 = AtomicU64::new(0);
struct TempDirectory(PathBuf);
impl TempDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "outfit-native-fixture-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn private_fixture_refuses_reused_repository_paths_and_uses_owner_only_permissions() {
    let directory = TempDirectory::new();
    let manifest = json!({"format":1,"frameSize":43,"cases":[]});
    emit_fixture(&directory.0, &manifest, &[]).unwrap();
    assert!(emit_fixture(&directory.0, &manifest, &[]).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&directory.0).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(directory.0.join("outfit-verification.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let child = directory.0.join("new");
    emit_fixture(&child, &manifest, &[]).unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert!(emit_fixture(&repo, &manifest, &[]).is_err());
}
#[test]
fn report_output_cannot_overwrite_package_through_canonical_or_hardlink_aliases() {
    let directory = TempDirectory::new();
    let pak = directory.0.join("source.pak");
    fs::write(&pak, b"immutable packed bytes").unwrap();
    assert!(write_report(&pak, &pak, &json!({})).is_err());
    let alias = directory.0.join("alias");
    fs::hard_link(&pak, &alias).unwrap();
    #[cfg(unix)]
    assert!(write_report(&alias, &pak, &json!({})).is_err());
    assert_eq!(fs::read(&pak).unwrap(), b"immutable packed bytes");
}

#[test]
fn actual_frame_names_follow_grid_names_numeric_fallback_and_frame_list_rectangles() {
    let frames = vec![vec!["0a".to_owned(), String::new(), "0b".to_owned()]];
    let named = json!({"frameGrid":{"size":[43,43],"dimensions":[3,1],"names":[["idle","unused","run.2"]] }});
    let poses = poses_for_spec(&named, &frames, "/sprite.png", "/sprite.frames").unwrap();
    assert_eq!(
        poses
            .iter()
            .map(|p| (p.name.as_str(), p.id, p.column, p.row))
            .collect::<Vec<_>>(),
        vec![("idle", 10, 0, 0), ("run.2", 11, 2, 0)]
    );
    let numeric = json!({"frameGrid":{"size":[43,43],"dimensions":[5,1]}});
    let poses = poses_for_spec(&numeric, &frames, "/sprite.png", "/sprite.frames").unwrap();
    assert_eq!(
        poses.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        vec!["0", "2"]
    );
    let listed = json!({"frameList":{"idle":[0,0,43,43],"run.2":[86,0,129,43]}});
    let poses = poses_for_spec(&listed, &frames, "/sprite.png", "/sprite.frames").unwrap();
    assert_eq!(
        poses.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        vec!["idle", "run.2"]
    );
    for bad in [
        json!({}),
        json!({"frameGrid":{"size":[42,43],"dimensions":[3,1]}}),
        json!({"frameGrid":{"size":[43,43],"dimensions":[2,1]}}),
        json!({"frameGrid":{"size":[43,43],"dimensions":[3.5,1]}}),
        json!({"frameGrid":{"size":[43,43],"dimensions":[3,1],"begin":[0.5,0]}}),
        json!({"frameGrid":{"size":[43,43],"dimensions":[3,1],"names":[["idle",null,"bad:name"]]}}),
        json!({"frameList":{"idle":[0,0,43,43],"run.2":[85,0,128,43]}}),
    ] {
        assert!(poses_for_spec(&bad, &frames, "/sprite.png", "/sprite.frames").is_err());
    }
}

#[test]
fn private_fixture_adds_only_safe_synthetic_hat_png_names_without_overwrite() {
    let directory = TempDirectory::new();
    let target = directory.0.join("fixture");
    let manifest = json!({"format":1,"frameSize":43,"cases":[],"hats":[]});
    for name in [
        "../hat-expected-0.png",
        "hat-expected-../escape.png",
        "_metadata",
        "/hat-expected-0.png",
    ] {
        assert!(emit_fixture(&target, &manifest, &[(name.into(), vec![1, 2, 3])]).is_err());
        assert!(!target.exists());
    }
    let pngs = vec![("hat-expected-0.png".into(), vec![1, 2, 3])];
    emit_fixture(&target, &manifest, &pngs).unwrap();
    assert!(
        emit_fixture(
            &target,
            &manifest,
            &[("hat-expected-0.png".into(), vec![9, 9, 9])]
        )
        .is_err()
    );
    assert_eq!(
        fs::read(target.join("hat-expected-0.png")).unwrap(),
        [1, 2, 3]
    );
}
