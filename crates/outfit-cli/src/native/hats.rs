//! Independent hat probe accounting; a normal server exit is not a pixel pass.
use super::{Rcon, invalid, output_line};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    error::Error,
    io::Write,
};

#[derive(Debug)]
pub(super) struct HatCase {
    name: String,
    width: u64,
    height: u64,
    sources: u64,
    variants: u64,
    colors: u64,
    transparent: u64,
}

pub(super) fn parse<'a>(
    value: &'a Value,
    names: &mut HashSet<&'a str>,
) -> Result<Vec<HatCase>, Box<dyn Error>> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    let cases = value
        .as_array()
        .ok_or_else(|| invalid("Hat cases must be an array"))?;
    let mut result = Vec::with_capacity(cases.len());
    for case in cases {
        let name = case["name"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| invalid("Missing hat fixture name"))?;
        if !names.insert(name) {
            return Err(invalid("Duplicate hat fixture name").into());
        }
        let dimension = |key: &str| {
            case[key]
                .as_u64()
                .filter(|n| (1..=256).contains(n))
                .ok_or_else(|| invalid(format!("Invalid hat {key}: {name}")))
        };
        let width = dimension("width")?;
        let height = dimension("height")?;
        if !case["expectedPath"]
            .as_str()
            .is_some_and(|s| s.starts_with('/') && s.ends_with(".png"))
        {
            return Err(invalid(format!("Invalid hat expected PNG: {name}")).into());
        }
        let sources = case["sourcePaths"]
            .as_array()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| invalid("Hat fixture has no source paths"))?;
        let mut paths = HashSet::new();
        for source in sources {
            let path = source
                .as_str()
                .filter(|s| s.starts_with('/'))
                .ok_or_else(|| invalid("Invalid hat source path"))?;
            if !paths.insert(path) {
                return Err(invalid("Duplicate hat source path").into());
            }
        }
        let variants = case["variants"]
            .as_array()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| invalid("Hat fixture has no variants"))?;
        let mut variant_names = HashSet::new();
        let mut directives = HashSet::new();
        for variant in variants {
            let name = variant["name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| invalid("Missing hat variant name"))?;
            let directive = variant["directives"]
                .as_str()
                .filter(|s| s.starts_with('?'))
                .ok_or_else(|| invalid("Missing hat directives"))?;
            if !variant_names.insert(name) || !directives.insert(directive) {
                return Err(invalid("Duplicate hat variant name or directive").into());
            }
        }
        let colors = case["colors"]
            .as_array()
            .ok_or_else(|| invalid("Missing expected hat palette"))?;
        let mut palette = HashSet::new();
        for color in colors {
            let color = color
                .as_str()
                .filter(|s| s.len() == 8 && s.bytes().all(|b| b.is_ascii_hexdigit()))
                .ok_or_else(|| invalid("Hat palette must contain RGBA8"))?;
            if color.ends_with("00") || !palette.insert(color) {
                return Err(invalid("Hat palette contains transparent or duplicate color").into());
            }
        }
        let transparent = case["transparent"]
            .as_array()
            .filter(|v| v.len() <= 5)
            .ok_or_else(|| invalid("Invalid transparent hat samples"))?;
        for probe in transparent {
            if !probe["x"].as_u64().is_some_and(|x| x < width)
                || !probe["y"].as_u64().is_some_and(|y| y < height)
                || !probe["rgba"].as_array().is_some_and(|v| {
                    v.len() == 4
                        && v[3] == 0
                        && v.iter().all(|n| n.as_u64().is_some_and(|n| n <= 255))
                })
            {
                return Err(invalid("Invalid transparent hat coordinate/color").into());
            }
        }
        if !case["descriptors"]
            .as_array()
            .is_some_and(|v| !v.is_empty() && v.iter().all(Value::is_object))
        {
            return Err(invalid("Missing native hat descriptors").into());
        }
        result.push(HatCase {
            name: name.into(),
            width,
            height,
            sources: sources.len() as u64,
            variants: variants.len() as u64,
            colors: colors.len() as u64,
            transparent: transparent.len() as u64,
        });
    }
    Ok(result)
}

fn parse_batch(
    response: &str,
    case: &HatCase,
    source: u64,
    variant: u64,
) -> Result<Value, Box<dyn Error>> {
    let mut result: Value = serde_json::from_str(response)
        .map_err(|e| invalid(format!("Hat {}: {e}: {response}", case.name)))?;
    if let Some(inner) = result.as_str() {
        result = serde_json::from_str(inner)?;
    }
    let expected = [
        ("sourceIndex", source),
        ("variantIndex", variant),
        ("width", case.width),
        ("height", case.height),
        ("colorChecks", case.colors),
        ("transparentProbes", case.transparent),
    ];
    if result["fixture"] != case.name
        || expected
            .iter()
            .any(|(key, value)| result[*key].as_u64() != Some(*value))
    {
        return Err(invalid(format!("Incomplete native hat result: {response}")).into());
    }
    Ok(result)
}

pub(super) fn run(
    client: &mut Rcon,
    cases: &[HatCase],
    output: &mut impl Write,
) -> Result<Vec<Value>, Box<dyn Error>> {
    let mut results = Vec::with_capacity(cases.len());
    for (index, case) in cases.iter().enumerate() {
        let mut batches = 0;
        for source in 1..=case.sources {
            for variant in 1..=case.variants {
                let response = client.command(&format!(
                    "eval outfitVanillaHatVerify({},{source},{variant})",
                    index + 1
                ))?;
                parse_batch(&response, case, source, variant)?;
                batches += 1;
            }
        }
        let result = json!({"fixture":case.name,"width":case.width,"height":case.height,"sources":case.sources,"variants":case.variants,"batches":batches,"colorChecks":case.colors*batches,"transparentProbes":case.transparent*batches});
        output_line(output, "OUTFIT_VANILLA_HAT_CASE_PASS", &result)?;
        results.push(result);
    }
    Ok(results)
}

pub(super) fn expected_batches(cases: &[HatCase]) -> u64 {
    cases.iter().map(|c| c.sources * c.variants).sum()
}

pub(super) fn opensb_marker(log: &str, cases: &[HatCase]) -> Result<String, Box<dyn Error>> {
    let markers: Vec<_> = log
        .lines()
        .filter(|line| line.contains("OUTFIT_HAT_VERIFY_PASS "))
        .collect();
    if markers.len() != 1 {
        return Err(invalid("Missing unique native hat RGBA PASS marker").into());
    }
    let marker = markers[0];
    let payload = marker
        .split_once("OUTFIT_HAT_VERIFY_PASS ")
        .ok_or_else(|| invalid("Missing hat marker payload"))?
        .1;
    let mut counts = BTreeMap::new();
    for part in payload.split_whitespace() {
        let (key, value) = part
            .split_once('=')
            .ok_or_else(|| invalid("Malformed hat PASS marker"))?;
        if counts.insert(key, value.parse::<u64>()?).is_some() {
            return Err(invalid("Duplicate hat PASS field").into());
        }
    }
    let variants = expected_batches(cases);
    let pixels: u64 = cases
        .iter()
        .map(|c| c.width * c.height * c.sources * c.variants)
        .sum();
    let expected = [
        ("fixtures", cases.len() as u64),
        ("variants", variants),
        ("pixels", pixels),
        ("channels", pixels * 4),
    ];
    if counts.len() != expected.len()
        || expected
            .iter()
            .any(|(key, count)| counts.get(key) != Some(count))
    {
        return Err(invalid(format!("Incomplete native hat RGBA pass: {payload}")).into());
    }
    Ok(marker.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        json!({"name":"hat","width":3,"height":2,"expectedPath":"/hat.png","sourcePaths":["/head.png"],"colors":["12345601"],"transparent":[{"x":0,"y":0,"rgba":[0,1,0,0]}],"variants":[{"name":"text","directives":"?replace;00010000=f00"}],"descriptors":[{"name":"eyepatchhead"}]})
    }
    #[test]
    fn hat_counts_reject_partial_native_success() {
        let mut value = json!([fixture()]);
        value[0]["transparent"] = json!([
            {"x":0,"y":0,"rgba":[0,0,0,0]},
            {"x":1,"y":0,"rgba":[1,0,0,0]},
            {"x":2,"y":0,"rgba":[2,0,0,0]},
            {"x":2,"y":1,"rgba":[2,0,1,0]},
            {"x":0,"y":1,"rgba":[0,0,1,0]}
        ]);
        let parsed = parse(&value, &mut HashSet::new()).unwrap();
        let case = &parsed[0];
        let response = r#"{"fixture":"hat","sourceIndex":1,"variantIndex":1,"width":3,"height":2,"colorChecks":1,"transparentProbes":5}"#;
        assert!(parse_batch(response, case, 1, 1).is_ok());
        assert!(parse_batch(response, case, 1, 2).is_err());
        assert!(
            parse_batch(
                &response.replace("\"transparentProbes\":5", "\"transparentProbes\":3"),
                case,
                1,
                1
            )
            .is_err()
        );
        assert!(
            parse_batch(
                &response.replace("\"colorChecks\":1", "\"colorChecks\":0"),
                case,
                1,
                1
            )
            .is_err()
        );
        assert!(
            opensb_marker(
                "OUTFIT_HAT_VERIFY_PASS fixtures=1 variants=1 pixels=6 channels=24",
                &parsed
            )
            .is_ok()
        );
        assert!(
            opensb_marker(
                "OUTFIT_HAT_VERIFY_PASS fixtures=1 variants=1 pixels=6 channels=0",
                &parsed
            )
            .is_err()
        );
        let mut duplicate = fixture();
        duplicate["variants"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"compressed","directives":"?replace;00010000=f00"}));
        assert!(parse(&json!([duplicate]), &mut HashSet::new()).is_err());
    }
}
