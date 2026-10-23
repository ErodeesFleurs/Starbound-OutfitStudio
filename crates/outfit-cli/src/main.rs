mod character;
mod character_verify;
mod hat_verify;
mod native;
mod packed;
mod recolor;
mod verify;

use outfit_core::{self as core, Garment, Layer, ProfileChoice, Sheet, Strategy};
use std::{
    borrow::Cow,
    collections::BTreeMap,
    error::Error,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    str::FromStr,
};

type Result<T, E = Box<dyn Error>> = std::result::Result<T, E>;
fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(message.into().into())
}

const HELP: &str = "Outfit Studio — local, lossless Starbound outfit encoding

Usage:
  outfit encode --garment KIND [PNG layers] [options]
  outfit character-encode [character PNG layers] [options]
  outfit recolor --source ORIGINAL.png --target EDITED.png [more pairs] [--out PATH] [--stats]
  outfit verify-assets --pak PATH [--hats-only | --characters-only] [--out PATH] [--engine-dir EXTERNAL_PATH]
  outfit native-verify --mode vanilla|opensb [native runner options]

Encode garment kinds: pants, hidingPants, chest, back, hat
PNG layers (all are decoded locally, never uploaded):
  --input PATH          Main sheet or hat PNG (1..256; 86x215 crops top-right 43x43)
  --pants PATH          Explicit pants sheet
  --body PATH           Chest body sheet (optional if sleeves provided)
  --front-sleeves PATH  Optional chest front sleeves
  --back-sleeves PATH   Optional chest back sleeves
  --back PATH           Explicit back sheet
Options:
  --profile auto|original|conservative|compact  Helper selection (default auto)
  --strategy text|compressed|passes         Selection strategy (default text)
  --palette auto|off                       Palette factoring (default auto)
  --format json|command|directives          Output format (default json)
  --out PATH                               Write result instead of stdout
  --stats                                  Print encoding statistics to stderr
  Auto compares complete garment directives across all three helper profiles.
  Text minimizes raw bytes; compressed minimizes full-string Deflate estimates;
  passes disables palette aliases and minimizes raw bytes with one pixel stage.
Hat-only options:
  --name TEXT                              Hat short description
  --description TEXT                       Hat description
  --rarity common|uncommon|rare|legendary|essential
  --remove-hair                            Mask hair
  --hide-body                              Use the vanilla Frogg base
  Hats do not use --profile.

Character redraw:
  Use outfit character-encode --help for independent body/arms, head/emotes and
  hair inputs, identity patches, local application commands and backup exports.
  Character output is not equipment. Command export uses a LOCAL OpenSB /run
  player-identity merge, not /identity set (which can reset unrelated fields).
  Neither /run nor /identity is a vanilla server command. Offline identity
  editing also works without adding assets to other players' installations.

Direct recoloring:
  Use outfit recolor --help for paired local source/target PNGs. This independent
  mode outputs only directives, not equipment or an identity patch.

verify-assets reads only the explicitly supplied SBAsset6 package. It checks real
outfit fixtures, hats and character redraws against independent RGBA models and
reference pixels. --hats-only and --characters-only select one suite.
--engine-dir must be new/empty and outside the repository; private expected PNGs,
manifests and Lua probes are written there.
Native compressed candidate scores use flate2 zlib level 6, not browser or wire sizes.
Use outfit native-verify --help for native runner options.
";
fn options(
    args: &[String],
    allowed: &[&str],
    switches: &[&str],
) -> Result<BTreeMap<String, String>> {
    let mut options = BTreeMap::new();
    let mut index = 0;
    while index < args.len() {
        let argument = &args[index];
        if options.contains_key(argument) {
            return fail(format!("Duplicate option {argument}"));
        }
        if switches.contains(&argument.as_str()) {
            options.insert(argument.clone(), String::new());
        } else if allowed.contains(&argument.as_str()) {
            index += 1;
            let value = args
                .get(index)
                .filter(|value| !value.starts_with("--"))
                .ok_or_else(|| format!("{argument} requires a value"))?;
            options.insert(argument.clone(), value.clone());
        } else {
            return fail(format!("Unknown option {argument}; use --help"));
        }
        index += 1;
    }
    Ok(options)
}
fn encode(args: &[String]) -> Result<()> {
    let options = options(
        args,
        &[
            "--garment",
            "--input",
            "--pants",
            "--body",
            "--front-sleeves",
            "--back-sleeves",
            "--back",
            "--profile",
            "--strategy",
            "--palette",
            "--format",
            "--out",
            "--name",
            "--description",
            "--rarity",
        ],
        &["--stats", "--remove-hair", "--hide-body"],
    )?;
    let garment = options
        .get("--garment")
        .ok_or("--garment KIND is required")?;
    let hat = hat_options(garment, &options)?;
    let strategy = Strategy::from_str(options.get("--strategy").map_or("text", String::as_str))?;
    let palette = match options.get("--palette").map_or("auto", String::as_str) {
        "auto" => true,
        "off" => false,
        value => return fail(format!("Invalid palette {value}; expected auto or off")),
    };
    let format = options.get("--format").map_or("json", String::as_str);
    if !["json", "command", "directives"].contains(&format) {
        return fail(format!("Invalid output format {format}"));
    }
    let mut inputs = Vec::new();
    let (result, descriptor, profile_key, profile_count, candidate_limit) = if let Some(metadata) =
        hat
    {
        let input = PathBuf::from(
            options
                .get("--input")
                .ok_or("Hat encoding requires --input PATH")?,
        );
        let image = core::decode_png(&fs::read(&input)?)
            .map_err(|error| format!("{}: {error}", input.display()))?;
        inputs.push(input);
        let result = core::optimize_hat_native(&image, palette, strategy)?;
        let descriptor = core::hat_descriptor(&result.encoding.directives, &metadata);
        (result, descriptor, None, None, None)
    } else {
        let kind = Garment::from_str(garment)?;
        let profile =
            ProfileChoice::from_str(options.get("--profile").map_or("auto", String::as_str))?;
        let main_sheet = match kind {
            Garment::Pants | Garment::HidingPants => Sheet::Pants,
            Garment::Chest => Sheet::Body,
            Garment::Back => Sheet::Back,
        };
        let mut sheets = Vec::new();
        if let Some(path) = options.get("--input") {
            sheets.push((main_sheet, PathBuf::from(path)));
        }
        for (flag, sheet) in [
            ("--pants", Sheet::Pants),
            ("--body", Sheet::Body),
            ("--front-sleeves", Sheet::FrontSleeves),
            ("--back-sleeves", Sheet::BackSleeves),
            ("--back", Sheet::Back),
        ] {
            if let Some(path) = options.get(flag) {
                if sheet != main_sheet
                    && !(kind == Garment::Chest
                        && matches!(sheet, Sheet::FrontSleeves | Sheet::BackSleeves))
                {
                    return fail(format!("{flag} is not a layer of {}", kind.key()));
                }
                if sheets.iter().any(|(existing, _)| *existing == sheet) {
                    return fail(format!("Duplicate layer {flag} (including --input)"));
                }
                sheets.push((sheet, PathBuf::from(path)));
            }
        }
        if sheets.is_empty() {
            return fail(
                "At least one PNG layer is required; use --input or explicit layer options",
            );
        }
        sheets.sort_by_key(|(sheet, _)| match sheet {
            Sheet::Pants | Sheet::Body | Sheet::Back => 0,
            Sheet::FrontSleeves => 1,
            Sheet::BackSleeves => 2,
        });
        let images = sheets
            .iter()
            .map(|(_, path)| -> Result<_> {
                core::decode_png(&fs::read(path)?)
                    .map_err(|error| format!("{}: {error}", path.display()).into())
            })
            .collect::<Result<Vec<_>>>()?;
        let layers = images
            .iter()
            .zip(&sheets)
            .map(|(image, (sheet, _))| Layer {
                image,
                frames: core::layout(*sheet),
            })
            .collect::<Vec<_>>();
        let selected = core::optimize_garment_native(&layers, kind, profile, palette, strategy)?;
        let profile_key = selected.profile.key();
        let profile_count = selected.profile_count;
        let candidate_limit = selected.candidate_limit;
        let result = selected.result;
        let descriptor = core::descriptor(kind, &result.encoding.directives);
        inputs.extend(sheets.into_iter().map(|(_, path)| path));
        (
            result,
            descriptor,
            Some(profile_key),
            Some(profile_count),
            Some(candidate_limit),
        )
    };
    let output = match format {
        "json" => Cow::Owned(serde_json::to_string_pretty(&descriptor)?),
        "command" => Cow::Owned(core::spawn_command(&descriptor)?),
        _ => Cow::Borrowed(result.encoding.directives.as_str()),
    };
    write_output(options.get("--out").map(Path::new), &inputs, &output)?;
    if options.contains_key("--stats") {
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "garment":garment, "profile":profile_key,
                "profileChoice":profile_key.map(|_| options.get("--profile").map_or("auto", String::as_str)),
                "profileCount":profile_count,"candidateLimit":candidate_limit,
                "strategy":strategy.key(),"baselineBytes":result.encoding.baseline_bytes,
                "outputBytes":result.encoding.output_bytes,"pixelCount":result.encoding.pixel_count,
                "colorCount":result.encoding.color_count,"paletteApplied":result.encoding.palette_applied,
                "factoredColorCount":result.encoding.factored_color_count,
                "candidateCount":result.candidate_count,"estimatedDeflateBytes":result.estimated_deflate_bytes,
                "compression":"native flate2 zlib level 6; not browser or network packet size"
            }))?
        );
    }
    Ok(())
}
fn hat_options(
    garment: &str,
    options: &BTreeMap<String, String>,
) -> Result<Option<core::HatMetadata>> {
    const HAT_FLAGS: [&str; 5] = [
        "--name",
        "--description",
        "--rarity",
        "--remove-hair",
        "--hide-body",
    ];
    if garment != "hat" {
        if let Some(flag) = HAT_FLAGS.iter().find(|flag| options.contains_key(**flag)) {
            return fail(format!("{flag} is only valid with --garment hat"));
        }
        return Ok(None);
    }
    for flag in [
        "--profile",
        "--pants",
        "--body",
        "--front-sleeves",
        "--back-sleeves",
        "--back",
    ] {
        if options.contains_key(flag) {
            return fail(format!("{flag} is not applicable to hats; use --input"));
        }
    }
    let defaults = core::HatMetadata::default();
    Ok(Some(core::HatMetadata {
        shortdescription: options
            .get("--name")
            .cloned()
            .unwrap_or(defaults.shortdescription),
        description: options
            .get("--description")
            .cloned()
            .unwrap_or(defaults.description),
        rarity: options
            .get("--rarity")
            .map(|value| core::HatRarity::from_str(value))
            .transpose()?
            .unwrap_or(defaults.rarity),
        remove_hair: options.contains_key("--remove-hair"),
        hide_body: options.contains_key("--hide-body"),
    }))
}
fn write_output(target: Option<&Path>, inputs: &[PathBuf], output: &str) -> Result<()> {
    if let Some(target) = target {
        if target.exists() {
            let canonical = fs::canonicalize(target)?;
            for input in inputs {
                let same = fs::canonicalize(input)? == canonical;
                #[cfg(unix)]
                let same = {
                    use std::os::unix::fs::MetadataExt;
                    let source = fs::metadata(input)?;
                    let destination = fs::metadata(target)?;
                    same || (source.dev() == destination.dev() && source.ino() == destination.ino())
                };
                if same {
                    return fail("Refusing to overwrite an input PNG");
                }
            }
        }
        let mut file = fs::File::create(target)?;
        file.write_all(output.as_bytes())?;
        file.write_all(b"\n")?;
    } else {
        writeln!(io::stdout().lock(), "{output}")?;
    }
    Ok(())
}
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else {
        print!("{HELP}");
        return Ok(());
    };
    if matches!(command.as_str(), "help" | "--help" | "-h") {
        print!("{HELP}");
        return Ok(());
    }
    if command == "native-verify" {
        return native::run(&args[1..]);
    }
    if command == "character-encode" {
        return character::run(&args[1..]);
    }
    if command == "recolor" {
        return recolor::run(&args[1..]);
    }
    if args[1..].iter().any(|arg| arg == "--help" || arg == "-h") {
        print!("{HELP}");
        return Ok(());
    }
    match command.as_str() {
        "encode" => encode(&args[1..]),
        "verify-assets" => {
            let options = options(
                &args[1..],
                &["--pak", "--out", "--engine-dir"],
                &["--hats-only", "--characters-only"],
            )?;
            if options.contains_key("--hats-only") && options.contains_key("--characters-only") {
                return fail("--hats-only and --characters-only are mutually exclusive");
            }
            let report = verify::run(verify::Options {
                pak: options
                    .get("--pak")
                    .ok_or("--pak PATH is required; no implicit installed assets are read")?
                    .into(),
                out: options.get("--out").map(PathBuf::from),
                engine_dir: options.get("--engine-dir").map(PathBuf::from),
                hats_only: options.contains_key("--hats-only"),
                characters_only: options.contains_key("--characters-only"),
            })?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        _ => fail(format!("Unknown command {command}; use --help")),
    }
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hat_only_flags_are_rejected_for_every_outfit_before_reading_images() {
        for garment in ["pants", "hidingPants", "chest", "back"] {
            for flag in [
                "--name",
                "--description",
                "--rarity",
                "--remove-hair",
                "--hide-body",
            ] {
                let options = BTreeMap::from([(flag.to_owned(), String::new())]);
                assert!(hat_options(garment, &options).is_err());
            }
        }
        for flag in [
            "--profile",
            "--pants",
            "--body",
            "--front-sleeves",
            "--back-sleeves",
            "--back",
        ] {
            let options = BTreeMap::from([(flag.to_owned(), String::new())]);
            assert!(hat_options("hat", &options).is_err());
        }
    }
    #[test]
    fn hat_rarity_validates_keys_without_preescaping_metadata() {
        let options = BTreeMap::from([
            ("--name".into(), "A \"quote\" \\ 星".into()),
            ("--rarity".into(), "rare".into()),
            ("--remove-hair".into(), String::new()),
        ]);
        let metadata = hat_options("hat", &options).unwrap().unwrap();
        assert_eq!(metadata.shortdescription, "A \"quote\" \\ 星");
        assert!(metadata.remove_hair);
        assert!(
            hat_options(
                "hat",
                &BTreeMap::from([("--rarity".into(), "mythic".into())])
            )
            .is_err()
        );
    }
    #[test]
    fn shared_output_protects_input_canonical_and_hardlink_paths() {
        let directory = std::env::temp_dir().join(format!("outfit-output-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let input = directory.join("input.png");
        fs::write(&input, b"original").unwrap();
        let inputs = vec![input.clone()];
        assert!(write_output(Some(&input), &inputs, "replacement").is_err());
        let alias = directory.join("alias.png");
        fs::hard_link(&input, &alias).unwrap();
        #[cfg(unix)]
        assert!(write_output(Some(&alias), &inputs, "replacement").is_err());
        #[cfg(unix)]
        {
            let link = directory.join("symlink.png");
            std::os::unix::fs::symlink(&input, &link).unwrap();
            assert!(write_output(Some(&link), &inputs, "replacement").is_err());
        }
        assert_eq!(fs::read(&input).unwrap(), b"original");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn garment_cli_exports_auto_and_explicit_profile_with_complete_layers() {
        let directory =
            std::env::temp_dir().join(format!("outfit-garment-cli-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        for garment in [
            Garment::Pants,
            Garment::HidingPants,
            Garment::Chest,
            Garment::Back,
        ] {
            let sheets: &[(Sheet, &str)] = match garment {
                Garment::Pants | Garment::HidingPants => &[(Sheet::Pants, "--pants")],
                Garment::Chest => &[
                    (Sheet::Body, "--body"),
                    (Sheet::FrontSleeves, "--front-sleeves"),
                    (Sheet::BackSleeves, "--back-sleeves"),
                ],
                Garment::Back => &[(Sheet::Back, "--back")],
            };
            let mut images = Vec::new();
            let mut paths = Vec::new();
            for (index, &(sheet, _)) in sheets.iter().enumerate() {
                let layout = core::layout(sheet);
                let width = layout[0].len() * 43;
                let height = layout.len() * 43;
                let mut rgba = vec![0; width * height * 4];
                let (row, column) = layout
                    .iter()
                    .enumerate()
                    .find_map(|(row, columns)| {
                        columns
                            .iter()
                            .position(|id| !id.is_empty())
                            .map(|column| (row, column))
                    })
                    .unwrap();
                let offset = ((row * 43) * width + column * 43) * 4;
                rgba[offset..offset + 4].copy_from_slice(&[0x12, 0x34, 0x56, [255, 128, 1][index]]);
                let path = directory.join(format!("{}-{index}.png", garment.key()));
                let mut encoder = png::Encoder::new(
                    fs::File::create(&path).unwrap(),
                    width as u32,
                    height as u32,
                );
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                encoder
                    .write_header()
                    .unwrap()
                    .write_image_data(&rgba)
                    .unwrap();
                paths.push(path);
                images.push(core::RgbaImage {
                    width,
                    height,
                    rgba,
                });
            }
            let layers: Vec<_> = images
                .iter()
                .zip(sheets)
                .map(|(image, (sheet, _))| Layer {
                    image,
                    frames: core::layout(*sheet),
                })
                .collect();
            for (choice, argument) in [
                (ProfileChoice::Auto, None),
                (ProfileChoice::Compact, Some("compact")),
            ] {
                for strategy in [Strategy::Text, Strategy::Compressed, Strategy::Passes] {
                    let expected =
                        core::optimize_garment_native(&layers, garment, choice, true, strategy)
                            .unwrap();
                    let descriptor =
                        core::descriptor(garment, &expected.result.encoding.directives);
                    for format in ["directives", "json", "command"] {
                        let out = directory.join("output");
                        let mut args = vec![
                            "--garment".into(),
                            garment.key().into(),
                            "--strategy".into(),
                            strategy.key().into(),
                            "--format".into(),
                            format.into(),
                            "--out".into(),
                            out.to_string_lossy().into_owned(),
                        ];
                        for ((_, flag), path) in sheets.iter().zip(&paths) {
                            args.extend([(*flag).into(), path.to_string_lossy().into_owned()]);
                        }
                        if let Some(profile) = argument {
                            args.extend(["--profile".into(), profile.into()]);
                        }
                        encode(&args).unwrap();
                        let actual = fs::read_to_string(&out).unwrap();
                        let actual = actual.strip_suffix('\n').unwrap();
                        match format {
                            "directives" => assert_eq!(actual, expected.result.encoding.directives),
                            "json" => assert_eq!(
                                serde_json::from_str::<serde_json::Value>(actual).unwrap(),
                                descriptor
                            ),
                            _ => assert_eq!(actual, core::spawn_command(&descriptor).unwrap()),
                        }
                    }
                }
            }
        }
        fs::remove_dir_all(directory).unwrap();
    }
}
