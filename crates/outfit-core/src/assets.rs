use crate::garment_fingerprints::{self, FingerprintData};
use crate::{Error, FrameLayout, Garment, HatMetadata, Profile, Sheet};
use serde::Deserialize;
use serde_json::Value;
use std::{collections::HashMap, fmt::Write, sync::LazyLock};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Garments<T> {
    pants: T,
    hiding_pants: T,
    chest: T,
    back: T,
}
impl<T> Garments<T> {
    fn get(&self, garment: Garment) -> &T {
        match garment {
            Garment::Pants => &self.pants,
            Garment::HidingPants => &self.hiding_pants,
            Garment::Chest => &self.chest,
            Garment::Back => &self.back,
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Layouts {
    pants: FrameLayout,
    chest: FrameLayout,
    front_sleeves: FrameLayout,
    back_sleeves: FrameLayout,
    back: FrameLayout,
}
#[derive(Deserialize)]
struct Legacy {
    templates: Garments<Vec<String>>,
    layouts: Layouts,
    descriptors: Garments<Value>,
}
struct Profiles {
    original: String,
    conservative: String,
    compact: String,
}
struct Assets {
    layouts: Layouts,
    descriptors: Garments<Value>,
    profiles: Garments<Profiles>,
}
fn source_families(
    garment: Garment,
    layouts: &Layouts,
) -> Vec<(Sheet, &FrameLayout, &'static str)> {
    match garment {
        Garment::Pants => vec![(
            Sheet::Pants,
            &layouts.pants,
            "/items/armors/floran/floran-tier6accelerator/pants.png",
        )],
        Garment::HidingPants => vec![(
            Sheet::Pants,
            &layouts.pants,
            "/items/armors/biome/swamp/froggmerchant/pantsm.png",
        )],
        Garment::Back => vec![(
            Sheet::Back,
            &layouts.back,
            "/items/armors/backitems/tigertail/back.png",
        )],
        Garment::Chest => vec![
            (
                Sheet::Body,
                &layouts.chest,
                "/items/armors/avian/avian-tier6separator/chestm.png",
            ),
            (
                Sheet::Body,
                &layouts.chest,
                "/items/armors/avian/avian-tier6separator/chestf.png",
            ),
            (
                Sheet::FrontSleeves,
                &layouts.front_sleeves,
                "/items/armors/avian/avian-tier6separator/fsleeve.png",
            ),
            (
                Sheet::BackSleeves,
                &layouts.back_sleeves,
                "/items/armors/avian/avian-tier6separator/bsleeve.png",
            ),
        ],
    }
}
static ASSETS: LazyLock<Assets> = LazyLock::new(|| {
    // This is a checked-in generated constant, never user-controlled data.
    let legacy: Legacy = serde_json::from_str(include_str!("../data/legacy.json"))
        .expect("invalid built-in legacy constants");
    let observations: FingerprintData =
        serde_json::from_str(include_str!("../data/garment-fingerprints.json"))
            .expect("invalid built-in native garment observations");
    let mut observations_by_garment = HashMap::new();
    for group in observations.garments {
        assert!(
            observations_by_garment
                .insert(group.garment, group)
                .is_none(),
            "duplicate built-in native garment group"
        );
    }
    let mut profiles = |garment| {
        let group = observations_by_garment
            .remove(&garment)
            .expect("missing built-in native garment group");
        let original = garment_fingerprints::augment(
            &legacy.templates.get(garment).concat(),
            &group,
            &source_families(garment, &legacy.layouts),
        )
        .expect("invalid built-in cross-engine garment fingerprints");
        let conservative = original.replacen(
            "/monsters/boss/apeboss/apeboss.png;1263;394",
            "/ships/glitch/glitchT5blocks.png;60;38",
            1,
        );
        let compact = conservative.replacen(
            "/dungeons/other/wreck/key.png;755;29",
            "/items/active/unsorted/translocator/translocatordiscunlit.png;2;0",
            1,
        );
        Profiles {
            original,
            conservative,
            compact,
        }
    };
    let profiles = Garments {
        pants: profiles(Garment::Pants),
        hiding_pants: profiles(Garment::HidingPants),
        chest: profiles(Garment::Chest),
        back: profiles(Garment::Back),
    };
    Assets {
        layouts: legacy.layouts,
        descriptors: legacy.descriptors,
        profiles,
    }
});

pub fn layout(sheet: Sheet) -> &'static FrameLayout {
    let layouts = &ASSETS.layouts;
    match sheet {
        Sheet::Pants => &layouts.pants,
        Sheet::Body => &layouts.chest,
        Sheet::FrontSleeves => &layouts.front_sleeves,
        Sheet::BackSleeves => &layouts.back_sleeves,
        Sheet::Back => &layouts.back,
    }
}
pub fn template(garment: Garment, profile: Profile) -> &'static str {
    let profiles = ASSETS.profiles.get(garment);
    match profile {
        Profile::Original => &profiles.original,
        Profile::Conservative => &profiles.conservative,
        Profile::Compact => &profiles.compact,
    }
}
pub fn descriptor(garment: Garment, directives: &str) -> Value {
    let mut descriptor = ASSETS.descriptors.get(garment).clone();
    descriptor["parameters"]["directives"] = Value::String(directives.to_owned());
    descriptor
}

/// Hatter item fields are retained, but metadata is real JSON text: only
/// `spawn_command` performs the chat processor's additional escaping.
pub fn hat_descriptor(directives: &str, metadata: &HatMetadata) -> Value {
    serde_json::json!({
        "name": if metadata.hide_body { "frogghead" } else { "eyepatchhead" },
        "count": 1,
        "parameters": {
            "directives": directives,
            "description": metadata.description,
            "femaleFrames": "head.png",
            "inventoryIcon": "head.png",
            "maleFrames": "head.png",
            "mask": if metadata.remove_hair {
                "?submask=/items/armors/decorative/hats/eyepatch/mask.png"
            } else {
                "mask.png"
            },
            "maxStack": 1,
            "price": 0,
            "rarity": metadata.rarity.key(),
            "shortdescription": metadata.shortdescription,
            "statusEffects": [],
            "tooltipKind": "armor"
        }
    })
}

fn array_index(key: &str) -> Option<u32> {
    if key.is_empty()
        || !key.bytes().all(|byte| byte.is_ascii_digit())
        || (key.len() > 1 && key.starts_with('0'))
    {
        return None;
    }
    key.parse::<u32>().ok().filter(|&value| value != u32::MAX)
}

fn number_string(number: &serde_json::Number) -> String {
    // serde_json numbers are finite; preserve JS's number rendering (including
    // integer-valued doubles, exponent thresholds and negative zero).
    let mut buffer = ryu_js::Buffer::new();
    match number.as_f64() {
        Some(value) => buffer.format(value).to_owned(),
        None => number.to_string(),
    }
}

fn quoted(output: &mut String, value: &str, escape_values: bool) {
    output.push('"');
    for ch in value.chars() {
        match ch {
            // escapeObject adds a backslash first; JSON.stringify escapes both.
            '\\' if escape_values => output.push_str("\\\\\\\\"),
            '"' if escape_values => output.push_str("\\\\\\\""),
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            ch if ch < ' ' => {
                let _ = write!(output, "\\u{:04x}", ch as u32);
            }
            ch => output.push(ch),
        }
    }
    output.push('"');
}

fn escaped_json(output: &mut String, value: &Value) -> Result<(), Error> {
    match value {
        // Legacy escapeObject cannot traverse null. Report it rather than
        // panicking or silently producing a different command.
        Value::Null => return Err(Error::new("Spawn command parameters cannot contain null")),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) => output.push_str(&number_string(value)),
        Value::String(value) => quoted(output, value, true),
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                escaped_json(output, value)?;
            }
            output.push(']');
        }
        Value::Object(values) => {
            output.push('{');
            let mut entries: Vec<_> = values.iter().collect();
            entries
                .sort_by_key(|(key, _)| array_index(key).map_or((true, 0), |value| (false, value)));
            for (index, (key, value)) in entries.into_iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                quoted(output, key, false);
                output.push(':');
                escaped_json(output, value)?;
            }
            output.push('}');
        }
    }
    Ok(())
}

/// Match the production legacy two-level escaping, without mutating metadata.
pub fn spawn_command(descriptor: &Value) -> Result<String, Error> {
    let object = descriptor
        .as_object()
        .ok_or_else(|| Error::new("Spawn command descriptor must be an object"))?;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("perfectlygenericitem");
    let count = object
        .get("count")
        .and_then(Value::as_number)
        .map_or_else(|| "1".to_owned(), number_string);
    let mut command = format!("/spawnitem {name} {count}");
    if let Some(parameters) = object.get("parameters") {
        if parameters.as_str().is_some_and(|value| !value.is_empty()) {
            return Err(Error::new(
                "Spawn command parameters must not be a nonempty string",
            ));
        }
        let mut json = String::new();
        escaped_json(&mut json, parameters)?;
        command.push_str(" '");
        for ch in json.chars() {
            if ch == '\'' {
                command.push('\\');
            }
            command.push(ch);
        }
        command.push('\'');
    }
    Ok(command)
}

#[cfg(test)]
mod fingerprint_tests {
    use super::*;

    fn observations() -> Value {
        serde_json::from_str(include_str!("../data/garment-fingerprints.json")).unwrap()
    }

    fn validate(value: Value) -> Result<(), Error> {
        let data: FingerprintData = serde_json::from_value(value).unwrap();
        let legacy: Legacy = serde_json::from_str(include_str!("../data/legacy.json")).unwrap();
        for group in data.garments {
            garment_fingerprints::augment(
                &legacy.templates.get(group.garment).concat(),
                &group,
                &source_families(group.garment, &legacy.layouts),
            )?;
        }
        Ok(())
    }

    #[test]
    fn cross_engine_garment_collisions_are_rejected() {
        let mut data = observations();
        let group = &mut data["garments"][0];
        let first = group["sources"][0]["id"].clone();
        let other = group["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|source| source["id"] != first)
            .unwrap()["sourcePath"]
            .clone();
        let first_path = group["sources"][0]["sourcePath"].clone();
        let observed = group["fingerprints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|fingerprint| fingerprint["sourcePath"] == first_path)
            .unwrap()["vanilla"]
            .clone();
        let target = group["fingerprints"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|fingerprint| fingerprint["sourcePath"] == other)
            .unwrap();
        target["opensb"] = observed;
        assert!(validate(data).is_err());
    }

    #[test]
    fn missing_native_observations_and_layout_sources_are_rejected() {
        for field in ["fingerprints", "sources"] {
            let mut data = observations();
            data["garments"][0][field].as_array_mut().unwrap().pop();
            assert!(validate(data).is_err());
        }
        let mut data = observations();
        let removed = data["garments"][0]["sources"]
            .as_array_mut()
            .unwrap()
            .pop()
            .unwrap()["sourcePath"]
            .clone();
        data["garments"][0]["fingerprints"]
            .as_array_mut()
            .unwrap()
            .retain(|fingerprint| fingerprint["sourcePath"] != removed);
        assert!(validate(data).is_err());
    }

    #[test]
    fn duplicate_sources_unknown_stems_and_layout_mismatches_are_rejected() {
        for field in ["fingerprints", "sources"] {
            let mut data = observations();
            let entries = data["garments"][0][field].as_array_mut().unwrap();
            entries.push(entries[0].clone());
            assert!(validate(data).is_err());
        }
        let mut data = observations();
        data["garments"][0]["sampling"] = Value::String("?scale=1".to_owned());
        assert!(validate(data).is_err());
        let mut data = observations();
        data["garments"][0]["sources"][0]["column"] = Value::from(1000);
        assert!(validate(data).is_err());
        let mut data = observations();
        data["garments"][0]["sources"][0]["id"] = Value::from(0);
        assert!(validate(data).is_err());
    }

    #[test]
    fn invalid_address_seeds_are_rejected_before_union() {
        let data: FingerprintData = serde_json::from_value(observations()).unwrap();
        let group = data
            .garments
            .iter()
            .find(|group| group.garment == Garment::Pants)
            .unwrap();
        let legacy: Legacy = serde_json::from_str(include_str!("../data/legacy.json")).unwrap();
        let template = legacy.templates.pants.concat();
        let malformed = template.replacen("=ffa1ff00", "=00a1ff00", 1);
        assert!(
            garment_fingerprints::augment(
                &malformed,
                group,
                &source_families(Garment::Pants, &legacy.layouts),
            )
            .is_err()
        );
    }
}
