//! Private character fixtures derived from the legal package, never distributed assets.
use crate::{Result, fail, packed::PackedAssets, verify};
use outfit_core::{
    self as core, CharacterFrame, CharacterLayer, CharacterPart, RgbaImage, Strategy,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub(crate) struct Verification {
    pub cases: Vec<Value>,
    pub files: Vec<(String, Vec<u8>)>,
    pub report: Value,
    pub variants: Value,
}

struct SourceCoverage {
    paths: Vec<(String, u8)>,
    references: Vec<Value>,
}
const SIZE: usize = 43;
const PARTS: [CharacterPart; 5] = [
    CharacterPart::Body,
    CharacterPart::FrontArm,
    CharacterPart::BackArm,
    CharacterPart::Head,
    CharacterPart::Hair,
];

// The sample depends on the declared address, not production color/pixel helpers.
// Different IDs have different colors, while intentionally shared IDs agree.
fn desired_pixel(id: u8, x: usize, y: usize) -> [u8; 4] {
    match (x + y * 3) % 11 {
        0..=3 => [x as u8, id, y as u8, 0],
        4 => [id, 53, 109, 255],
        5 => [197, id, 149, 128],
        6 => [13, 29, id, 1],
        7 => [255, 255, 255, 255],
        8 => [0, 0, 0, 255],
        _ => [71, 93, 119, 255],
    }
}
fn desired(id: u8, suppressed: bool) -> RgbaImage {
    let mut rgba = Vec::with_capacity(SIZE * SIZE * 4);
    for top_y in 0..SIZE {
        for x in 0..SIZE {
            let y = SIZE - 1 - top_y;
            let pixel = if suppressed {
                [x as u8, 0, y as u8, 0]
            } else {
                desired_pixel(id, x, y)
            };
            rgba.extend_from_slice(&pixel);
        }
    }
    RgbaImage {
        width: SIZE,
        height: SIZE,
        rgba,
    }
}
fn sheet(part: CharacterPart) -> Result<RgbaImage> {
    let layout = core::character_layout(part);
    let mut image = RgbaImage {
        width: layout.width,
        height: layout.height,
        rgba: vec![0; layout.width * layout.height * 4],
    };
    for frame in &layout.frames {
        if frame.x + SIZE > image.width || frame.y + SIZE > image.height {
            return fail("Character layout exceeds sheet bounds");
        }
        for top_y in 0..SIZE {
            for x in 0..SIZE {
                let offset = ((frame.y + top_y) * image.width + frame.x + x) * 4;
                let mut pixel = desired_pixel(frame.id, x, SIZE - 1 - top_y);
                // Exercise hidden input RGB, which must not leak into the output.
                if pixel[3] == 0 {
                    pixel = [101, 151, 199, 0];
                }
                image.rgba[offset..offset + 4].copy_from_slice(&pixel);
            }
        }
    }
    Ok(image)
}

// The humanoid definitions use shared documents in /humanoid; hair has a local
// default document. These paths are checked against the supplied legal package.
fn frame_document(pak: &PackedAssets, path: &str) -> Result<(String, Value)> {
    let filename = path
        .rsplit('/')
        .next()
        .and_then(|s| s.strip_suffix(".png"))
        .ok_or("Invalid character PNG path")?;
    let specific = format!(
        "{}.frames",
        path.strip_suffix(".png").ok_or("Invalid character PNG")?
    );
    let shared = format!("/humanoid/{filename}.frames");
    let directory = path.rsplit_once('/').ok_or("Invalid character source")?.0;
    for candidate in [specific, shared, format!("{directory}/default.frames")] {
        if pak.entries.contains_key(&candidate.to_lowercase()) {
            return Ok((candidate.clone(), pak.json(&candidate)?));
        }
    }
    fail(format!("No actual frame document for {path}"))
}
fn original_frames(spec: &Value) -> Result<BTreeMap<String, (usize, usize)>> {
    let mut result = BTreeMap::new();
    if let Some(list) = spec["frameList"].as_object() {
        for (name, rectangle) in list {
            let rectangle = rectangle
                .as_array()
                .filter(|v| v.len() == 4)
                .ok_or("Invalid character frame rectangle")?;
            let bounds = rectangle
                .iter()
                .map(|v| {
                    v.as_u64()
                        .and_then(|n| usize::try_from(n).ok())
                        .ok_or("Invalid character frame bound")
                })
                .collect::<Result<Vec<_>, _>>()?;
            if bounds[2].checked_sub(bounds[0]) != Some(SIZE)
                || bounds[3].checked_sub(bounds[1]) != Some(SIZE)
                || name.is_empty()
                || name.contains(['?', ':'])
            {
                return fail("Character frameList rectangle/name is not a valid 43px frame");
            }
            result.insert(name.clone(), (bounds[0], bounds[1]));
        }
    } else {
        let grid = &spec["frameGrid"];
        if grid["size"] != json!([43, 43])
            || grid["begin"]
                .as_array()
                .is_some_and(|v| v != &vec![json!(0), json!(0)])
        {
            return fail("Character source does not use a zero-origin 43px grid");
        }
        let rows = grid["names"]
            .as_array()
            .ok_or("Character source frame grid has no names")?;
        for (y, row) in rows.iter().enumerate() {
            for (x, name) in row
                .as_array()
                .ok_or("Invalid character frame row")?
                .iter()
                .enumerate()
            {
                if name.is_null() {
                    continue;
                }
                let name = name
                    .as_str()
                    .filter(|s| !s.is_empty() && !s.contains(['?', ':']))
                    .ok_or("Invalid original character frame name")?;
                if result.insert(name.into(), (x * SIZE, y * SIZE)).is_some() {
                    return fail("Duplicate original character frame name");
                }
            }
        }
    }
    let empty = serde_json::Map::new();
    let aliases = spec["aliases"].as_object().unwrap_or(&empty);
    for (alias, _) in aliases {
        if alias.is_empty() || alias.contains(['?', ':']) || result.contains_key(alias) {
            return fail("Invalid/duplicate character frame alias");
        }
        let mut target = alias.as_str();
        let mut visited = HashSet::new();
        while let Some(value) = aliases.get(target) {
            if !visited.insert(target) {
                return fail("Character frame alias cycle");
            }
            target = value
                .as_str()
                .ok_or("Invalid character frame alias target")?;
        }
        let rectangle = *result
            .get(target)
            .ok_or("Character alias has no real target frame")?;
        result.insert(alias.clone(), rectangle);
    }
    Ok(result)
}

fn sources(pak: &PackedAssets, part: CharacterPart) -> Result<SourceCoverage> {
    let layout = core::character_layout(part);
    let mut by_path: BTreeMap<String, Vec<(&CharacterFrame, String)>> = BTreeMap::new();
    for frame in &layout.frames {
        for source in &frame.source_paths {
            let (path, canonical) = source
                .rsplit_once(':')
                .ok_or("Character source must include its original frame name")?;
            for name in std::iter::once(canonical).chain(frame.aliases.iter().map(String::as_str)) {
                by_path
                    .entry(path.to_owned())
                    .or_default()
                    .push((frame, name.to_owned()));
            }
        }
    }
    if by_path.is_empty() {
        return fail("Character layout has no real sources");
    }
    let mut paths = Vec::new();
    let mut references = Vec::new();
    for (path, frames) in by_path {
        let image = verify::read_image(pak, &path)?;
        let (document, spec) = frame_document(pak, &path)?;
        let actual = original_frames(&spec)?;
        let mut covered = BTreeSet::new();
        for (frame, name) in frames {
            let (x, y) = *actual
                .get(&name)
                .ok_or_else(|| format!("Missing actual original frame {path}:{name}"))?;
            if x + SIZE > image.width || y + SIZE > image.height || !covered.insert(name.clone()) {
                return fail(format!(
                    "Invalid/duplicate actual character frame {path}:{name}"
                ));
            }
            // Hair deliberately redraws the same static image on normal/climb,
            // whose original frameList rectangles have different top-down y.
            if part != CharacterPart::Hair && (x, y) != (frame.x, frame.y) {
                return fail(format!(
                    "Character layout disagrees with actual source rectangle {path}:{name}"
                ));
            }
            paths.push((format!("{path}:{name}"), frame.id));
        }
        if covered != actual.keys().cloned().collect() {
            return fail(format!(
                "Character layout silently omits original frames/aliases from {path}"
            ));
        }
        references.push(json!({"part":part.key(),"path":path,"framesPath":document,"width":image.width,"height":image.height,"namedFramesIncludingAliases":covered.len()}));
    }
    Ok(SourceCoverage { paths, references })
}

fn suppressed_sources(pak: &PackedAssets) -> Result<SourceCoverage> {
    let mut by_path: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for source in core::character_suppressed_sources() {
        let (path, name) = source
            .rsplit_once(':')
            .ok_or("Invalid suppressed character source")?;
        if !by_path.entry(path.into()).or_default().insert(name.into()) {
            return fail("Duplicate suppressed character source");
        }
    }
    if by_path.is_empty() {
        return fail("Character body omits original head suppression sources");
    }
    let mut paths = Vec::new();
    let mut references = Vec::new();
    for (path, names) in by_path {
        let image = verify::read_image(pak, &path)?;
        let (document, spec) = frame_document(pak, &path)?;
        let actual = original_frames(&spec)?;
        if names != actual.keys().cloned().collect() {
            return fail(format!("Suppressed head omits real frame/alias: {path}"));
        }
        for name in names {
            let (x, y) = actual[&name];
            if x + SIZE > image.width || y + SIZE > image.height {
                return fail("Suppressed head frame exceeds actual PNG");
            }
            paths.push((format!("{path}:{name}"), 0));
        }
        references.push(json!({"part":"suppressedHead","path":path,"framesPath":document,"width":image.width,"height":image.height,"namedFramesIncludingAliases":actual.len()}));
    }
    Ok(SourceCoverage { paths, references })
}

// Model only the post-canvas simultaneous replacement phases. The real prefix's
// source fingerprint and floating-point canvas behavior remain native gates.
fn verify_candidate(
    ids: &BTreeSet<u8>,
    prefix: &str,
    directives: &str,
    suppress_head: bool,
) -> Result<usize> {
    let suffix = directives
        .strip_prefix(prefix)
        .ok_or("Character candidate changed its fixed fingerprint prefix")?;
    let mut phases = Vec::new();
    for operation in suffix.split('?').filter(|s| !s.is_empty()) {
        let pairs = operation
            .strip_prefix("replace;")
            .ok_or("Unexpected character pixel operation")?;
        let mut map = HashMap::new();
        for pair in pairs.split(';') {
            let (source, target) = pair
                .split_once('=')
                .ok_or("Malformed character replacement")?;
            if map
                .insert(verify::decode_color(source)?, verify::decode_color(target)?)
                .is_some()
            {
                return fail("Duplicate canonical character replacement source");
            }
        }
        phases.push(map);
    }
    for &id in ids {
        for y in 0..SIZE {
            for x in 0..SIZE {
                let mut actual = u32::from_be_bytes([x as u8, id, y as u8, 0]);
                for phase in &phases {
                    if let Some(&target) = phase.get(&actual) {
                        actual = target;
                    }
                }
                let expected = if suppress_head && id == 0 {
                    u32::from_be_bytes([x as u8, 0, y as u8, 0])
                } else {
                    u32::from_be_bytes(desired_pixel(id, x, y))
                };
                if actual != expected {
                    return fail(format!(
                        "Character model id={id} ({x},{y}) expected {expected:08x}, got {actual:08x}"
                    ));
                }
            }
        }
    }
    Ok(phases.len())
}

fn variants(layers: &[CharacterLayer<'_>]) -> Result<(Vec<Value>, Vec<Value>)> {
    let mut selected = HashSet::new();
    let mut variants = Vec::new();
    let mut measurements = Vec::new();
    let mut checked = HashSet::new();
    let mut ids: BTreeSet<_> = layers
        .iter()
        .flat_map(|layer| {
            core::character_layout(layer.part)
                .frames
                .iter()
                .map(|frame| frame.id)
        })
        .collect();
    if layers[0].part == CharacterPart::Body {
        ids.insert(0);
    }
    let prefix = core::character_prefix(layers[0].part);
    for (name, palette, strategy) in [
        ("direct", false, Strategy::Text),
        ("palette", true, Strategy::Text),
        ("compressed", true, Strategy::Compressed),
        ("passes", true, Strategy::Passes),
    ] {
        let candidates = core::character_candidates(layers, palette, strategy)?;
        for candidate in &candidates {
            if checked.insert(candidate.directives.clone()) {
                verify_candidate(
                    &ids,
                    prefix,
                    &candidate.directives,
                    layers[0].part == CharacterPart::Body,
                )?;
            }
        }
        let result = core::optimize_character_native(layers, palette, strategy)?;
        if !candidates
            .iter()
            .any(|c| c.directives == result.encoding.directives)
        {
            return fail("Character optimizer selected a non-candidate");
        }
        if strategy == Strategy::Passes
            && verify_candidate(
                &ids,
                prefix,
                &result.encoding.directives,
                layers[0].part == CharacterPart::Body,
            )? != 1
        {
            return fail("Character passes strategy must have exactly one pixel replacement phase");
        }
        if result.encoding.output_bytes != result.encoding.directives.len()
            || result.estimated_deflate_bytes
                != verify::measured_deflate_bytes(&result.encoding.directives)?
        {
            return fail("Character encoding byte/deflate measurement mismatch");
        }
        if (!palette || strategy == Strategy::Passes) && result.encoding.palette_applied {
            return fail("Character encoder ignored palette setting");
        }
        measurements.push(json!({"variant":name,"candidateCount":result.candidate_count,"fullDirectiveBytes":result.encoding.output_bytes,"estimatedDeflateBytes":result.estimated_deflate_bytes,"paletteApplied":result.encoding.palette_applied}));
        if selected.insert(result.encoding.directives.clone()) {
            variants.push(json!({"name":name,"directives":result.encoding.directives}));
        }
    }
    Ok((variants, measurements))
}

pub(crate) fn run(pak: &PackedAssets) -> Result<Verification> {
    let images = PARTS
        .iter()
        .copied()
        .map(sheet)
        .collect::<Result<Vec<_>>>()?;
    let body_layers: Vec<_> = PARTS[..3]
        .iter()
        .zip(&images[..3])
        .map(|(&part, image)| CharacterLayer { part, image })
        .collect();
    let (body_variants, body_measurements) = variants(&body_layers)?;
    let (head_variants, head_measurements) = variants(&[CharacterLayer {
        part: CharacterPart::Head,
        image: &images[3],
    }])?;
    let (hair_variants, hair_measurements) = variants(&[CharacterLayer {
        part: CharacterPart::Hair,
        image: &images[4],
    }])?;
    let mut cases = Vec::new();
    let mut files = Vec::new();
    let mut references = Vec::new();
    let mut expected_paths: HashMap<(u8, bool), String> = HashMap::new();
    let mut modeled_coordinates = 0usize;
    for part in PARTS {
        let SourceCoverage {
            mut paths,
            references: source_references,
        } = sources(pak, part)?;
        references.extend(source_references);
        if part == CharacterPart::Body {
            let SourceCoverage {
                paths: suppressed,
                references: suppressed_references,
            } = suppressed_sources(pak)?;
            paths.extend(suppressed);
            references.extend(suppressed_references);
        }
        let variant_group = match part {
            CharacterPart::Head => "head",
            CharacterPart::Hair => "hair",
            _ => "body",
        };
        for (source, id) in paths {
            let suppressed = part == CharacterPart::Body && id == 0;
            let image = desired(id, suppressed);
            let expected_name = if let Some(name) = expected_paths.get(&(id, suppressed)) {
                name.clone()
            } else {
                let name = format!(
                    "character-expected-{id}-{}.png",
                    if suppressed { "suppressed" } else { "drawn" }
                );
                files.push((name.clone(), crate::hat_verify::encode_png(&image)?));
                expected_paths.insert((id, suppressed), name.clone());
                name
            };
            let mut colors = BTreeSet::new();
            let mut transparent = Vec::new();
            for y in 0..SIZE {
                for x in 0..SIZE {
                    let rgba = if suppressed {
                        [x as u8, 0, y as u8, 0]
                    } else {
                        desired_pixel(id, x, y)
                    };
                    if rgba[3] != 0 {
                        colors.insert(format!("{:08x}", u32::from_be_bytes(rgba)));
                    } else if transparent.len() < 3 {
                        transparent.push(json!({"x":x,"y":y,"rgba":rgba}));
                    }
                }
            }
            modeled_coordinates += SIZE * SIZE;
            cases.push(json!({"name":format!("character/{}/{}",part.key(),source),"part":part.key(),"frameId":id,"width":SIZE,"height":SIZE,"sourcePaths":[source],"expectedPath":format!("/{expected_name}"),"colors":colors,"transparent":transparent,"variantGroup":variant_group}));
        }
    }
    let report = json!({"fixtureCount":cases.len(),"modeledFrameCoordinates":modeled_coordinates,"references":references,"measurements":{"body":body_measurements,"head":head_measurements,"hair":hair_measurements},"evidence":"Independent simultaneous-replace exact RGBA model for every distinct optimization candidate, with synthetic desired artwork per address and complete legal-package frame-name/alias coverage. All source frames require native exact-color masks, occupancy and transparent-address probes; OpenStarbound additionally compares every RGBA channel. Prefix fingerprint/canvas semantics require actual native execution. No vanilla-binary pass is claimed by generation."});
    Ok(Verification {
        cases,
        files,
        report,
        variants: json!({"body":body_variants,"head":head_variants,"hair":hair_variants}),
    })
}
