use crate::{Result, fail, packed::PackedAssets};
use flate2::{Compression, write::ZlibEncoder};
use outfit_core::{
    self as core, FrameLayout, Garment, Layer, Profile, ProfileChoice, RgbaImage, Sheet, Strategy,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

const FRAME_SIZE: usize = 43;
const PROFILES: [Profile; 3] = [Profile::Original, Profile::Conservative, Profile::Compact];
#[derive(Serialize)]
struct Pose {
    name: String,
    id: u32,
    column: usize,
    row: usize,
}
struct LoadedLayer {
    path: String,
    frames_path: String,
    image: RgbaImage,
    frames: &'static FrameLayout,
    poses: Vec<Pose>,
}
struct Fixture {
    name: &'static str,
    kind: Garment,
    layers: Vec<Arc<LoadedLayer>>,
}
impl Fixture {
    fn layers(&self) -> Vec<Layer<'_>> {
        self.layers
            .iter()
            .map(|layer| Layer {
                image: &layer.image,
                frames: layer.frames,
            })
            .collect()
    }
}
fn dirname(path: &str) -> &str {
    path.rsplit_once('/')
        .map(|(dir, _)| if dir.is_empty() { "/" } else { dir })
        .unwrap_or("/")
}
fn resolve_asset(base: &str, name: &str) -> String {
    let joined = if name.starts_with('/') {
        name.to_owned()
    } else {
        format!("{base}/{name}")
    };
    let mut components = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                components.pop();
            }
            _ => components.push(part),
        }
    }
    format!("/{}", components.join("/"))
}
pub(crate) fn read_image(pak: &PackedAssets, path: &str) -> Result<RgbaImage> {
    core::decode_png(&pak.read(path)?).map_err(|error| format!("{path}: {error}").into())
}
fn load_layer(
    pak: &PackedAssets,
    item_path: &str,
    image_name: &Value,
    sheet: Sheet,
) -> Result<Arc<LoadedLayer>> {
    let name = image_name
        .as_str()
        .filter(|s| !s.is_empty() && !s.contains(['?', ':']))
        .ok_or_else(|| format!("Invalid item image reference in {item_path}"))?;
    let path = resolve_asset(dirname(item_path), name);
    let image = read_image(pak, &path)?;
    let frames = core::layout(sheet);
    if image.width != frames[0].len() * FRAME_SIZE || image.height != frames.len() * FRAME_SIZE {
        return fail(format!(
            "Whole sprite sheet does not match production layout: {path} ({}x{})",
            image.width, image.height
        ));
    }
    let basename = path.rsplit('/').next().unwrap_or("");
    let stem = basename.rsplit_once('.').map_or(basename, |(stem, _)| stem);
    let mut search = dirname(&path);
    let frames_path = loop {
        let found = [
            resolve_asset(search, &format!("{stem}.frames")),
            resolve_asset(search, "default.frames"),
        ]
        .into_iter()
        .find(|p| pak.entries.contains_key(&p.to_lowercase()));
        if let Some(found) = found {
            break found;
        }
        if search == "/" {
            return fail(format!("No actual frame specification found for {path}"));
        }
        search = dirname(search);
    };
    let spec = pak.json(&frames_path)?;
    let poses = poses_for_spec(&spec, frames, &path, &frames_path)?;
    Ok(Arc::new(LoadedLayer {
        path,
        frames_path,
        image,
        frames,
        poses,
    }))
}
fn poses_for_spec(
    spec: &Value,
    frames: &FrameLayout,
    path: &str,
    frames_path: &str,
) -> Result<Vec<Pose>> {
    let grid = spec.get("frameGrid").filter(|v| !v.is_null());
    if let Some(grid) = grid {
        let dimension = |i: usize| grid["dimensions"][i].as_u64();
        let begins_at_zero =
            |i: usize| grid["begin"][i].is_null() || grid["begin"][i].as_i64() == Some(0);
        if grid["size"][0] != FRAME_SIZE
            || grid["size"][1] != FRAME_SIZE
            || !begins_at_zero(0)
            || !begins_at_zero(1)
            || dimension(0).is_none_or(|n| n < frames[0].len() as u64)
            || dimension(1).is_none_or(|n| n < frames.len() as u64)
        {
            return fail(format!(
                "Actual .frames grid does not match production layout: {frames_path}"
            ));
        }
    } else if !spec["frameList"].is_object() {
        return fail(format!("No frameGrid or frameList in {frames_path}"));
    }
    let mut poses = Vec::new();
    for (row, ids) in frames.iter().enumerate() {
        for (column, id) in ids.iter().enumerate().filter(|(_, id)| !id.is_empty()) {
            let name = if let Some(grid) = grid {
                if grid.get("names").is_some_and(|names| !names.is_null()) {
                    grid["names"][row][column].as_str().map(str::to_owned)
                } else {
                    Some(
                        (row as u64
                            * grid["dimensions"][0]
                                .as_u64()
                                .ok_or("Invalid grid dimensions")?
                            + column as u64)
                            .to_string(),
                    )
                }
            } else {
                let rectangle = [
                    column * FRAME_SIZE,
                    row * FRAME_SIZE,
                    (column + 1) * FRAME_SIZE,
                    (row + 1) * FRAME_SIZE,
                ];
                spec["frameList"].as_object().and_then(|list| {
                    list.iter()
                        .find(|(_, value)| {
                            rectangle
                                .iter()
                                .enumerate()
                                .all(|(i, expected)| value[i].as_u64() == Some(*expected as u64))
                        })
                        .map(|(name, _)| name.clone())
                })
            }
            .filter(|s| !s.is_empty() && !s.contains(['?', ':']))
            .ok_or_else(|| {
                format!("No real pose name at {path} frame ({column},{row}), encoder ID {id}")
            })?;
            poses.push(Pose {
                name,
                id: u32::from_str_radix(id, 16)?,
                column,
                row,
            });
        }
    }
    Ok(poses)
}
fn fixtures(pak: &PackedAssets) -> Result<Vec<Fixture>> {
    let floran_path = "/items/armors/floran/floran-tier6accelerator/florantier6accelerator.legs";
    let frogg_path = "/items/armors/biome/swamp/froggmerchant/froggmerchant.legs";
    let avian_path = "/items/armors/avian/avian-tier6separator/aviantier6separator.chest";
    let tiger_path = "/items/armors/backitems/tigertail/tigertail.back";
    let floran = pak.json(floran_path)?;
    let frogg = pak.json(frogg_path)?;
    let avian = pak.json(avian_path)?;
    let tiger = pak.json(tiger_path)?;
    if frogg["hideBody"] != true {
        return fail("Expected actual Frogg item to declare hideBody:true");
    }
    let pants = load_layer(pak, floran_path, &floran["maleFrames"], Sheet::Pants)?;
    let hiding = load_layer(pak, frogg_path, &frogg["maleFrames"], Sheet::Pants)?;
    let male = load_layer(pak, avian_path, &avian["maleFrames"]["body"], Sheet::Body)?;
    let female = load_layer(pak, avian_path, &avian["femaleFrames"]["body"], Sheet::Body)?;
    let front = load_layer(
        pak,
        avian_path,
        &avian["maleFrames"]["frontSleeve"],
        Sheet::FrontSleeves,
    )?;
    let back = load_layer(
        pak,
        avian_path,
        &avian["maleFrames"]["backSleeve"],
        Sheet::BackSleeves,
    )?;
    if avian["femaleFrames"]["frontSleeve"] != avian["maleFrames"]["frontSleeve"]
        || avian["femaleFrames"]["backSleeve"] != avian["maleFrames"]["backSleeve"]
    {
        return fail(
            "Avian female sleeves differ; fixture specification must include both real sleeve sheets",
        );
    }
    let tail = load_layer(pak, tiger_path, &tiger["maleFrames"], Sheet::Back)?;
    Ok(vec![
        Fixture {
            name: "floran-pants",
            kind: Garment::Pants,
            layers: vec![pants],
        },
        Fixture {
            name: "frogg-hiding-pants",
            kind: Garment::HidingPants,
            layers: vec![hiding],
        },
        Fixture {
            name: "avian-male-body",
            kind: Garment::Chest,
            layers: vec![Arc::clone(&male)],
        },
        Fixture {
            name: "avian-female-body",
            kind: Garment::Chest,
            layers: vec![Arc::clone(&female)],
        },
        Fixture {
            name: "avian-front-sleeves",
            kind: Garment::Chest,
            layers: vec![Arc::clone(&front)],
        },
        Fixture {
            name: "avian-back-sleeves",
            kind: Garment::Chest,
            layers: vec![Arc::clone(&back)],
        },
        Fixture {
            name: "tiger-tail",
            kind: Garment::Back,
            layers: vec![tail],
        },
        Fixture {
            name: "avian-male-complete",
            kind: Garment::Chest,
            layers: vec![male, Arc::clone(&front), Arc::clone(&back)],
        },
        Fixture {
            name: "avian-female-complete",
            kind: Garment::Chest,
            layers: vec![female, front, back],
        },
    ])
}

// Intentionally independent of encoder color/address helpers: each pass is simultaneous.
pub(crate) fn decode_color(text: &str) -> Result<u32> {
    if ![3, 4, 6, 8].contains(&text.len()) || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return fail(format!("Invalid directive RGBA: {text}"));
    }
    let mut expanded = if text.len() <= 4 {
        text.chars().flat_map(|c| [c, c]).collect::<String>()
    } else {
        text.to_owned()
    };
    if expanded.len() == 6 {
        expanded.push_str("ff");
    }
    Ok(u32::from_str_radix(&expanded, 16)?)
}
fn replace_passes(suffix: &str) -> Result<Vec<HashMap<u32, u32>>> {
    if suffix.is_empty() {
        return Ok(Vec::new());
    }
    let suffix = suffix
        .strip_prefix('?')
        .ok_or("Directive suffix is not an operation list")?;
    suffix
        .split('?')
        .map(|operation| {
            let mut parts = operation.split(';');
            if parts.next() != Some("replace") {
                return fail(format!("Unexpected encoder operation {operation}"));
            }
            let mut replacements = HashMap::new();
            for pair in parts {
                let (source, target) = pair
                    .split_once('=')
                    .ok_or_else(|| format!("Invalid replacement {pair}"))?;
                if replacements
                    .insert(decode_color(source)?, decode_color(target)?)
                    .is_some()
                {
                    return fail(format!("Duplicate replacement source {source}"));
                }
            }
            Ok(replacements)
        })
        .collect()
}
fn verify_pixels(fixture: &Fixture, passes: &[HashMap<u32, u32>]) -> Result<(usize, usize)> {
    let mut coordinates = 0;
    let mut transparent = 0;
    for layer in &fixture.layers {
        for pose in &layer.poses {
            for top_y in 0..FRAME_SIZE {
                for x in 0..FRAME_SIZE {
                    let y = FRAME_SIZE - 1 - top_y;
                    let source = ((x as u32) << 24) | (pose.id << 16) | ((y as u32) << 8);
                    let offset = ((pose.row * FRAME_SIZE + top_y) * layer.image.width
                        + pose.column * FRAME_SIZE
                        + x)
                        * 4;
                    let rgba = &layer.image.rgba[offset..offset + 4];
                    let expected = if rgba[3] == 0 {
                        transparent += 1;
                        source
                    } else {
                        u32::from_be_bytes(rgba.try_into()?)
                    };
                    let actual = passes.iter().fold(source, |value, pass| {
                        pass.get(&value).copied().unwrap_or(value)
                    });
                    if actual != expected {
                        return fail(format!(
                            "{}: {}:{} pixel ({x},{y}) expected RGBA {expected:08x}, got {actual:08x}",
                            fixture.name, layer.path, pose.name
                        ));
                    }
                    coordinates += 1;
                }
            }
        }
    }
    Ok((coordinates, transparent))
}
pub(crate) fn measured_deflate_bytes(text: &str) -> Result<usize> {
    let mut compressor = ZlibEncoder::new(Vec::new(), Compression::new(6));
    compressor.write_all(text.as_bytes())?;
    Ok(compressor.finish()?.len())
}

fn verify_references(pak: &PackedAssets) -> Result<Vec<Value>> {
    let definitions = [
        (
            "originalVertical",
            "/monsters/boss/apeboss/apeboss.png",
            1263,
            394,
            1,
            2,
        ),
        (
            "conservativeVertical",
            "/ships/glitch/glitchT5blocks.png",
            60,
            38,
            1,
            2,
        ),
        (
            "originalHorizontal",
            "/dungeons/other/wreck/key.png",
            755,
            29,
            2,
            2,
        ),
        (
            "compactHorizontal",
            "/items/active/unsorted/translocator/translocatordiscunlit.png",
            2,
            0,
            2,
            2,
        ),
    ];
    let mut samples = Vec::new();
    for (index, (name, path, x, y, width, height)) in definitions.into_iter().enumerate() {
        let image = read_image(pak, path)?;
        if x + width > image.width || y + height > image.height {
            return fail(format!("Reference sample out of bounds: {path}"));
        }
        let mut pixels = Vec::new();
        for dy in 0..height {
            for dx in 0..width {
                let offset = ((image.height - 1 - y - dy) * image.width + x + dx) * 4;
                let rgba = &image.rgba[offset..offset + 4];
                let expected = if width == 1 {
                    if dy == 0 {
                        [255, 255, 0, 255]
                    } else {
                        [255, 255, 255, 255]
                    }
                } else if dx == 0 {
                    [0, 255, 255, 255]
                } else {
                    [255, 255, 255, 255]
                };
                let channels = if index == 3 { 3 } else { 4 };
                if rgba[..channels] != expected[..channels] {
                    return fail(format!(
                        "Reference prerequisite failed: {path} sample ({dx},{dy})"
                    ));
                }
                pixels.push(rgba.to_vec());
            }
        }
        samples.push(json!({"name":name,"path":path,"x":x,"y":y,"width":width,"height":height,"pixels":pixels,"pngBytes":pak.entries[&path.to_lowercase()].size,"decodedRgbaBytes":image.rgba.len()}));
    }
    for kind in [
        Garment::Pants,
        Garment::HidingPants,
        Garment::Chest,
        Garment::Back,
    ] {
        for profile in PROFILES {
            let operations: Vec<_> = core::template(kind, profile)
                .trim_start_matches('?')
                .split('?')
                .collect();
            let replace = operations
                .iter()
                .find(|op| op.starts_with("replace;"))
                .ok_or("Missing frame recognition")?;
            let values: Vec<_> = replace
                .split(';')
                .skip(1)
                .map(|pair| {
                    decode_color(
                        pair.split_once('=')
                            .ok_or("Invalid template replacement")?
                            .1,
                    )
                })
                .collect::<Result<_>>()?;
            if kind == Garment::Chest {
                if !operations.contains(&"multiply=2eff2e00") {
                    return fail("Compact alpha prerequisite missing");
                }
            } else if values.iter().any(|value| value & 255 != 0) {
                return fail("Frame recognition does not erase alpha");
            }
            let vertical = &samples[if profile == Profile::Original { 0 } else { 1 }];
            let horizontal = &samples[if profile == Profile::Compact { 3 } else { 2 }];
            for sample in [vertical, horizontal] {
                let operation = format!(
                    "blendmult={};{};{}",
                    sample["path"].as_str().ok_or("Missing reference path")?,
                    sample["x"],
                    sample["y"]
                );
                if !operations.contains(&operation.as_str()) {
                    return fail(format!(
                        "Unexpected profile references: {}/{}",
                        kind.key(),
                        profile.key()
                    ));
                }
            }
        }
    }
    Ok(samples)
}

pub struct Options {
    pub pak: PathBuf,
    pub out: Option<PathBuf>,
    pub engine_dir: Option<PathBuf>,
    pub hats_only: bool,
    pub characters_only: bool,
}
pub fn run(options: Options) -> Result<Value> {
    if options.hats_only && options.characters_only {
        return fail("--hats-only and --characters-only are mutually exclusive");
    }
    let pak = PackedAssets::open(&options.pak)?;
    let outfit_enabled = !options.hats_only && !options.characters_only;
    let references = if !outfit_enabled {
        Vec::new()
    } else {
        verify_references(&pak)?
    };
    let fixtures = if !outfit_enabled {
        Vec::new()
    } else {
        fixtures(&pak)?
    };
    let hats = if options.characters_only {
        crate::hat_verify::Verification {
            cases: Vec::new(),
            files: Vec::new(),
            report: Value::Null,
        }
    } else {
        crate::hat_verify::run(&pak)?
    };
    let characters = if options.hats_only {
        crate::character_verify::Verification {
            cases: Vec::new(),
            files: Vec::new(),
            report: Value::Null,
            variants: Value::Null,
        }
    } else {
        crate::character_verify::run(&pak)?
    };
    let mut measurements = Vec::new();
    let mut cases = Vec::new();
    let mut total_coordinates = 0usize;
    let mut garment_measurements = Vec::new();
    let mut garment_candidates = Vec::new();
    let mut garment_coordinates = 0usize;
    for fixture in &fixtures {
        let layers = fixture.layers();
        let original_prefix = core::template(fixture.kind, Profile::Original);
        let baseline = core::encode(&layers, original_prefix, false)?;
        // Also independently validate the baseline used by the game probes.
        verify_pixels(
            fixture,
            &replace_passes(
                baseline
                    .directives
                    .strip_prefix(original_prefix)
                    .ok_or("Baseline omitted prefix")?,
            )?,
        )?;
        let mut variants = Vec::new();
        for profile in PROFILES {
            let prefix = core::template(fixture.kind, profile);
            let direct = core::optimize_native(&layers, prefix, false, Strategy::Text)?;
            let automatic = core::optimize_native(&layers, prefix, true, Strategy::Text)?;
            let optimized = core::optimize_native(&layers, prefix, true, Strategy::Compressed)?;
            let no_palette = core::optimize_native(&layers, prefix, false, Strategy::Compressed)?;
            let passes_only = core::optimize_native(&layers, prefix, true, Strategy::Passes)?;
            if optimized.estimated_deflate_bytes
                > direct
                    .estimated_deflate_bytes
                    .min(automatic.estimated_deflate_bytes)
                || no_palette.estimated_deflate_bytes > direct.estimated_deflate_bytes
            {
                return fail(format!(
                    "Compressed strategy exceeded an available baseline: {}/{}",
                    fixture.name,
                    profile.key()
                ));
            }
            if direct.encoding.directives != core::encode(&layers, prefix, false)?.directives
                || automatic.encoding.directives != core::encode(&layers, prefix, true)?.directives
            {
                return fail("Text strategy changed existing encoder output");
            }
            for (name, palette, result) in [
                ("off", "off", &direct),
                ("auto", "auto", &automatic),
                ("text", "auto", &automatic),
                ("compressed", "auto", &optimized),
                ("passes", "off", &passes_only),
                ("compressed-no-palette", "off", &no_palette),
            ] {
                let encoding = &result.encoding;
                let suffix = encoding
                    .directives
                    .strip_prefix(prefix)
                    .ok_or("Production optimizer omitted requested prefix")?;
                let passes = replace_passes(suffix)?;
                let (coordinates, transparent) = verify_pixels(fixture, &passes)?;
                if encoding.output_bytes != encoding.directives.len() {
                    return fail(
                        "Production outputBytes does not match measured complete directives",
                    );
                }
                if result.estimated_deflate_bytes != measured_deflate_bytes(&encoding.directives)? {
                    return fail(
                        "Production DEFLATE estimate does not match independent native zlib measurement",
                    );
                }
                if palette == "off" && encoding.palette_applied {
                    return fail("Palette-disabled strategy generated aliases");
                }
                if name == "passes" && passes.len() != 1 {
                    return fail("Passes strategy generated more than one pixel stage");
                }
                total_coordinates += coordinates;
                measurements.push(json!({"fixture":fixture.name,"profile":profile.key(),"palette":palette,"variant":name,"strategy":result.strategy.key(),"candidateCount":result.candidate_count,"estimatedDeflateBytes":result.estimated_deflate_bytes,"fullDirectiveBytes":encoding.directives.len(),"originalBaselineBytes":baseline.directives.len(),"replacePassCount":passes.len(),"replacementCount":passes.iter().map(HashMap::len).sum::<usize>(),"encodedCoordinates":coordinates,"untouchedTransparentCoordinates":transparent,"pixelCount":encoding.pixel_count,"colorCount":encoding.color_count,"factoredColorCount":encoding.factored_color_count,"paletteApplied":encoding.palette_applied}));
                variants.push(json!({"name":format!("{}/{}",profile.key(),name),"directives":encoding.directives}));
            }
        }
        // Compressed exposes every complete palette allocation and safe pair order,
        // including the raw families reduced to one winner by text/passes.
        for choice in [
            ProfileChoice::Original,
            ProfileChoice::Conservative,
            ProfileChoice::Compact,
            ProfileChoice::Auto,
        ] {
            for palette in [false, true] {
                for strategy in [Strategy::Text, Strategy::Compressed, Strategy::Passes] {
                    let set =
                        core::garment_candidates(&layers, fixture.kind, choice, palette, strategy)?;
                    if set.candidate_count > set.candidate_limit {
                        return fail("Garment candidate search exceeded its declared bound");
                    }
                    let mut estimates = Vec::with_capacity(set.candidates.len());
                    for (index, candidate) in set.candidates.iter().enumerate() {
                        let encoding = &candidate.encoding;
                        let prefix = core::template(fixture.kind, candidate.profile);
                        let suffix = encoding
                            .directives
                            .strip_prefix(prefix)
                            .ok_or("Garment candidate omitted its declared helper prefix")?;
                        let passes = replace_passes(suffix)?;
                        let (coordinates, transparent) = verify_pixels(fixture, &passes)?;
                        let estimate = measured_deflate_bytes(&encoding.directives)?;
                        if encoding.output_bytes != encoding.directives.len() {
                            return fail(
                                "Garment candidate outputBytes does not match its directives",
                            );
                        }
                        if (!palette || strategy == Strategy::Passes) && encoding.palette_applied {
                            return fail("Palette-disabled garment candidate generated aliases");
                        }
                        if strategy == Strategy::Passes && passes.len() != 1 {
                            return fail("Garment passes strategy changed the pixel-stage count");
                        }
                        garment_coordinates += coordinates;
                        estimates.push(estimate);
                        garment_candidates.push(json!({
                            "fixture":fixture.name,"requestedProfile":choice.key(),"palette":palette,
                            "strategy":strategy.key(),"candidate":index,"selectedProfile":candidate.profile.key(),
                            "fullDirectiveBytes":encoding.output_bytes,"estimatedDeflateBytes":estimate,
                            "replacePassCount":passes.len(),"paletteApplied":encoding.palette_applied,
                            "factoredColorCount":encoding.factored_color_count,"encodedCoordinates":coordinates,
                            "untouchedTransparentCoordinates":transparent
                        }));
                    }
                    let winner = core::select_garment(set, &estimates, strategy)?;
                    let production = core::optimize_garment_native(
                        &layers,
                        fixture.kind,
                        choice,
                        palette,
                        strategy,
                    )?;
                    if production != winner {
                        return fail(
                            "Garment native optimizer disagrees with independently scored candidate selection",
                        );
                    }
                    let result = &winner.result;
                    garment_measurements.push(json!({
                        "fixture":fixture.name,"requestedProfile":choice.key(),"palette":palette,
                        "strategy":strategy.key(),"selectedProfile":winner.profile.key(),
                        "candidateCount":result.candidate_count,"profileCount":winner.profile_count,
                        "candidateLimit":winner.candidate_limit,"fullDirectiveBytes":result.encoding.output_bytes,
                        "estimatedDeflateBytes":result.estimated_deflate_bytes
                    }));
                    if choice == ProfileChoice::Auto && palette {
                        variants.push(json!({
                            "name":format!("auto/{}",strategy.key()),"directives":result.encoding.directives,
                            "selectedProfile":winner.profile.key(),"candidateCount":result.candidate_count
                        }));
                    }
                }
            }
        }
        cases.push(json!({"name":fixture.name,"originalPrefix":original_prefix,"baseline":baseline.directives,"variants":variants,"layers":fixture.layers.iter().map(|layer| json!({"path":layer.path,"framesPath":layer.frames_path,"poses":layer.poses})).collect::<Vec<_>>()}));
    }
    let report = json!({"format":1,"packageEntries":pak.entries.len(),"packageBytes":pak.file_bytes,"indexBytes":pak.index_bytes,"fixtureCount":fixtures.len(),"ordinaryFixtureCount":if outfit_enabled {7} else {0},"completeChestFixtureCount":if outfit_enabled {2} else {0},"measuredVariants":measurements.len(),"verifiedCoordinates":total_coordinates,"references":references,"measurements":measurements,"hats":hats.report,"characters":characters.report,"evidence":"Independent simultaneous-replace RGBA model on every mapped coordinate, including untouched alpha-zero RGB. estimatedDeflateBytes is native flate2 zlib level-6 deflate of complete UTF-8 directives, not network packet bytes; candidate selection may differ from browser CompressionStream. Native game evidence requires separately running the emitted fixture. No vanilla-binary claim."});
    let mut report = report;
    report["garmentSearch"] = json!({
        "measurements":garment_measurements,"candidates":garment_candidates,
        "verifiedCandidates":garment_candidates.len(),"verifiedCoordinates":garment_coordinates,
        "mappedFrames":fixtures.iter().flat_map(|fixture| &fixture.layers).map(|layer| layer.poses.len()).sum::<usize>(),
        "evidence":"Independent full-RGBA simultaneous-replacement CPU model at every mapped coordinate for each emitted fixed-profile and auto search candidate, both palette settings and all strategies. Compressed enumerates all palette allocations and safe orders; text/passes reduce the same raw candidate family before exposure. This is not vanilla engine full-RGBA readback."
    });
    if let Some(directory) = options.engine_dir {
        let mut files = hats.files;
        files.extend(characters.files);
        emit_fixture(
            &directory,
            &json!({"format":1,"frameSize":FRAME_SIZE,"cases":cases,"hats":hats.cases,"characters":characters.cases,"characterVariants":characters.variants}),
            &files,
        )?;
    }
    if let Some(out) = options.out {
        write_report(&out, &pak.path, &report)?;
    }
    Ok(report)
}

fn effective_path(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return Ok(fs::canonicalize(path)?);
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    Ok(fs::canonicalize(parent)?.join(path.file_name().ok_or("Missing output filename")?))
}
fn write_report(path: &Path, pak: &Path, report: &Value) -> Result<()> {
    let target = effective_path(path)?;
    if target == fs::canonicalize(pak)? {
        return fail("Refusing to overwrite the source pak");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(target_meta) = fs::metadata(&target) {
            let pak_meta = fs::metadata(pak)?;
            if target_meta.dev() == pak_meta.dev() && target_meta.ino() == pak_meta.ino() {
                return fail("Refusing to overwrite the source pak");
            }
        }
    }
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(target)?;
    serde_json::to_writer_pretty(&mut file, report)?;
    file.write_all(b"\n")?;
    Ok(())
}
fn emit_fixture(
    directory: &Path,
    manifest: &Value,
    synthetic_pngs: &[(String, Vec<u8>)],
) -> Result<()> {
    let mut names = std::collections::HashSet::new();
    for (name, _) in synthetic_pngs {
        if !(name.starts_with("hat-expected-") || name.starts_with("character-expected-"))
            || !name.ends_with(".png")
            || Path::new(name).components().count() != 1
            || !matches!(
                Path::new(name).components().next(),
                Some(std::path::Component::Normal(_))
            )
            || !names.insert(name)
        {
            return fail("Invalid or duplicate private expectation PNG filename");
        }
    }
    let parent = directory
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let target = effective_path(directory)?;
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("Invalid compiled repository path")?;
    let root = if source_root.exists() {
        fs::canonicalize(source_root)?
    } else {
        source_root.to_owned()
    };
    if target.starts_with(root) {
        return fail("--engine-dir must be outside the repository");
    }
    if target.exists() && fs::read_dir(&target)?.next().is_some() {
        return fail("--engine-dir must be a new or empty private directory");
    }
    fs::create_dir_all(&target)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o700))?;
    }
    let metadata = json!({"name":"outfit-studio-private-verification","friendlyName":"Private OpenStarbound outfit verification (not vanilla proof)","scripts":{"onLoad":["/engine-verify.lua"]}});
    let files = [
        ("_metadata", serde_json::to_vec_pretty(&metadata)?),
        ("outfit-verification.json", serde_json::to_vec(manifest)?),
        (
            "engine-verify.lua",
            include_bytes!("../../../tools/engine-verify.lua").to_vec(),
        ),
        (
            "vanilla-verify.lua",
            include_bytes!("../../../tools/vanilla-verify.lua").to_vec(),
        ),
    ];
    for (name, bytes) in files
        .iter()
        .map(|(name, bytes)| (*name, bytes.as_slice()))
        .chain(
            synthetic_pngs
                .iter()
                .map(|(name, bytes)| (name.as_str(), bytes.as_slice())),
        )
    {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(target.join(name))?.write_all(bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
