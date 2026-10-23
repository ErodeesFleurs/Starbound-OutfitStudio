use crate::{Encoding, Error, Layer};
use indexmap::IndexMap;
use std::{collections::HashMap, fmt::Write};

pub(crate) const REPLACE: &str = "?replace";
const FRAME_SIZE: usize = 43;

pub(crate) enum Source {
    Canonical(u32),
    Address(u32),
}

#[derive(Clone, Copy)]
pub(crate) enum SourceSpelling {
    Shortest,
    FixedAddresses,
}

impl Source {
    fn len(&self, spelling: SourceSpelling) -> usize {
        match self {
            Self::Canonical(color) => color_hex_len(*color),
            Self::Address(color) => match spelling {
                SourceSpelling::Shortest => color_hex_len(*color),
                SourceSpelling::FixedAddresses => 8,
            },
        }
    }

    fn append(&self, output: &mut String, spelling: SourceSpelling) {
        match self {
            Self::Canonical(color) => append_color_hex(output, *color),
            Self::Address(color) => match spelling {
                SourceSpelling::Shortest => append_color_hex(output, *color),
                SourceSpelling::FixedAddresses => {
                    let _ = write!(output, "{color:08x}");
                }
            },
        }
    }
}

pub(crate) struct Pair {
    pub source: Source,
    pub target: usize,
}
pub(crate) struct Color {
    pub hex: String,
    value: u32,
    count: usize,
}
pub(crate) struct Scan {
    pub entries: Vec<Pair>,
    pub colors: Vec<Color>,
    pub pixel_count: usize,
}

/// Shared color accounting for outfit layouts and dynamically sized hats.
pub(crate) struct ScanBuilder {
    scan: Scan,
    colors: HashMap<u32, usize>,
}

impl ScanBuilder {
    pub fn new() -> Self {
        Self {
            scan: Scan {
                entries: Vec::new(),
                colors: Vec::new(),
                pixel_count: 0,
            },
            colors: HashMap::new(),
        }
    }

    pub fn count_pixels(&mut self, count: usize) {
        self.scan.pixel_count += count;
    }

    pub fn pair(&mut self, source: Source, target: u32) {
        let color_index = *self.colors.entry(target).or_insert_with(|| {
            let next = self.scan.colors.len();
            self.scan.colors.push(Color {
                hex: color_hex(target),
                value: target,
                count: 0,
            });
            next
        });
        self.scan.colors[color_index].count += 1;
        self.scan.entries.push(Pair {
            source,
            target: color_index,
        });
    }

    pub fn finish(self) -> Scan {
        self.scan
    }
}

fn color_hex_len(color: u32) -> usize {
    let bytes = color.to_be_bytes();
    let channels = if bytes[3] == 255 { 3 } else { 4 };
    channels
        * if bytes.iter().all(|byte| byte % 17 == 0) {
            1
        } else {
            2
        }
}

pub(crate) fn append_color_hex(hex: &mut String, color: u32) {
    let bytes = color.to_be_bytes();
    let short = bytes.iter().all(|byte| byte % 17 == 0);
    let channels = if bytes[3] == 255 { 3 } else { 4 };
    const DIGITS: &[u8] = b"0123456789abcdef";
    for &byte in &bytes[..channels] {
        if short {
            hex.push(DIGITS[(byte / 17) as usize] as char);
        } else {
            hex.push(DIGITS[(byte >> 4) as usize] as char);
            hex.push(DIGITS[(byte & 15) as usize] as char);
        }
    }
}

/// Canonical upstream RGB3/RGBA4/RGB6/RGBA8 spelling.
pub(crate) fn color_hex(color: u32) -> String {
    let mut hex = String::with_capacity(8);
    append_color_hex(&mut hex, color);
    hex
}

fn validate(layer: &Layer<'_>, index: usize) -> Result<(), Error> {
    let number = index + 1;
    let columns = layer.frames.first().map_or(0, Vec::len);
    if columns == 0 {
        return Err(Error::new(format!(
            "Layer {number}: frames must be a nonempty rectangular layout"
        )));
    }
    for (row_index, row) in layer.frames.iter().enumerate() {
        if row.len() != columns {
            return Err(Error::new(format!(
                "Layer {number}: frame row {} must contain {columns} cells",
                row_index + 1
            )));
        }
        if row.iter().any(|id| {
            !id.is_empty() && (id.len() > 2 || !id.bytes().all(|b| b.is_ascii_hexdigit()))
        }) {
            return Err(Error::new(format!(
                "Layer {number}: frame identifiers must be hexadecimal bytes or empty strings"
            )));
        }
    }
    let width = columns
        .checked_mul(FRAME_SIZE)
        .ok_or_else(|| Error::new(format!("Layer {number}: sheet dimensions overflow")))?;
    let height = layer
        .frames
        .len()
        .checked_mul(FRAME_SIZE)
        .ok_or_else(|| Error::new(format!("Layer {number}: sheet dimensions overflow")))?;
    if layer.image.width != width || layer.image.height != height {
        return Err(Error::new(format!(
            "Layer {number}: expected a {width} × {height} RGBA sheet"
        )));
    }
    let expected = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| Error::new(format!("Layer {number}: RGBA buffer size overflow")))?;
    if layer.image.rgba.len() != expected {
        return Err(Error::new(format!(
            "Layer {number}: RGBA buffer must contain exactly {expected} bytes"
        )));
    }
    Ok(())
}

// ECMAScript Object.keys places canonical decimal array indices before other
// keys. Leading zeros and 2^32-1 are not array indices, even when parseable.
fn array_index(key: &str) -> Option<u32> {
    if key.len() > 1 && key.starts_with('0') {
        return None;
    }
    key.parse::<u32>().ok().filter(|&n| n != u32::MAX)
}

pub(crate) fn scan(layers: &[Layer<'_>]) -> Result<Scan, Error> {
    let mut result = ScanBuilder::new();
    for (index, layer) in layers.iter().enumerate() {
        validate(layer, index)?;
        let ids: Vec<Vec<Option<u8>>> = layer
            .frames
            .iter()
            .map(|row| {
                row.iter()
                    .map(|id| {
                        if id.is_empty() {
                            None
                        } else {
                            u8::from_str_radix(id, 16).ok()
                        }
                    })
                    .collect()
            })
            .collect();
        let mut swaps = IndexMap::<u32, u32>::new();
        for (y, pixels) in layer
            .image
            .rgba
            .chunks_exact(layer.image.width * 4)
            .enumerate()
        {
            let row = &ids[y / FRAME_SIZE];
            let local_y = (FRAME_SIZE - 1 - y % FRAME_SIZE) as u32;
            for (x, rgba) in pixels.as_chunks::<4>().0.iter().enumerate() {
                if rgba[3] == 0 {
                    continue;
                }
                let Some(id) = row[x / FRAME_SIZE] else {
                    continue;
                };
                let source = ((x % FRAME_SIZE) as u32) << 24 | (id as u32) << 16 | local_y << 8;
                let target = u32::from_be_bytes(*rgba);
                swaps.insert(source, target);
                result.count_pixels(1);
            }
        }
        let mut ordered: Vec<_> = swaps
            .into_iter()
            .map(|(source, target)| {
                let hex = color_hex(source);
                let index = array_index(&hex);
                (target, source, index)
            })
            .collect();
        // Stable sort retains original property insertion order for nonindices.
        ordered.sort_by_key(|(_, _, index)| index.map_or((true, 0), |n| (false, n)));
        for (target, source, _) in ordered {
            result.pair(Source::Canonical(source), target);
        }
    }
    Ok(result.finish())
}

pub(crate) fn baseline_bytes(scan: &Scan, prefix: &str, spelling: SourceSpelling) -> usize {
    prefix.len()
        + REPLACE.len()
        + scan
            .entries
            .iter()
            .map(|pair| pair.source.len(spelling) + scan.colors[pair.target].hex.len() + 2)
            .sum::<usize>()
}

pub(crate) fn direct(
    scan: &Scan,
    prefix: &str,
    baseline_bytes: usize,
    spelling: SourceSpelling,
) -> Encoding {
    let mut directives = String::with_capacity(baseline_bytes);
    directives.push_str(prefix);
    directives.push_str(REPLACE);
    for pair in &scan.entries {
        directives.push(';');
        pair.source.append(&mut directives, spelling);
        directives.push('=');
        directives.push_str(&scan.colors[pair.target].hex);
    }
    Encoding {
        baseline_bytes: directives.len(),
        output_bytes: directives.len(),
        directives,
        pixel_count: scan.pixel_count,
        color_count: scan.colors.len(),
        palette_applied: false,
        factored_color_count: 0,
    }
}

/// Return a smaller palette encoding, or None when the original must win.
pub(crate) fn factored(
    scan: &Scan,
    prefix: &str,
    baseline_bytes: usize,
    spelling: SourceSpelling,
) -> Option<Encoding> {
    let mut ranked: Vec<_> = scan
        .colors
        .iter()
        .enumerate()
        .filter_map(|(index, color)| {
            let len = color.hex.len() as i128;
            let savings = color.count as i128 * (len - 3) - (len + 5);
            (savings > 0).then_some((index, savings))
        })
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut reserved = [false; 4096];
    for color in &scan.colors {
        if color.hex.len() == 3 {
            // Canonical hex was produced internally, so conversion cannot fail.
            if let Ok(value) = usize::from_str_radix(&color.hex, 16) {
                reserved[value] = true;
            }
        }
    }
    let mut aliases = vec![None; scan.colors.len()];
    let mut assigned = Vec::new();
    let mut next = 0;
    let mut savings = 0i128;
    for (index, gain) in ranked {
        while next < reserved.len() && reserved[next] {
            next += 1;
        }
        if next == reserved.len() {
            break;
        }
        aliases[index] = Some(next);
        assigned.push(index);
        savings += gain;
        next += 1;
    }
    if savings <= REPLACE.len() as i128 {
        return None;
    }
    Some(assigned_palette(
        scan,
        prefix,
        baseline_bytes,
        spelling,
        &aliases,
        &assigned,
        savings,
    ))
}

fn assigned_palette(
    scan: &Scan,
    prefix: &str,
    baseline_bytes: usize,
    spelling: SourceSpelling,
    aliases: &[Option<usize>],
    assigned: &[usize],
    savings: i128,
) -> Encoding {
    let output_bytes = (baseline_bytes as i128 + REPLACE.len() as i128 - savings) as usize;
    let mut directives = String::with_capacity(output_bytes);
    directives.push_str(prefix);
    directives.push_str(REPLACE);
    for pair in &scan.entries {
        directives.push(';');
        pair.source.append(&mut directives, spelling);
        directives.push('=');
        if let Some(alias) = aliases[pair.target] {
            let _ = write!(directives, "{alias:03x}");
        } else {
            directives.push_str(&scan.colors[pair.target].hex);
        }
    }
    directives.push_str(REPLACE);
    for &index in assigned {
        if let Some(alias) = aliases[index] {
            let _ = write!(directives, ";{alias:03x}={}", scan.colors[index].hex);
        }
    }
    Encoding {
        baseline_bytes,
        output_bytes: directives.len(),
        directives,
        pixel_count: scan.pixel_count,
        color_count: scan.colors.len(),
        palette_applied: true,
        factored_color_count: assigned.len(),
    }
}

/// Clothing-only search: three color orders, three subsets and two assignments.
/// All accounting is per distinct color; only accepted, distinct plans traverse
/// the already scanned entries to emit their replacement strings.
pub(crate) fn clothing_palettes(scan: &Scan, prefix: &str, baseline_bytes: usize) -> Vec<Encoding> {
    let gains: Vec<_> = scan
        .colors
        .iter()
        .map(|color| {
            let len = color.hex.len() as i128;
            color.count as i128 * (len - 3) - (len + 5)
        })
        .collect();
    let mut reserved = [false; 4096];
    for color in &scan.colors {
        // RGB3 aliases are opaque. No other canonical spelling can represent
        // one of these RGBA values, including alpha-one or transparent colors.
        if color.hex.len() == 3 {
            let [r, g, b, _] = color.value.to_be_bytes();
            reserved[((r as usize / 17) << 8) | ((g as usize / 17) << 4) | (b as usize / 17)] =
                true;
        }
    }
    let available = reserved.iter().filter(|&&used| !used).count();
    if available == 0 {
        return Vec::new();
    }
    let positive_count = gains.iter().filter(|&&gain| gain > 0).count();
    let mut plans = std::collections::HashSet::new();
    let mut result = Vec::new();
    for order in 0..3 {
        let mut ranked: Vec<_> = (0..scan.colors.len()).collect();
        ranked.sort_by(|&a, &b| {
            let colors = &scan.colors;
            match order {
                0 => gains[b].cmp(&gains[a]),
                1 => colors[b].count.cmp(&colors[a].count),
                _ => colors[a].hex.cmp(&colors[b].hex),
            }
            .then_with(|| a.cmp(&b))
        });
        for subset in 0..3 {
            let limit = if subset == 1 {
                positive_count.div_ceil(2).min(available)
            } else {
                available
            };
            let assigned: Vec<_> = ranked
                .iter()
                .copied()
                .filter(|&index| {
                    if subset == 2 {
                        scan.colors[index].count > 1
                    } else {
                        gains[index] > 0
                    }
                })
                .take(limit)
                .collect();
            let savings: i128 = assigned.iter().map(|&index| gains[index]).sum();
            if assigned.is_empty() {
                continue;
            }
            for correlated in [false, true] {
                let mut used = reserved;
                let mut aliases = vec![None; scan.colors.len()];
                let mut next = 0;
                for &index in &assigned {
                    let mut alias = if correlated {
                        let [r, g, b, _] = scan.colors[index].value.to_be_bytes();
                        ((r as usize >> 4) << 8) | ((g as usize >> 4) << 4) | (b as usize >> 4)
                    } else {
                        next
                    };
                    while used[alias] {
                        alias = (alias + 1) & 4095;
                    }
                    used[alias] = true;
                    aliases[index] = Some(alias);
                    next = (alias + 1) & 4095;
                }
                // Canonicalize only the plan key, never pixel/layer ordering.
                // Equivalent assignments need not emit or reparse a full string.
                let key: Vec<_> = aliases
                    .iter()
                    .enumerate()
                    .filter_map(|(index, alias)| alias.map(|alias| (index, alias)))
                    .collect();
                if plans.insert(key) {
                    // The legacy automatic family already emitted this exact
                    // gain-ranked, sequential assignment and all its orders.
                    if order == 0 && subset == 0 && !correlated && savings > REPLACE.len() as i128 {
                        continue;
                    }
                    result.push(assigned_palette(
                        scan,
                        prefix,
                        baseline_bytes,
                        SourceSpelling::Shortest,
                        &aliases,
                        &assigned,
                        savings,
                    ));
                }
            }
        }
    }
    result
}

pub fn encode(layers: &[Layer<'_>], prefix: &str, palette: bool) -> Result<Encoding, Error> {
    let scan = scan(layers)?;
    let spelling = SourceSpelling::Shortest;
    let baseline_bytes = baseline_bytes(&scan, prefix, spelling);
    if palette && let Some(encoding) = factored(&scan, prefix, baseline_bytes, spelling) {
        return Ok(encoding);
    }
    Ok(direct(&scan, prefix, baseline_bytes, spelling))
}
