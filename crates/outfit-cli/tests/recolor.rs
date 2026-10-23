use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "outfit-recolor-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
    fn png(&self, name: &str, width: u32, pixels: &[[u8; 4]]) -> PathBuf {
        let path = self.path(name);
        let mut encoder = png::Encoder::new(
            fs::File::create(&path).unwrap(),
            width,
            pixels.len() as u32 / width,
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&pixels.iter().flatten().copied().collect::<Vec<_>>())
            .unwrap();
        writer.finish().unwrap();
        path
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn recolor(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_outfit"))
        .arg("recolor")
        .args(arguments)
        .output()
        .unwrap()
}
fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}
fn error(output: &Output, message: &str) {
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(message), "{stderr} lacks {message}");
}

#[test]
fn repeated_options_pair_by_order_emit_only_directives_and_report_stats_separately() {
    let directory = Directory::new();
    let source_a = directory.png("source-a.png", 2, &[[255, 0, 0, 255], [0, 255, 0, 255]]);
    let target_a = directory.png("target-a.png", 2, &[[0, 255, 0, 255], [255, 0, 0, 255]]);
    let source_b = directory.png("source-b.png", 1, &[[17, 0, 34, 0], [1, 2, 3, 1]]);
    let target_b = directory.png("target-b.png", 1, &[[34, 51, 68, 0], [1, 2, 3, 1]]);
    let originals =
        [&source_a, &target_a, &source_b, &target_b].map(|path| fs::read(path).unwrap());
    let output = recolor(&[
        "--source",
        text(&source_a),
        "--source",
        text(&source_b),
        "--target",
        text(&target_a),
        "--target",
        text(&target_b),
        "--stats",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"?replace;f00=0f0;0f0=f00;1020=2340\n");
    let stats: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(stats["mode"], "recolor");
    assert_eq!(stats["layerCount"], 2);
    assert_eq!(stats["pixelCount"], 4);
    assert_eq!(stats["mappingCount"], 3);
    assert_eq!(stats["outputBytes"], output.stdout.len() - 1);
    for (path, original) in [&source_a, &target_a, &source_b, &target_b]
        .into_iter()
        .zip(originals)
    {
        assert_eq!(fs::read(path).unwrap(), original);
    }
    let destination = directory.path("directives.txt");
    let written = recolor(&[
        "--source",
        text(&source_a),
        "--target",
        text(&target_a),
        "--source",
        text(&source_b),
        "--target",
        text(&target_b),
        "--out",
        text(&destination),
    ]);
    assert!(written.status.success());
    assert!(written.stdout.is_empty());
    assert!(written.stderr.is_empty());
    assert_eq!(fs::read(destination).unwrap(), output.stdout);
}

#[test]
fn unchanged_inputs_output_empty_string_and_keep_statistics_truthful() {
    let directory = Directory::new();
    let source = directory.png("source.png", 2, &[[1, 2, 3, 0], [4, 5, 6, 1]]);
    let output = recolor(&[
        "--source",
        text(&source),
        "--target",
        text(&source),
        "--stats",
    ]);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"\n");
    let stats: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(stats["unchanged"], true);
    assert_eq!(stats["mappingCount"], 0);
    assert_eq!(stats["outputBytes"], 0);
}

#[test]
fn rejects_missing_pairs_counts_values_and_duplicate_single_options_before_reading_files() {
    for (arguments, message) in [
        (vec![], "At least one"),
        (
            vec!["--source", "missing.png"],
            "got 1 source(s) and 0 target(s)",
        ),
        (
            vec!["--target", "missing.png"],
            "got 0 source(s) and 1 target(s)",
        ),
        (vec!["--source"], "--source requires a value"),
        (
            vec!["--source", "--target", "missing.png"],
            "--source requires a value",
        ),
        (vec!["--target"], "--target requires a value"),
        (vec!["--out"], "--out requires a value"),
        (
            vec!["--out", "first", "--out", "second"],
            "Duplicate option --out",
        ),
        (vec!["--stats", "--stats"], "Duplicate option --stats"),
        (vec!["--format", "json"], "Unknown option --format"),
    ] {
        error(&recolor(&arguments), message);
    }
}

#[test]
fn malformed_png_dimensions_and_color_splits_do_not_create_or_modify_output() {
    let directory = Directory::new();
    let source = directory.png("source.png", 2, &[[17, 34, 51, 0], [17, 34, 51, 0]]);
    let split = directory.png("split.png", 2, &[[255, 0, 0, 255], [17, 34, 51, 0]]);
    let wrong_size = directory.png("wrong-size.png", 1, &[[17, 34, 51, 0]]);
    let invalid = directory.path("invalid.png");
    fs::write(&invalid, b"not PNG").unwrap();
    let destination = directory.path("result.txt");
    for (target, message) in [
        (&split, "Layer 1 pixel (1, 0)"),
        (&wrong_size, "dimensions must match"),
        (&invalid, "Layer 1 target"),
    ] {
        let output = recolor(&[
            "--source",
            text(&source),
            "--target",
            text(target),
            "--out",
            text(&destination),
        ]);
        error(&output, message);
        assert!(!destination.exists());
        fs::write(&destination, b"keep existing output").unwrap();
        error(
            &recolor(&[
                "--source",
                text(&source),
                "--target",
                text(target),
                "--out",
                text(&destination),
            ]),
            message,
        );
        assert_eq!(fs::read(&destination).unwrap(), b"keep existing output");
        fs::remove_file(&destination).unwrap();
    }
    error(
        &recolor(&["--source", text(&invalid), "--target", text(&source)]),
        "Layer 1 source",
    );
}

#[test]
fn output_cannot_overwrite_either_source_or_target_or_their_aliases() {
    let directory = Directory::new();
    let source = directory.png("source.png", 1, &[[255, 0, 0, 255]]);
    let target = directory.png("target.png", 1, &[[0, 255, 0, 255]]);
    let original_source = fs::read(&source).unwrap();
    let original_target = fs::read(&target).unwrap();
    let hardlink = directory.path("target-hardlink.png");
    fs::hard_link(&target, &hardlink).unwrap();
    let mut aliases = vec![source.clone(), target.clone(), hardlink];
    #[cfg(unix)]
    {
        let symlink = directory.path("source-symlink.png");
        std::os::unix::fs::symlink(&source, &symlink).unwrap();
        aliases.push(symlink);
    }
    for output in aliases {
        error(
            &recolor(&[
                "--source",
                text(&source),
                "--target",
                text(&target),
                "--out",
                text(&output),
            ]),
            "Refusing to overwrite an input PNG",
        );
    }
    assert_eq!(fs::read(&source).unwrap(), original_source);
    assert_eq!(fs::read(&target).unwrap(), original_target);
}
