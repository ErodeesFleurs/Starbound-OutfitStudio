use super::{Result, fail, write_output};
use outfit_core::{self as core, RecolorLayer};
use std::{fs, path::PathBuf};

const HELP: &str = "Outfit Studio — direct RGBA recoloring (not equipment or identity)

Usage:
  outfit recolor --source ORIGINAL.png --target EDITED.png [more pairs] [options]

Inputs (local only; no package or installed game required):
  --source PATH  Repeat for each original PNG layer/frame crop or merged frame sheet
  --target PATH  Repeat for each edited PNG, in the same order as --source
  The nth source pairs with the nth target, even if flags are grouped together.
  Provide at least one pair and equal source/target counts. Each pair must have
  identical nonzero dimensions; different pairs may have different dimensions.
Options:
  --out PATH     Write pure directives instead of stdout (never overwrite inputs)
  --stats        Print JSON mapping statistics to stderr

Application and limits:
  Output is only a single simultaneous ?replace, not a /spawnitem command,
  equipment descriptor, identity patch, arbitrary drawing or coordinate canvas.
  Identical original/edited images produce an empty string (one output newline).
  All occurrences of each original RGBA must map to exactly one target RGBA across
  EVERY pair. Two-color swaps are simultaneous, not cascaded. Conflicts name both
  layers and top-left, zero-based pixel coordinates; no fallback is performed.
  Alpha-zero hidden RGB and alpha-one pixels are included exactly, not discarded.
  Alpha/silhouette changes are allowed but can reveal hidden RGB and change every
  matching pixel. Editing software must preserve hidden RGB in original inputs.
  Supply the actual original effective frames/crops for every affected animation
  and shared layer, including climb frames where present. Unprovided pixels are
  NOT validated: the same color elsewhere will also change. These directives must
  be applied to those exact original image sources, not a redraw-mode canvas.
  This mode alone is not proof of game animation, equipment or identity setup.
";

#[derive(Default)]
struct Options {
    sources: Vec<PathBuf>,
    targets: Vec<PathBuf>,
    out: Option<PathBuf>,
    stats: bool,
}

fn parse(args: &[String]) -> Result<Options> {
    let mut options = Options::default();
    let mut arguments = args.iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--source" | "--target" | "--out" => {
                if argument == "--out" && options.out.is_some() {
                    return fail("Duplicate option --out");
                }
                let value = arguments
                    .next()
                    .filter(|value| !value.starts_with("--"))
                    .ok_or_else(|| format!("{argument} requires a value"))?;
                let path = PathBuf::from(value);
                match argument.as_str() {
                    "--source" => options.sources.push(path),
                    "--target" => options.targets.push(path),
                    _ => options.out = Some(path),
                }
            }
            "--stats" => {
                if options.stats {
                    return fail("Duplicate option --stats");
                }
                options.stats = true;
            }
            _ => return fail(format!("Unknown option {argument}; use recolor --help")),
        }
    }
    if options.sources.len() != options.targets.len() {
        return fail(format!(
            "Recolor requires equal --source and --target counts, got {} source(s) and {} target(s)",
            options.sources.len(),
            options.targets.len()
        ));
    }
    if options.sources.is_empty() {
        return fail("At least one --source PNG --target PNG pair is required");
    }
    Ok(options)
}

pub fn run(args: &[String]) -> Result<()> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print!("{HELP}");
        return Ok(());
    }
    let options = parse(args)?;
    let images = options
        .sources
        .iter()
        .zip(&options.targets)
        .enumerate()
        .map(|(index, (source, target))| -> Result<_> {
            let decode = |path: &PathBuf, role: &str| -> Result<_> {
                let context = || format!("Layer {} {role} {}", index + 1, path.display());
                let bytes = fs::read(path).map_err(|error| format!("{}: {error}", context()))?;
                core::decode_png(&bytes).map_err(|error| format!("{}: {error}", context()).into())
            };
            Ok((decode(source, "source")?, decode(target, "target")?))
        })
        .collect::<Result<Vec<_>>>()?;
    let layers = images
        .iter()
        .map(|(source, target)| RecolorLayer { source, target })
        .collect::<Vec<_>>();
    let directives = core::recolor_directives(&layers)?;
    let inputs = options
        .sources
        .into_iter()
        .chain(options.targets)
        .collect::<Vec<_>>();
    write_output(options.out.as_deref(), &inputs, &directives)?;
    if options.stats {
        let pixels = images.iter().try_fold(0usize, |total, (source, _)| {
            total
                .checked_add(source.rgba.len() / 4)
                .ok_or("Pixel count overflow")
        })?;
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "mode": "recolor",
                "layerCount": layers.len(),
                "pixelCount": pixels,
                "mappingCount": directives.bytes().filter(|byte| *byte == b';').count(),
                "outputBytes": directives.len(),
                "unchanged": directives.is_empty(),
                "coverage": "only supplied source/target pixels; include every affected frame and shared layer",
                "alpha": "full RGBA including alpha-zero hidden RGB; alpha/silhouette changes are allowed"
            }))?
        );
    }
    Ok(())
}
