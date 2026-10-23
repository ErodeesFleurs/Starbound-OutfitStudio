//! Native observations augment, never select between, the shared garment prefix.
use crate::{Error, FrameLayout, Garment, Sheet};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    fmt::Write,
};

#[derive(Deserialize)]
pub(crate) struct FingerprintData {
    pub(crate) garments: Vec<GarmentFingerprints>,
}
#[derive(Deserialize)]
pub(crate) struct GarmentFingerprints {
    pub(crate) garment: Garment,
    sampling: String,
    sources: Vec<Source>,
    fingerprints: Vec<Fingerprint>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Source {
    source_path: String,
    sheet: Sheet,
    column: usize,
    row: usize,
    id: u8,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fingerprint {
    source_path: String,
    vanilla: String,
    opensb: String,
}

fn color(hex: &str) -> Result<u32, Error> {
    if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::new("Invalid garment fingerprint RGBA"));
    }
    let value =
        u32::from_str_radix(hex, 16).map_err(|_| Error::new("Invalid garment fingerprint RGBA"))?;
    match hex.len() {
        3 | 4 => {
            let mut expanded = 0;
            for digit in hex.bytes() {
                let nibble = (digit as char)
                    .to_digit(16)
                    .ok_or_else(|| Error::new("Invalid garment fingerprint RGBA"))?;
                expanded = (expanded << 8) | (nibble * 17);
            }
            Ok(if hex.len() == 3 {
                (expanded << 8) | 255
            } else {
                expanded
            })
        }
        6 => Ok((value << 8) | 255),
        8 => Ok(value),
        _ => Err(Error::new("Invalid garment fingerprint RGBA length")),
    }
}
fn observed_color(hex: &str) -> Result<u32, Error> {
    if hex.len() != 8 {
        return Err(Error::new(
            "Native garment fingerprint requires four RGBA bytes",
        ));
    }
    color(hex)
}

/// Every mapped layout cell must have one native observation for each actual
/// source family. This is derived from the existing consumer layouts rather
/// than a count asserted by the observation file itself.
pub(crate) fn augment(
    template: &str,
    data: &GarmentFingerprints,
    families: &[(Sheet, &FrameLayout, &str)],
) -> Result<String, Error> {
    let (sampling, rest) = template
        .split_once("?replace;")
        .ok_or_else(|| Error::new("Garment template has no fingerprint stage"))?;
    if sampling != data.sampling {
        return Err(Error::new("Unknown garment fingerprint sampling stem"));
    }
    let (replacements, canvas) = rest
        .split_once('?')
        .ok_or_else(|| Error::new("Garment template has no address canvas"))?;
    // Chest historically seeds opaque alpha and explicitly clears it in the
    // fixed canvas multiply. The other templates seed transparent alpha.
    let alpha = if data.garment == Garment::Chest {
        255
    } else {
        0
    };
    let mut required = HashMap::new();
    let mut ids = HashSet::new();
    for &(sheet, layout, family) in families {
        for (row, cells) in layout.iter().enumerate() {
            for (column, id) in cells.iter().enumerate().filter(|(_, id)| !id.is_empty()) {
                let id = u8::from_str_radix(id, 16)
                    .map_err(|_| Error::new("Invalid garment layout frame ID"))?;
                if required.insert((sheet, column, row, family), id).is_some() {
                    return Err(Error::new("Duplicate garment source family"));
                }
                ids.insert(id);
            }
        }
    }
    let mut mappings = HashMap::new();
    let mut seeds = HashSet::new();
    for pair in replacements.split(';') {
        let (from, to) = pair
            .split_once('=')
            .ok_or_else(|| Error::new("Invalid garment fingerprint replacement"))?;
        let target = color(to)?.to_be_bytes();
        if target[0] != 255 || target[2] != 255 || target[3] != alpha || !ids.contains(&target[1]) {
            return Err(Error::new("Invalid garment fingerprint address seed"));
        }
        if mappings.insert(color(from)?, target[1]).is_some() {
            return Err(Error::new("Duplicate garment fingerprint replacement"));
        }
        seeds.insert(target[1]);
    }
    if seeds != ids {
        return Err(Error::new(
            "Garment fingerprint seeds do not cover the layout",
        ));
    }
    let mut sources = HashMap::new();
    for source in &data.sources {
        let (family, _) = source
            .source_path
            .split_once(':')
            .filter(|(_, frame)| !frame.is_empty() && !frame.contains([':', '?']))
            .ok_or_else(|| Error::new("Invalid garment fingerprint source path"))?;
        // Starbound asset paths are ASCII case-insensitive; item JSON uses
        // pantsM.png while the packed index and canonical families use pantsm.
        let family = families
            .iter()
            .find_map(|(_, _, expected)| family.eq_ignore_ascii_case(expected).then_some(*expected))
            .ok_or_else(|| Error::new("Unknown garment source family"))?;
        let expected = required
            .remove(&(source.sheet, source.column, source.row, family))
            .ok_or_else(|| Error::new("Unknown or duplicate garment source cell"))?;
        if expected != source.id || !seeds.contains(&source.id) {
            return Err(Error::new("Garment source ID differs from its layout cell"));
        }
        if sources
            .insert(source.source_path.as_str(), source.id)
            .is_some()
        {
            return Err(Error::new("Duplicate garment fingerprint source"));
        }
    }
    if !required.is_empty() {
        return Err(Error::new(
            "Garment fingerprint source coverage is incomplete",
        ));
    }
    let mut additions = String::new();
    for fingerprint in &data.fingerprints {
        let id = sources
            .remove(fingerprint.source_path.as_str())
            .ok_or_else(|| Error::new("Unknown or duplicate native garment fingerprint source"))?;
        for observed in [&fingerprint.vanilla, &fingerprint.opensb] {
            let rgba = observed_color(observed)?;
            match mappings.get(&rgba) {
                Some(&previous) if previous != id => {
                    return Err(Error::new(format!(
                        "Cross-engine garment fingerprint collision at {}: IDs {previous} and {id}",
                        fingerprint.source_path
                    )));
                }
                Some(_) => {}
                None => {
                    mappings.insert(rgba, id);
                    write!(additions, ";{rgba:08x}=ff{id:02x}ff{alpha:02x}")
                        .expect("writing a String cannot fail");
                }
            }
        }
    }
    if !sources.is_empty() {
        return Err(Error::new(
            "Native garment fingerprint coverage is incomplete",
        ));
    }
    Ok(format!(
        "{sampling}?replace;{replacements}{additions}?{canvas}"
    ))
}
