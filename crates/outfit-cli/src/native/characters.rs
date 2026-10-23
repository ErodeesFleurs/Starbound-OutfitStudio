//! Every original frame name (including aliases) is an independently accounted batch.
use super::{Rcon, invalid, output_line};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    error::Error,
    io::Write,
};

#[derive(Debug)]
pub(super) struct CharacterCase {
    name: String,
    sources: u64,
    variants: u64,
    colors: u64,
    transparent: u64,
}

pub(super) fn parse<'a>(
    value: &'a Value,
    groups: &Value,
    names: &mut HashSet<&'a str>,
) -> Result<Vec<CharacterCase>, Box<dyn Error>> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    let cases = value
        .as_array()
        .ok_or_else(|| invalid("Character cases must be an array"))?;
    let mut result = Vec::with_capacity(cases.len());
    for case in cases {
        let name = case["name"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| invalid("Missing character fixture name"))?;
        if !names.insert(name) {
            return Err(invalid("Duplicate character fixture name").into());
        }
        if case["width"] != 43
            || case["height"] != 43
            || !case["expectedPath"]
                .as_str()
                .is_some_and(|s| s.starts_with('/') && s.ends_with(".png"))
        {
            return Err(invalid("Invalid character expected PNG geometry/path").into());
        }
        let sources = case["sourcePaths"]
            .as_array()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| invalid("Missing character original frame paths"))?;
        let mut paths = HashSet::new();
        for source in sources {
            let path = source
                .as_str()
                .filter(|s| s.starts_with('/') && s.contains(".png:"))
                .ok_or_else(|| invalid("Character source must select an original frame"))?;
            if !paths.insert(path) {
                return Err(invalid("Duplicate character frame path").into());
            }
        }
        let group = case["variantGroup"]
            .as_str()
            .ok_or_else(|| invalid("Missing character variant group"))?;
        let variants = groups[group]
            .as_array()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| invalid("Missing character variants"))?;
        let mut variant_names = HashSet::new();
        let mut directives = HashSet::new();
        for variant in variants {
            let name = variant["name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| invalid("Missing character variant name"))?;
            let directive = variant["directives"]
                .as_str()
                .filter(|s| s.starts_with('?'))
                .ok_or_else(|| invalid("Missing character directives"))?;
            if !variant_names.insert(name) || !directives.insert(directive) {
                return Err(invalid("Duplicate character variant").into());
            }
        }
        let colors = case["colors"]
            .as_array()
            .ok_or_else(|| invalid("Missing character palette"))?;
        let mut palette = HashSet::new();
        for color in colors {
            let color = color
                .as_str()
                .filter(|s| s.len() == 8 && s.bytes().all(|b| b.is_ascii_hexdigit()))
                .ok_or_else(|| invalid("Invalid character RGBA8"))?;
            if color.ends_with("00") || !palette.insert(color) {
                return Err(invalid("Duplicate/transparent character palette color").into());
            }
        }
        let transparent = case["transparent"]
            .as_array()
            .filter(|v| v.len() <= 3)
            .ok_or_else(|| invalid("Invalid character transparent samples"))?;
        let mut coordinates = HashSet::new();
        for probe in transparent {
            let x = probe["x"]
                .as_u64()
                .filter(|n| *n < 43)
                .ok_or_else(|| invalid("Invalid character probe x"))?;
            let y = probe["y"]
                .as_u64()
                .filter(|n| *n < 43)
                .ok_or_else(|| invalid("Invalid character probe y"))?;
            if !coordinates.insert((x, y))
                || !probe["rgba"].as_array().is_some_and(|v| {
                    v.len() == 4
                        && v[3] == 0
                        && v.iter().all(|n| n.as_u64().is_some_and(|n| n <= 255))
                })
            {
                return Err(invalid("Invalid character transparent RGBA").into());
            }
        }
        result.push(CharacterCase {
            name: name.into(),
            sources: sources.len() as u64,
            variants: variants.len() as u64,
            colors: colors.len() as u64,
            transparent: transparent.len() as u64,
        });
    }
    Ok(result)
}

pub(super) fn expected_batches(cases: &[CharacterCase]) -> u64 {
    cases.iter().map(|c| c.sources * c.variants).sum()
}

pub(super) fn run(
    client: &mut Rcon,
    cases: &[CharacterCase],
    output: &mut impl Write,
) -> Result<Vec<Value>, Box<dyn Error>> {
    let mut results = Vec::with_capacity(cases.len());
    for (index, case) in cases.iter().enumerate() {
        let mut batches = 0;
        for source in 1..=case.sources {
            for variant in 1..=case.variants {
                let response = client.command(&format!(
                    "eval outfitVanillaCharacterVerify({},{source},{variant})",
                    index + 1
                ))?;
                let mut result: Value = serde_json::from_str(&response)
                    .map_err(|e| invalid(format!("Character {}: {e}: {response}", case.name)))?;
                if let Some(inner) = result.as_str() {
                    result = serde_json::from_str(inner)?;
                }
                let counts = [
                    ("sourceIndex", source),
                    ("variantIndex", variant),
                    ("width", 43),
                    ("height", 43),
                    ("colorChecks", case.colors),
                    ("transparentProbes", case.transparent),
                ];
                if result["fixture"] != case.name
                    || counts
                        .iter()
                        .any(|(key, n)| result[*key].as_u64() != Some(*n))
                {
                    return Err(
                        invalid(format!("Incomplete native character batch: {response}")).into(),
                    );
                }
                batches += 1;
            }
        }
        let result = json!({"fixture":case.name,"sources":case.sources,"variants":case.variants,"batches":batches,"colorChecks":case.colors*batches,"transparentProbes":case.transparent*batches});
        output_line(output, "OUTFIT_VANILLA_CHARACTER_CASE_PASS", &result)?;
        results.push(result);
    }
    Ok(results)
}

pub(super) fn opensb_marker(log: &str, cases: &[CharacterCase]) -> Result<String, Box<dyn Error>> {
    let markers: Vec<_> = log
        .lines()
        .filter(|line| line.contains("OUTFIT_CHARACTER_VERIFY_PASS "))
        .collect();
    if markers.len() != 1 {
        return Err(invalid("Missing unique native character RGBA PASS marker").into());
    }
    let marker = markers[0];
    let payload = marker
        .split_once("OUTFIT_CHARACTER_VERIFY_PASS ")
        .ok_or_else(|| invalid("Missing character PASS payload"))?
        .1;
    let mut counts = BTreeMap::new();
    for field in payload.split_whitespace() {
        let (key, value) = field
            .split_once('=')
            .ok_or_else(|| invalid("Malformed character PASS field"))?;
        if counts.insert(key, value.parse::<u64>()?).is_some() {
            return Err(invalid("Duplicate character PASS field").into());
        }
    }
    let batches = expected_batches(cases);
    let pixels = batches * 43 * 43;
    let expected = [
        ("fixtures", cases.len() as u64),
        ("variants", batches),
        ("pixels", pixels),
        ("channels", pixels * 4),
    ];
    if counts.len() != expected.len() || expected.iter().any(|(key, n)| counts.get(key) != Some(n))
    {
        return Err(invalid(format!("Incomplete native character RGBA PASS: {payload}")).into());
    }
    Ok(marker.into())
}
