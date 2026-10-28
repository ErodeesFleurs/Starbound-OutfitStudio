//! Independently derived vanilla/OpenSB Floran fingerprints and address sheets.
//! Input frames are separate render layers, never copies of an entire humanoid.
use crate::{
    Encoding, Error, OptimizedEncoding, RgbaImage, Strategy, encoder::ScanBuilder, optimizer,
};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::{collections::HashMap, fmt::Write, str::FromStr, sync::LazyLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CharacterPart {
    Body,
    FrontArm,
    BackArm,
    Head,
    Hair,
}
impl CharacterPart {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Body => "body",
            Self::FrontArm => "frontArm",
            Self::BackArm => "backArm",
            Self::Head => "head",
            Self::Hair => "hair",
        }
    }
}
impl FromStr for CharacterPart {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self, Error> {
        match value {
            "body" => Ok(Self::Body),
            "frontArm" => Ok(Self::FrontArm),
            "backArm" => Ok(Self::BackArm),
            "head" => Ok(Self::Head),
            "hair" => Ok(Self::Hair),
            _ => Err(Error::new(format!("Unknown character part: {value}"))),
        }
    }
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterFrame {
    pub name: String,
    /// Pixel origin in the top-down PNG, not directive coordinates.
    pub x: usize,
    pub y: usize,
    /// Equal IDs MUST have equal visible artwork, including fractional alpha.
    pub id: u8,
    pub aliases: Vec<String>,
    /// Complete original asset references, including the canonical frame name.
    pub source_paths: Vec<String>,
}
#[derive(Debug, Deserialize)]
pub struct CharacterLayout {
    pub width: usize,
    pub height: usize,
    pub frames: Vec<CharacterFrame>,
}
#[derive(Clone, Copy, Debug)]
pub struct CharacterLayer<'a> {
    pub part: CharacterPart,
    pub image: &'a RgbaImage,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CharacterFingerprint {
    source_path: String,
    vanilla: String,
    opensb: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CharacterData {
    layouts: HashMap<String, CharacterLayout>,
    body_prefix: String,
    head_prefix: String,
    suppressed_sources: Vec<String>,
    fingerprints: Vec<CharacterFingerprint>,
}
static DATA: LazyLock<CharacterData> = LazyLock::new(|| {
    let mut data = serde_json::from_str(include_str!("../data/character.json"))
        .expect("invalid built-in character constants");
    validate_fingerprints(&data).expect("invalid built-in cross-engine character fingerprints");
    // Keep native provenance in the distributed table, not in the live encoder.
    data.fingerprints = Vec::new();
    data
});

// A shared directive cannot distinguish two sources with the same fingerprint.
// Check BOTH engines in one namespace before trusting any replacement table:
// HashMap::insert alone would silently turn a collision into the wrong pose.
fn validate_fingerprints(data: &CharacterData) -> Result<(), Error> {
    let mut sources = HashMap::new();
    for (part, layout) in &data.layouts {
        if part == "hair" {
            continue;
        }
        for frame in &layout.frames {
            for source in &frame.source_paths {
                if sources
                    .insert(source.as_str(), (part == "head", frame.id))
                    .is_some()
                {
                    return Err(Error::new("Duplicate character fingerprint source"));
                }
            }
        }
    }
    for source in &data.suppressed_sources {
        if sources.insert(source.as_str(), (false, 0)).is_some() {
            return Err(Error::new("Duplicate suppressed character source"));
        }
    }
    let mut prefixes = HashMap::new();
    for (head, prefix) in [(false, &data.body_prefix), (true, &data.head_prefix)] {
        let stage = prefix
            .split('?')
            .find_map(|s| s.strip_prefix("replace;"))
            .ok_or_else(|| Error::new("Character prefix has no fingerprint replacements"))?;
        for pair in stage.split(';') {
            let (source, target) = pair
                .split_once('=')
                .ok_or_else(|| Error::new("Invalid character fingerprint replacement"))?;
            let source = fingerprint_color(source)?;
            let target = fingerprint_color(target)?.to_be_bytes();
            if target[0] != 255 || target[2..] != [255, 0] {
                return Err(Error::new("Invalid character fingerprint address seed"));
            }
            if prefixes.insert((head, source), target[1]).is_some() {
                return Err(Error::new("Duplicate character fingerprint replacement"));
            }
        }
    }
    let mut observed = HashMap::new();
    for fingerprint in &data.fingerprints {
        let (head, id) = sources
            .remove(fingerprint.source_path.as_str())
            .ok_or_else(|| {
                Error::new("Unknown or duplicate native character fingerprint source")
            })?;
        for color in [&fingerprint.vanilla, &fingerprint.opensb] {
            let key = (head, fingerprint_color(color)?);
            if let Some(previous) = observed.insert(key, id)
                && previous != id
            {
                return Err(Error::new(format!(
                    "Cross-engine character fingerprint collision at {}: IDs {previous} and {id}",
                    fingerprint.source_path
                )));
            }
            if prefixes.get(&key) != Some(&id) {
                return Err(Error::new(format!(
                    "Character prefix does not map native fingerprint {color} to ID {id} at {}",
                    fingerprint.source_path
                )));
            }
        }
    }
    if !sources.is_empty() {
        return Err(Error::new(
            "Native character fingerprint coverage is incomplete",
        ));
    }
    Ok(())
}
fn fingerprint_color(color: &str) -> Result<u32, Error> {
    if color.len() != 8 {
        return Err(Error::new(
            "Character fingerprint must contain four explicit RGBA bytes",
        ));
    }
    u32::from_str_radix(color, 16).map_err(|_| Error::new("Invalid character fingerprint RGBA"))
}
const HAIR_PREFIX: &str = "?multiply=00000000?replace;0000=fff?crop;0;0;2;2?blendmult=/items/active/weapons/protectorate/aegisaltpistol/beamend.png;0;0?replace;a355c0a5=00000000;a355c07b=ff000000;ffffffa5=0000ff00;ffffff7b=ff00ff00?scale=256;256?crop;1;1;44;44";
pub fn character_layout(part: CharacterPart) -> &'static CharacterLayout {
    &DATA.layouts[part.key()]
}
pub fn character_prefix(part: CharacterPart) -> &'static str {
    match part {
        CharacterPart::Body | CharacterPart::FrontArm | CharacterPart::BackArm => &DATA.body_prefix,
        CharacterPart::Head => &DATA.head_prefix,
        CharacterPart::Hair => HAIR_PREFIX,
    }
}
pub fn character_suppressed_sources() -> &'static [String] {
    &DATA.suppressed_sources
}

fn pixel(image: &RgbaImage, frame: &CharacterFrame, x: usize, y: usize) -> [u8; 4] {
    let offset = ((frame.y + y) * image.width + frame.x + x) * 4;
    image.rgba[offset..offset + 4]
        .try_into()
        .expect("validated character image")
}
fn equal_art(image: &RgbaImage, a: &CharacterFrame, b: &CharacterFrame) -> bool {
    (0..43).all(|y| {
        (0..43).all(|x| {
            let a = pixel(image, a, x, y);
            let b = pixel(image, b, x, y);
            (a[3] == 0 && b[3] == 0) || a == b
        })
    })
}
/// Validate a single 43 × 43 head or a vanilla 86 × 215 head sheet.
/// Vanilla sheets contribute only their normal frame at (43, 0).
pub fn validate_character_head(image: &RgbaImage) -> Result<(), Error> {
    if !matches!((image.width, image.height), (43, 43) | (86, 215))
        || image.rgba.len() != image.width * image.height * 4
    {
        return Err(Error::new(
            "Base head must be exactly 43 × 43 or 86 × 215 RGBA pixels",
        ));
    }
    Ok(())
}

/// Compose the base head below each valid emote using straight-alpha source-over.
/// All arithmetic rounds to the nearest integer; unused sheet cells stay clear.
pub fn compose_character_head(head: &RgbaImage, emotes: &RgbaImage) -> Result<RgbaImage, Error> {
    validate_character_head(head)?;
    validate_character_layer(CharacterLayer {
        part: CharacterPart::Head,
        image: emotes,
    })?;
    let layout = character_layout(CharacterPart::Head);
    let mut output = RgbaImage {
        width: layout.width,
        height: layout.height,
        rgba: vec![0; layout.width * layout.height * 4],
    };
    let head_x = if head.width == 86 { 43 } else { 0 };
    for frame in &layout.frames {
        for y in 0..43 {
            for x in 0..43 {
                let destination = ((frame.y + y) * layout.width + frame.x + x) * 4;
                let source = (y * head.width + head_x + x) * 4;
                let below = &head.rgba[source..source + 4];
                let above = &emotes.rgba[destination..destination + 4];
                let result = &mut output.rgba[destination..destination + 4];
                match above[3] {
                    0 => result.copy_from_slice(below),
                    255 => result.copy_from_slice(above),
                    alpha => {
                        let sa = u32::from(alpha);
                        let da = u32::from(below[3]);
                        let inverse = 255 - sa;
                        let numerator = sa * 255 + da * inverse;
                        for channel in 0..3 {
                            let color = u32::from(above[channel]) * sa * 255
                                + u32::from(below[channel]) * da * inverse;
                            result[channel] = ((color + numerator / 2) / numerator) as u8;
                        }
                        result[3] = ((numerator + 127) / 255) as u8;
                    }
                }
            }
        }
    }
    Ok(output)
}

/// Reject painted cells that cannot be rendered, and conflicting shared poses.
/// Transparent RGB is not scrubbed; the encoder deliberately leaves unpainted
/// pixels as their hidden address RGBA, just like the outfit/hat encoders.
pub fn validate_character_layer(layer: CharacterLayer<'_>) -> Result<(), Error> {
    let layout = character_layout(layer.part);
    let image = layer.image;
    if (image.width, image.height) != (layout.width, layout.height)
        || image.rgba.len() != layout.width * layout.height * 4
    {
        return Err(Error::new(format!(
            "{} sheet must be exactly {} × {} RGBA pixels",
            layer.part.key(),
            layout.width,
            layout.height
        )));
    }
    let mut used = [[false; 9]; 8];
    for frame in &layout.frames {
        used[frame.y / 43][frame.x / 43] = true;
    }
    for y in 0..image.height {
        for x in 0..image.width {
            if image.rgba[(y * image.width + x) * 4 + 3] != 0 && !used[y / 43][x / 43] {
                return Err(Error::new(format!(
                    "{} pixel ({x}, {y}) is in an unused cell and cannot be rendered; use the layout reference",
                    layer.part.key()
                )));
            }
        }
    }
    let mut ids: [Option<&CharacterFrame>; 256] = [None; 256];
    for frame in &layout.frames {
        if let Some(previous) = ids[usize::from(frame.id)].replace(frame)
            && !equal_art(image, previous, frame)
        {
            return Err(Error::new(format!(
                "{} poses {} and {} share fingerprint ID {}; their visible RGBA artwork must be identical",
                layer.part.key(),
                previous.name,
                frame.name,
                frame.id
            )));
        }
    }
    Ok(())
}

/// Optimize one directive field: the complete body/arm trio, head/emotes, or hair.
/// Body and head exports are linked by `identity_patch`, since native heads are
/// erased by the body prefix and the redrawn head lives in the emote layer.
pub fn character_candidates(
    layers: &[CharacterLayer<'_>],
    palette: bool,
    strategy: Strategy,
) -> Result<Vec<Encoding>, Error> {
    let mut parts = [false; 5];
    for &layer in layers {
        if std::mem::replace(&mut parts[layer.part as usize], true) {
            return Err(Error::new("Character parts cannot be supplied twice"));
        }
        validate_character_layer(layer)?;
    }
    let body = [
        CharacterPart::Body,
        CharacterPart::FrontArm,
        CharacterPart::BackArm,
    ];
    let part = if layers.len() == 3 && body.iter().all(|&p| parts[p as usize]) {
        CharacterPart::Body
    } else if layers.len() == 1
        && matches!(layers[0].part, CharacterPart::Head | CharacterPart::Hair)
    {
        layers[0].part
    } else {
        return Err(Error::new(
            "Encode all three body/legs, front arm and back arm sheets together; encode head/emotes or hair as separate directive fields",
        ));
    };
    let mut scan = ScanBuilder::new();
    for layer in layers {
        let mut seen = [false; 256];
        for frame in &character_layout(layer.part).frames {
            if std::mem::replace(&mut seen[usize::from(frame.id)], true) {
                continue;
            }
            for x in 0..43 {
                for y in 0..43 {
                    let rgba = pixel(layer.image, frame, x, 42 - y);
                    if rgba[3] == 0 {
                        continue;
                    }
                    scan.count_pixels(1);
                    scan.pair(
                        crate::encoder::Source::Address(
                            (x as u32) << 24 | u32::from(frame.id) << 16 | (y as u32) << 8,
                        ),
                        u32::from_be_bytes(rgba),
                    );
                }
            }
        }
    }
    Ok(optimizer::scan_candidates(
        &scan.finish(),
        character_prefix(part),
        palette,
        strategy,
    ))
}
pub fn optimize_character_native(
    layers: &[CharacterLayer<'_>],
    palette: bool,
    strategy: Strategy,
) -> Result<OptimizedEncoding, Error> {
    optimizer::optimize_candidates_native(
        character_candidates(layers, palette, strategy)?,
        strategy,
    )
}

const PATCH_FIELDS: &[&str] = &[
    "bodyDirectives",
    "emoteDirectives",
    "hairDirectives",
    "hairGroup",
    "hairType",
    "facialHairDirectives",
    "facialHairGroup",
    "facialHairType",
    "facialMaskDirectives",
    "facialMaskGroup",
    "facialMaskType",
    "imagePath",
];
fn checked_patch(patch: &Value) -> Result<&Map<String, Value>, Error> {
    let patch = patch
        .as_object()
        .ok_or_else(|| Error::new("Identity patch must be a JSON object"))?;
    for (key, value) in patch {
        if !PATCH_FIELDS.contains(&key.as_str())
            || !(value.is_string() || (key == "imagePath" && value.is_null()))
        {
            return Err(Error::new(format!(
                "Identity patch field {key} must be a supported art string (only imagePath permits null removal)"
            )));
        }
    }
    if patch.is_empty() {
        return Err(Error::new(
            "Identity patch must contain at least one edited field",
        ));
    }
    Ok(patch)
}
fn string_field<'a>(identity: &'a Value, key: &str) -> Result<&'a str, Error> {
    identity.get(key).and_then(Value::as_str).ok_or_else(|| {
        Error::new(format!(
            "Baseline identity requires string field {key}; paste /identity get output"
        ))
    })
}
fn asset_component(value: &str, name: &str) -> Result<(), Error> {
    if value.is_empty()
        || value
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
        || value
            .bytes()
            .any(|b| !(b.is_ascii_alphanumeric() || b"_-/".contains(&b)))
    {
        return Err(Error::new(format!(
            "Baseline {name} is not a safe vanilla resource path: {value}"
        )));
    }
    Ok(())
}
/// Clone an untouched static hair/facial layer through a valid Floran base.
/// This uses an absolute vanilla asset reference, NOT unproven ../ traversal.
/// White multiplication retains all original RGBA, including hidden RGB.
fn preserve_layer(
    identity: &Value,
    patch: &mut Map<String, Value>,
    root: &str,
    group: &str,
    kind: &str,
    directives: &str,
) -> Result<(), Error> {
    if root == "floran" {
        return Ok(());
    }
    let source_type = string_field(identity, kind)?;
    if source_type.is_empty() {
        return Ok(());
    }
    let source_group = string_field(identity, group)?;
    asset_component(source_group, group)?;
    asset_component(source_type, kind)?;
    let previous = string_field(identity, directives)?;
    patch.insert(group.into(), "hair".into());
    patch.insert(kind.into(), "1".into());
    patch.insert(directives.into(), format!("?multiply=00000000?replace;0000=fff?blendmult=/humanoid/{root}/{source_group}/{source_type}.png:normal{previous}").into());
    Ok(())
}
/// Return only edited identity fields. Species, gender, name, personality,
/// colors, animation parameters and every other non-art field remain untouched.
/// Every export requires a baseline identity. Hair-only preserves its existing
/// nonempty resource base; the complete redraw selects a coherent Floran base.
pub fn identity_patch(
    identity: &Value,
    body: Option<&str>,
    head: Option<&str>,
    hair: Option<&str>,
) -> Result<Value, Error> {
    if !identity.is_object() {
        return Err(Error::new("Baseline identity must be a JSON object"));
    }
    if body.is_some() != head.is_some() {
        return Err(Error::new(
            "Body/arms and head/emotes must be edited together: body directives clear the original head",
        ));
    }
    if body.is_none() && hair.is_none() {
        return Err(Error::new(
            "Select the complete body/head group or hair before exporting",
        ));
    }
    for value in [body, head, hair].into_iter().flatten() {
        if value.is_empty() {
            return Err(Error::new("Edited character directives cannot be empty"));
        }
    }
    let mut patch = Map::new();
    if let (Some(body), Some(head)) = (body, head) {
        let species = string_field(identity, "species")?;
        let gender = string_field(identity, "gender")?;
        if !matches!(gender, "male" | "female") {
            return Err(Error::new(
                "Baseline identity gender must be male or female",
            ));
        }
        let root = identity
            .get("imagePath")
            .and_then(Value::as_str)
            .unwrap_or(species);
        asset_component(root, "imagePath/species")?;
        if hair.is_none() {
            preserve_layer(
                identity,
                &mut patch,
                root,
                "hairGroup",
                "hairType",
                "hairDirectives",
            )?;
        }
        preserve_layer(
            identity,
            &mut patch,
            root,
            "facialHairGroup",
            "facialHairType",
            "facialHairDirectives",
        )?;
        preserve_layer(
            identity,
            &mut patch,
            root,
            "facialMaskGroup",
            "facialMaskType",
            "facialMaskDirectives",
        )?;
        patch.insert("imagePath".into(), "floran".into());
        patch.insert("bodyDirectives".into(), body.into());
        patch.insert("emoteDirectives".into(), head.into());
    }
    if let Some(hair) = hair {
        if body.is_some() {
            patch.insert("hairGroup".into(), "hair".into());
            patch.insert("hairType".into(), "1".into());
        } else if string_field(identity, "hairType")?.is_empty() {
            return Err(Error::new(
                "Hair-only redraw needs an existing nonempty valid hair base; use the complete body/head group to select the fixed Floran base",
            ));
        }
        patch.insert("hairDirectives".into(), hair.into());
    }
    Ok(Value::Object(patch))
}
pub fn apply_identity_patch(identity: &Value, patch: &Value) -> Result<Value, Error> {
    let identity = identity
        .as_object()
        .ok_or_else(|| Error::new("Baseline identity must be a JSON object"))?;
    let patch = checked_patch(patch)?;
    let mut result = Map::with_capacity(identity.len() + patch.len());
    for (key, value) in identity {
        let value = match patch.get(key) {
            Some(Value::Null) => continue,
            Some(replacement) => replacement,
            None => value,
        };
        result.insert(key.clone(), value.clone());
    }
    for (key, value) in patch {
        if !value.is_null() && !identity.contains_key(key) {
            result.insert(key.clone(), value.clone());
        }
    }
    Ok(Value::Object(result))
}
fn push_lua_string(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\0'..='\u{1f}' | '\u{7f}' => {
                write!(out, "\\{:03}", u32::from(c)).expect("String write");
            }
            _ => out.push(c),
        }
    }
    out.push('"');
}
/// One atomic OpenSB LOCAL /run command; not a vanilla-server
/// command, not a destructive /identity set {object}. No new game assets needed.
pub fn identity_command(patch: &Value) -> Result<String, Error> {
    const BEGIN: &str = "/run local identity=player.humanoidIdentity();";
    const END: &str = " player.setHumanoidIdentity(identity)";
    let patch = checked_patch(patch)?;
    let capacity = BEGIN.len()
        + END.len()
        + patch
            .iter()
            .map(|(key, value)| {
                " identity[\"\"]=\"\";".len() + key.len() + value.as_str().map_or(3, str::len)
            })
            .sum::<usize>();
    let mut command = String::with_capacity(capacity);
    command.push_str(BEGIN);
    for (key, value) in patch {
        command.push_str(" identity[");
        push_lua_string(&mut command, key);
        command.push_str("]=");
        if value.is_null() {
            command.push_str("nil");
        } else {
            push_lua_string(&mut command, value.as_str().expect("checked patch string"));
        }
        command.push(';');
    }
    command.push_str(END);
    Ok(command)
}

#[cfg(test)]
mod fingerprint_tests {
    use super::*;

    fn table() -> CharacterData {
        serde_json::from_str(include_str!("../data/character.json")).unwrap()
    }

    #[test]
    fn cross_engine_collision_cannot_silently_overwrite_another_pose() {
        let mut data = table();
        // ID 2 in OpenSB collides with ID 1 in vanilla, despite each engine
        // having an otherwise valid, internally unique mapping.
        data.fingerprints[2].opensb = data.fingerprints[0].vanilla.clone();
        let error = validate_fingerprints(&data).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Cross-engine character fingerprint collision")
        );
    }

    #[test]
    fn missing_and_duplicate_measurements_are_not_accepted_as_coverage() {
        let mut data = table();
        data.fingerprints.pop();
        assert!(
            validate_fingerprints(&data)
                .unwrap_err()
                .to_string()
                .contains("coverage is incomplete")
        );
        let mut data = table();
        data.fingerprints[1].source_path = data.fingerprints[0].source_path.clone();
        assert!(
            validate_fingerprints(&data)
                .unwrap_err()
                .to_string()
                .contains("duplicate native character fingerprint source")
        );
    }
}
