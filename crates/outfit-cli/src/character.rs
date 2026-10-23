use super::{Result, fail, options, write_output};
use outfit_core::{self as core, CharacterLayer, CharacterPart, OptimizedEncoding, Strategy};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
};

const HELP: &str = "Outfit Studio — character identity redraw (not equipment)

Usage:
  outfit character-encode --identity IDENTITY.json --body BODY.png \\
    --front-arm FRONT.png --back-arm BACK.png --head EMOTES.png [--hair HAIR.png] [options]
  outfit character-encode --identity IDENTITY.json --hair HAIR.png [options]

PNG layers are decoded locally, never uploaded. No new assets are installed:
  --body PATH       Body/legs animation sheet
  --front-arm PATH  Front arm animation sheet
  --back-arm PATH   Back arm animation sheet
  --head PATH       Head/emotes animation sheet (not a single head portrait)
  --hair PATH       Independent hair layer
  Body/legs, both arms and head/emotes form one linked redraw: provide all four.
  They share bodyDirectives and the original head must be replaced in emotes.
  Hair may be changed independently or included with the linked group.
  Hair-only redraw preserves the baseline hair style, which must be nonempty.
  To hide one linked part, provide a transparent PNG at its required dimensions;
  omitting a linked input cannot preserve the original shared body layer.
  See the part dimensions/frame names below; aliases share the same source frame
  and cannot have independent images. Transparent/unmapped cells are not art.

Options:
  --identity PATH   Existing identity JSON OBJECT, not a .player binary/full save.
                    Required for EVERY redraw; preserves species and rebases
                    untouched hair/facial paths during a linked redraw.
  --strategy text|compressed|passes  Selection strategy (default text)
  --palette auto|off                Palette factoring (default auto)
  --format json|merged|command|directives   Output format (default json)
  --out PATH                        Write result instead of stdout
  --backup-out PATH                 Save exact original identity JSON to a NEW file
  --stats                           Print per-group encoding statistics to stderr

Formats:
  json        Identity PATCH: merge its keys, never replace your whole identity.
  merged      Full identity with patch applied; requires --identity.
  command     Correctly quoted LOCAL OpenSB /run atomic player-identity merge.
  directives  JSON containing only the patch's *Directives fields; not standalone
              application instructions (imagePath/rebased paths can be required).

Safety and application:
  Make a backup before editing; --backup-out writes the original input unchanged
  and refuses to overwrite any file. Stop the game before offline save editing.
  Apply the patch to identity, NOT equipment or the whole player save. Preserve
  unrelated fields. --format merged performs this merge on an identity object.
  Undo by restoring the backed-up identity with your offline editor (game closed).
  Command output uses the LOCAL player API to merge once, preserving unrelated
  identity fields. Do NOT use /identity set PATCH: it can reset omitted fields.
  /run and /identity are NOT vanilla server commands; no /spawnitem is generated.
  Only your editing client needs OpenSB; other players do not need its editor or
  assets. Image encoding alone is not full animation or
  cross-client compatibility proof; equipment can still mask character layers.
  Compression scores are native flate2 zlib level 6, not actual network sizes.
";

fn print_help() {
    print!("{HELP}");
    println!("\nRequired PNG dimensions and frames (x,y are top-down pixel origins):");
    for part in [
        CharacterPart::Body,
        CharacterPart::FrontArm,
        CharacterPart::BackArm,
        CharacterPart::Head,
        CharacterPart::Hair,
    ] {
        let layout = core::character_layout(part);
        println!("  {}: {}x{}", part.key(), layout.width, layout.height);
        for frame in &layout.frames {
            println!(
                "    {} at ({},{}) [id {}]",
                frame.name, frame.x, frame.y, frame.id
            );
            if !frame.aliases.is_empty() {
                println!("      aliases: {}", frame.aliases.join(", "));
            }
        }
    }
}

fn read_identity(path: &Path) -> Result<(Vec<u8>, Value)> {
    let bytes = fs::read(path)?;
    let identity: Value =
        serde_json::from_slice(&bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    if !identity.is_object() {
        return fail(
            "--identity must contain a JSON identity object, not an array or .player file",
        );
    }
    Ok((bytes, identity))
}

fn write_backup(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    Ok(())
}

fn directives(value: &Option<OptimizedEncoding>) -> Option<&str> {
    value
        .as_ref()
        .map(|value| value.encoding.directives.as_str())
}

fn statistics(group: &str, result: &OptimizedEncoding) -> Value {
    json!({
        "group": group,
        "strategy": result.strategy.key(),
        "baselineBytes": result.encoding.baseline_bytes,
        "outputBytes": result.encoding.output_bytes,
        "pixelCount": result.encoding.pixel_count,
        "colorCount": result.encoding.color_count,
        "paletteApplied": result.encoding.palette_applied,
        "factoredColorCount": result.encoding.factored_color_count,
        "candidateCount": result.candidate_count,
        "estimatedDeflateBytes": result.estimated_deflate_bytes
    })
}

pub fn run(args: &[String]) -> Result<()> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return Ok(());
    }
    let options = options(
        args,
        &[
            "--body",
            "--front-arm",
            "--back-arm",
            "--head",
            "--hair",
            "--identity",
            "--strategy",
            "--palette",
            "--format",
            "--out",
            "--backup-out",
        ],
        &["--stats"],
    )?;
    let strategy = Strategy::from_str(options.get("--strategy").map_or("text", String::as_str))?;
    let palette = match options.get("--palette").map_or("auto", String::as_str) {
        "auto" => true,
        "off" => false,
        value => return fail(format!("Invalid palette {value}; expected auto or off")),
    };
    let format = options.get("--format").map_or("json", String::as_str);
    if !["json", "merged", "command", "directives"].contains(&format) {
        return fail(format!(
            "Invalid character output format {format}; use --help"
        ));
    }
    let body_flags = ["--body", "--front-arm", "--back-arm", "--head"];
    let full = body_flags.iter().any(|flag| options.contains_key(*flag));
    if full && body_flags.iter().any(|flag| !options.contains_key(*flag)) {
        return fail(
            "Body/legs, front arm, back arm and head/emotes are linked: provide --body, --front-arm, --back-arm and --head together; use character-encode --help",
        );
    }
    if !full && !options.contains_key("--hair") {
        return fail(
            "Provide the four linked character layers or independent --hair; use character-encode --help",
        );
    }
    let identity_path = options
        .get("--identity")
        .ok_or("--identity PATH is required for every character redraw, including hair-only")?;
    let original = read_identity(Path::new(identity_path))?;
    let identity = &original.1;
    let mut inputs = vec![PathBuf::from(identity_path)];
    let mut images = Vec::new();
    for (flag, part) in [
        ("--body", CharacterPart::Body),
        ("--front-arm", CharacterPart::FrontArm),
        ("--back-arm", CharacterPart::BackArm),
        ("--head", CharacterPart::Head),
        ("--hair", CharacterPart::Hair),
    ] {
        if let Some(path) = options.get(flag) {
            let image =
                core::decode_png(&fs::read(path)?).map_err(|error| format!("{path}: {error}"))?;
            images.push((part, image));
            inputs.push(PathBuf::from(path));
        }
    }
    let layers: Vec<_> = images
        .iter()
        .map(|(part, image)| CharacterLayer { part: *part, image })
        .collect();
    // Input order above keeps each independently optimized directive group contiguous.
    let (body_layers, head_layers, hair_layers) = if full {
        (&layers[..3], &layers[3..4], &layers[4..])
    } else {
        (&layers[..0], &layers[..0], layers.as_slice())
    };
    let optimize = |group: &[CharacterLayer<'_>]| -> Result<Option<OptimizedEncoding>> {
        if group.is_empty() {
            return Ok(None);
        }
        Ok(Some(core::optimize_character_native(
            group, palette, strategy,
        )?))
    };
    let body = optimize(body_layers)?;
    let head = optimize(head_layers)?;
    let hair = optimize(hair_layers)?;
    let patch = core::identity_patch(
        identity,
        directives(&body),
        directives(&head),
        directives(&hair),
    )?;
    let output = match format {
        "command" => core::identity_command(&patch)?,
        "merged" => serde_json::to_string_pretty(&core::apply_identity_patch(identity, &patch)?)?,
        "directives" => {
            let fields: serde_json::Map<String, Value> = patch
                .as_object()
                .ok_or("Identity patch is not an object")?
                .iter()
                .filter(|(key, _)| key.ends_with("Directives"))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            serde_json::to_string_pretty(&fields)?
        }
        _ => serde_json::to_string_pretty(&patch)?,
    };
    if let Some(path) = options.get("--backup-out") {
        write_backup(Path::new(path), &original.0)?;
        inputs.push(PathBuf::from(path));
    }
    write_output(options.get("--out").map(Path::new), &inputs, &output)?;
    if options.contains_key("--stats") {
        let groups: Vec<_> = [("body", &body), ("head", &head), ("hair", &hair)]
            .into_iter()
            .filter_map(|(name, value)| value.as_ref().map(|value| statistics(name, value)))
            .collect();
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "character": true,
                "groups": groups,
                "compression": "native flate2 zlib level 6; not browser or network packet size"
            }))?
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn rejects_partial_linked_layers_and_missing_identity_before_reading_pngs() {
        for flag in ["--body", "--front-arm", "--back-arm", "--head"] {
            let error = run(&arguments(&[flag, "not-read.png"]))
                .unwrap_err()
                .to_string();
            assert!(error.contains("linked"), "{error}");
        }
        let error = run(&arguments(&[
            "--body",
            "not-read.png",
            "--front-arm",
            "not-read.png",
            "--back-arm",
            "not-read.png",
            "--head",
            "not-read.png",
        ]))
        .unwrap_err()
        .to_string();
        assert!(error.contains("--identity"), "{error}");
        let error = run(&arguments(&["--hair", "not-read.png"]))
            .unwrap_err()
            .to_string();
        assert!(error.contains("--identity"), "{error}");
    }

    #[test]
    fn rejects_garment_options_and_invalid_export_options_before_reading_pngs() {
        for extra in [
            vec!["--profile", "conservative"],
            vec!["--garment", "hat"],
            vec!["--format", "spawnitem"],
            vec!["--palette", "fake"],
            vec!["--format", "merged"],
            vec!["--backup-out", "not-created.json"],
        ] {
            let mut args = arguments(&["--hair", "not-read.png"]);
            args.extend(arguments(&extra));
            assert!(run(&args).is_err());
        }
    }

    #[test]
    fn identity_backup_is_exact_private_and_never_overwritten() {
        let directory =
            std::env::temp_dir().join(format!("outfit-character-backup-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let original_path = directory.join("identity.json");
        let backup_path = directory.join("backup.json");
        let bytes = b" { \"species\": \"human\", \"name\": \"A \\\"quote\\\"\", \"unrelated\": {\"x\":1} }\n";
        fs::write(&original_path, bytes).unwrap();
        let (original, identity) = read_identity(&original_path).unwrap();
        assert_eq!(identity["unrelated"]["x"], 1);
        write_backup(&backup_path, &original).unwrap();
        assert_eq!(fs::read(&backup_path).unwrap(), bytes);
        assert!(write_backup(&backup_path, b"replacement").is_err());
        assert_eq!(fs::read(&backup_path).unwrap(), bytes);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&backup_path).unwrap().permissions().mode() & 0o077,
                0
            );
        }
        assert!(
            write_output(
                Some(&original_path),
                std::slice::from_ref(&original_path),
                "replacement"
            )
            .is_err()
        );
        fs::write(&original_path, b"[]").unwrap();
        assert!(read_identity(&original_path).is_err());
        fs::remove_dir_all(directory).unwrap();
    }
}
