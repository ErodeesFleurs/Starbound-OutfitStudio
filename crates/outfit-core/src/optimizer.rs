use crate::{Encoding, Error, Layer, OptimizedEncoding, Strategy, encoder};
use flate2::{Compression, write::ZlibEncoder};
use std::{
    collections::HashSet,
    io::{self, Write},
};

struct Stage<'a> {
    pairs: Vec<(&'a str, &'a str)>,
    unique: bool,
}

fn rgba_value(hex: &str) -> u32 {
    let mut value = 0;
    for digit in hex.bytes() {
        let n = match digit {
            b'0'..=b'9' => digit - b'0',
            b'a'..=b'f' => digit - b'a' + 10,
            _ => 0,
        } as u32;
        if hex.len() <= 4 {
            value = (value << 8) | (n * 17);
        } else {
            value = (value << 4) | n;
        }
    }
    if hex.len() == 3 || hex.len() == 6 {
        (value << 8) | 255
    } else {
        value
    }
}

fn reordered(base: &Encoding, prefix: &str) -> Vec<String> {
    let stages: Vec<_> = base.directives[prefix.len()..]
        .split('?')
        .skip(1)
        .map(|operation| {
            let pairs: Vec<_> = operation
                .split(';')
                .skip(1)
                .filter_map(|pair| pair.split_once('='))
                .collect();
            let mut sources = HashSet::with_capacity(pairs.len());
            let unique = pairs
                .iter()
                .all(|(source, _)| sources.insert(rgba_value(source)));
            Stage { pairs, unique }
        })
        .collect();
    let mut alternatives = Vec::with_capacity(3);
    for order in 0..3 {
        let mut directives = String::with_capacity(base.output_bytes);
        directives.push_str(prefix);
        for stage in &stages {
            let mut indices: Vec<_> = (0..stage.pairs.len()).collect();
            if stage.unique {
                indices.sort_by(|&a, &b| {
                    let a = stage.pairs[a];
                    let b = stage.pairs[b];
                    match order {
                        0 => a.0.cmp(b.0),
                        1 => a.1.cmp(b.1).then_with(|| a.0.cmp(b.0)),
                        _ => b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)),
                    }
                });
            }
            directives.push_str(encoder::REPLACE);
            for index in indices {
                let (source, target) = stage.pairs[index];
                directives.push(';');
                directives.push_str(source);
                directives.push('=');
                directives.push_str(target);
            }
        }
        alternatives.push(directives);
    }
    alternatives
}

/// Exact candidate family: both bases first, then each base's three
/// safe within-stage orders. Never reorder a duplicate-source stage or prefix.
pub fn candidates(
    layers: &[Layer<'_>],
    prefix: &str,
    palette: bool,
    strategy: Strategy,
) -> Result<Vec<Encoding>, Error> {
    let scan = encoder::scan(layers)?;
    Ok(scan_candidates(&scan, prefix, palette, strategy))
}

pub(crate) fn scan_candidates(
    scan: &encoder::Scan,
    prefix: &str,
    palette: bool,
    strategy: Strategy,
) -> Vec<Encoding> {
    use encoder::SourceSpelling::{FixedAddresses, Shortest};
    let baseline = encoder::baseline_bytes(scan, prefix, Shortest);
    if strategy != Strategy::Compressed {
        return vec![if palette && strategy == Strategy::Text {
            encoder::factored(scan, prefix, baseline, Shortest)
                .unwrap_or_else(|| encoder::direct(scan, prefix, baseline, Shortest))
        } else {
            encoder::direct(scan, prefix, baseline, Shortest)
        }];
    }
    // Keep every old fixed-address candidate, ahead of the short-address family.
    // Clothing sources already have their upstream spelling and are unchanged.
    let fixed_baseline = encoder::baseline_bytes(scan, prefix, FixedAddresses);
    let direct = encoder::direct(scan, prefix, fixed_baseline, FixedAddresses);
    let automatic = palette
        .then(|| encoder::factored(scan, prefix, fixed_baseline, FixedAddresses))
        .flatten();
    let mut result = expand_candidates(direct, automatic, prefix);
    if fixed_baseline != baseline {
        let direct = encoder::direct(scan, prefix, baseline, Shortest);
        let automatic = palette
            .then(|| encoder::factored(scan, prefix, baseline, Shortest))
            .flatten();
        for candidate in expand_candidates(direct, automatic, prefix) {
            if !result
                .iter()
                .any(|previous| previous.directives == candidate.directives)
            {
                result.push(candidate);
            }
        }
    }
    result
}

/// Eight legacy clothing candidates plus at most eighteen new palette bases,
/// each with its original and three safe within-stage orders.
pub(crate) const CLOTHING_CANDIDATE_LIMIT: usize = 80;

pub(crate) fn clothing_candidate_limit(palette: bool, strategy: Strategy) -> usize {
    match (palette, strategy) {
        (_, Strategy::Text | Strategy::Passes) => 1,
        (false, Strategy::Compressed) => 4,
        (true, Strategy::Compressed) => CLOTHING_CANDIDATE_LIMIT,
    }
}

/// Extended family used only by garment generation. Hats, characters and the
/// general optimizer retain their existing scan_candidates contract.
pub(crate) fn clothing_scan_candidates(
    scan: &encoder::Scan,
    prefix: &str,
    palette: bool,
    strategy: Strategy,
) -> Vec<Encoding> {
    let mut result = scan_candidates(scan, prefix, palette, strategy);
    // Fixed-width RGB3 aliases make raw savings additive per color. The
    // existing gain-ranked palette already maximizes raw savings, so only
    // DEFLATE needs alternate subsets/assignments and their full strings.
    if !palette || strategy != Strategy::Compressed {
        return result;
    }
    let baseline = encoder::baseline_bytes(scan, prefix, encoder::SourceSpelling::Shortest);
    for base in encoder::clothing_palettes(scan, prefix, baseline) {
        let alternatives = expand_candidates(base, None, prefix);
        for candidate in alternatives {
            if result.len() == CLOTHING_CANDIDATE_LIMIT {
                return result;
            }
            if !result
                .iter()
                .any(|previous| previous.directives == candidate.directives)
            {
                result.push(candidate);
            }
        }
    }
    result
}

/// Expand the pixel replacement stages without changing their phase boundaries.
fn expand_candidates(direct: Encoding, automatic: Option<Encoding>, prefix: &str) -> Vec<Encoding> {
    let mut result = Vec::with_capacity(8);
    result.push(direct);
    if let Some(automatic) = automatic {
        result.push(automatic);
    }
    let base_count = result.len();
    for base_index in 0..base_count {
        for directives in reordered(&result[base_index], prefix) {
            if result
                .iter()
                .any(|candidate| candidate.directives == directives)
            {
                continue;
            }
            let base = &result[base_index];
            result.push(Encoding {
                directives,
                baseline_bytes: base.baseline_bytes,
                output_bytes: base.output_bytes,
                pixel_count: base.pixel_count,
                color_count: base.color_count,
                palette_applied: base.palette_applied,
                factored_color_count: base.factored_color_count,
            });
        }
    }
    result
}

/// Select using real caller-supplied scores. Equal score and byte length retain
/// candidate insertion order. Own the selected string without cloning it.
pub fn select(
    candidates: Vec<Encoding>,
    estimates: &[usize],
    strategy: Strategy,
) -> Result<OptimizedEncoding, Error> {
    if candidates.is_empty() {
        return Err(Error::new(
            "Outfit optimization requires at least one candidate",
        ));
    }
    if candidates.len() != estimates.len() {
        return Err(Error::new(format!(
            "Outfit optimization requires one DEFLATE estimate per candidate ({} candidates, {} estimates)",
            candidates.len(),
            estimates.len()
        )));
    }
    let candidate_count = candidates.len();
    let mut winner = None;
    for (encoding, &estimate) in candidates.into_iter().zip(estimates) {
        let replace = match &winner {
            None => true,
            Some(previous) => {
                let previous: &OptimizedEncoding = previous;
                (estimate, encoding.output_bytes)
                    < (
                        previous.estimated_deflate_bytes,
                        previous.encoding.output_bytes,
                    )
            }
        };
        if replace {
            winner = Some(OptimizedEncoding {
                encoding,
                strategy,
                estimated_deflate_bytes: estimate,
                candidate_count,
            });
        }
    }
    winner.ok_or_else(|| Error::new("Outfit optimization requires at least one candidate"))
}

#[derive(Default)]
struct CountBytes(usize);
impl Write for CountBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("DEFLATE output size overflow"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Native estimates use flate2's Rust zlib backend at level 6. They measure
/// complete UTF-8 strings, but can differ from browser CompressionStream scores
/// and do not measure actual Starbound protocol traffic.
pub fn optimize_native(
    layers: &[Layer<'_>],
    prefix: &str,
    palette: bool,
    strategy: Strategy,
) -> Result<OptimizedEncoding, Error> {
    let candidates = candidates(layers, prefix, palette, strategy)?;
    optimize_candidates_native(candidates, strategy)
}

pub(crate) fn optimize_candidates_native(
    candidates: Vec<Encoding>,
    strategy: Strategy,
) -> Result<OptimizedEncoding, Error> {
    let estimates: Result<Vec<_>, Error> = candidates
        .iter()
        .map(|candidate| {
            let mut encoder = ZlibEncoder::new(CountBytes::default(), Compression::new(6));
            encoder
                .write_all(candidate.directives.as_bytes())
                .map_err(|error| {
                    Error::new(format!("Unable to DEFLATE outfit directives: {error}"))
                })?;
            encoder.finish().map(|count| count.0).map_err(|error| {
                Error::new(format!(
                    "Unable to finish DEFLATE outfit directives: {error}"
                ))
            })
        })
        .collect();
    select(candidates, &estimates?, strategy)
}

#[cfg(test)]
mod clothing_tests {
    use super::*;
    use crate::{FrameLayout, RgbaImage};
    use std::collections::HashMap;

    fn sheet(ids: &[&str]) -> (RgbaImage, FrameLayout) {
        let width = ids.len() * 43;
        (
            RgbaImage {
                width,
                height: 43,
                rgba: vec![0; width * 43 * 4],
            },
            vec![ids.iter().map(|id| (*id).to_owned()).collect()],
        )
    }

    fn paint(image: &mut RgbaImage, index: usize, rgba: [u8; 4]) {
        image.rgba[index * 4..index * 4 + 4].copy_from_slice(&rgba);
    }

    fn fixture() -> (RgbaImage, FrameLayout) {
        let (mut image, frames) = sheet(&["a1", "11", ""]);
        let colors = [
            [18, 52, 86, 128],
            [17, 34, 51, 136],
            [0, 0, 0, 255],
            [0, 0, 17, 255],
            [255, 255, 255, 255],
            [1, 2, 3, 1],
            [254, 253, 252, 0],
            [18, 52, 86, 1],
            [17, 51, 85, 255],
            [1, 2, 3, 255],
        ];
        for index in 0..image.width * image.height {
            // Different repetition counts make gain and frequency rankings
            // disagree; the empty cell is deliberately painted too.
            let color = match index % 21 {
                0..=7 => colors[0],
                8..=11 => colors[1],
                n => colors[(n - 10) % colors.len()],
            };
            paint(&mut image, index, color);
        }
        (image, frames)
    }

    // Independent hex parser, not encoder's formatting or optimizer's parser.
    fn rgba(hex: &str) -> u32 {
        let mut bytes = [255; 4];
        if hex.len() <= 4 {
            for (channel, digit) in hex.chars().enumerate() {
                bytes[channel] = digit.to_digit(16).unwrap() as u8 * 17;
            }
        } else {
            for (channel, digits) in hex.as_bytes().as_chunks::<2>().0.iter().enumerate() {
                bytes[channel] =
                    u8::from_str_radix(std::str::from_utf8(digits).unwrap(), 16).unwrap();
            }
        }
        u32::from_be_bytes(bytes)
    }

    fn stages(directives: &str) -> Vec<Vec<(u32, u32)>> {
        directives
            .split('?')
            .filter(|stage| !stage.is_empty())
            .map(|stage| {
                let mut fields = stage.split(';');
                assert_eq!(fields.next(), Some("replace"));
                fields
                    .map(|pair| {
                        let (source, target) = pair.split_once('=').unwrap();
                        (rgba(source), rgba(target))
                    })
                    .collect()
            })
            .collect()
    }

    fn expected(layers: &[Layer<'_>]) -> HashMap<u32, u32> {
        let mut pixels = HashMap::new();
        for layer in layers {
            for (row, cells) in layer.frames.iter().enumerate() {
                for (column, id) in cells.iter().enumerate() {
                    if id.is_empty() {
                        continue;
                    }
                    let id = u8::from_str_radix(id, 16).unwrap();
                    for y in 0..43 {
                        for x in 0..43 {
                            let address = u32::from_be_bytes([x as u8, id, 42 - y as u8, 0]);
                            let offset = ((row * 43 + y) * layer.image.width + column * 43 + x) * 4;
                            let color: [u8; 4] =
                                layer.image.rgba[offset..offset + 4].try_into().unwrap();
                            if color[3] == 0 {
                                pixels.entry(address).or_insert(address);
                            } else {
                                pixels.insert(address, u32::from_be_bytes(color));
                            }
                        }
                    }
                }
            }
        }
        pixels
    }

    fn assert_pixels(layers: &[Layer<'_>], encoding: &Encoding, prefix: &str) {
        let stages = stages(encoding.directives.strip_prefix(prefix).unwrap());
        let tables: Vec<HashMap<_, _>> = stages
            .into_iter()
            .map(|pairs| pairs.into_iter().collect())
            .collect();
        let expected = expected(layers);
        let finals: HashSet<_> = expected.values().copied().collect();
        for table in tables.iter().skip(1) {
            for &alias in table.keys() {
                assert_eq!(alias & 255, 255, "alias must not touch hidden addresses");
                assert!(!finals.contains(&alias), "alias collides with final RGBA");
            }
        }
        for (source, target) in expected {
            let actual = tables
                .iter()
                .fold(source, |pixel, table| *table.get(&pixel).unwrap_or(&pixel));
            assert_eq!(actual, target, "full RGBA at {source:08x}");
        }
        assert_eq!(encoding.output_bytes, encoding.directives.len());
    }

    #[test]
    fn clothing_search_preserves_rgba_and_changes_assignments_and_subsets() {
        let (image, frames) = fixture();
        let layers = [Layer {
            image: &image,
            frames: &frames,
        }];
        let scan = encoder::scan(&layers).unwrap();
        let prefix = "衣装🧵?replace;deadbeef=0123";
        let old = scan_candidates(&scan, prefix, true, Strategy::Compressed);
        let searched = clothing_scan_candidates(&scan, prefix, true, Strategy::Compressed);
        let old_tables: HashSet<_> = old
            .iter()
            .filter_map(|candidate| {
                stages(candidate.directives.strip_prefix(prefix).unwrap()).pop()
            })
            .map(|mut table| {
                table.sort_unstable();
                table
            })
            .collect();
        let old_subsets: HashSet<Vec<_>> = old_tables
            .iter()
            .map(|table| {
                let mut targets: Vec<_> = table.iter().map(|pair| pair.1).collect();
                targets.sort_unstable();
                targets
            })
            .collect();
        let mut different_assignment = false;
        let mut different_subset = false;
        for candidate in &searched {
            assert_pixels(&layers, candidate, prefix);
            let mut final_table = stages(candidate.directives.strip_prefix(prefix).unwrap())
                .pop()
                .unwrap();
            final_table.sort_unstable();
            different_assignment |= candidate.palette_applied && !old_tables.contains(&final_table);
            let mut targets: Vec<_> = final_table.iter().map(|pair| pair.1).collect();
            targets.sort_unstable();
            different_subset |= candidate.palette_applied && !old_subsets.contains(&targets);
        }
        assert!(
            different_assignment,
            "search must explore a different actual RGBA alias map"
        );
        assert!(
            different_subset,
            "search must explore a different set of factored colors"
        );
    }

    #[test]
    fn clothing_search_keeps_baselines_and_cannot_worsen_either_objective() {
        let (image, frames) = fixture();
        let layers = [Layer {
            image: &image,
            frames: &frames,
        }];
        let scan = encoder::scan(&layers).unwrap();
        for palette in [false, true] {
            for strategy in [Strategy::Text, Strategy::Compressed, Strategy::Passes] {
                let old = scan_candidates(&scan, "完整前缀🧵", palette, strategy);
                let searched = clothing_scan_candidates(&scan, "完整前缀🧵", palette, strategy);
                for baseline in &old {
                    assert!(searched.contains(baseline), "legacy candidate was dropped");
                }
                if !palette || strategy == Strategy::Passes {
                    assert_eq!(searched, old);
                } else if strategy == Strategy::Text {
                    assert!(
                        searched
                            .iter()
                            .map(|candidate| candidate.directives.len())
                            .min()
                            <= old.iter().map(|candidate| candidate.directives.len()).min()
                    );
                } else {
                    let before = optimize_candidates_native(old, strategy).unwrap();
                    let after = optimize_candidates_native(searched, strategy).unwrap();
                    assert!(
                        (after.estimated_deflate_bytes, after.encoding.output_bytes)
                            <= (before.estimated_deflate_bytes, before.encoding.output_bytes)
                    );
                }
            }
        }
    }

    #[test]
    fn clothing_search_respects_scarce_aliases_and_exhausted_final_colors() {
        for occupied in [4095usize, 4096] {
            let (mut image, frames) = sheet(&["a1", "a2", "a3"]);
            for index in 0..occupied {
                paint(
                    &mut image,
                    index,
                    [
                        ((index >> 8) * 17) as u8,
                        (((index >> 4) & 15) * 17) as u8,
                        ((index & 15) * 17) as u8,
                        255,
                    ],
                );
            }
            for index in occupied..occupied + 90 {
                paint(
                    &mut image,
                    index,
                    if index % 3 == 0 {
                        [255, 255, 255, 1]
                    } else {
                        [18, 52, 86, 128]
                    },
                );
            }
            let layers = [Layer {
                image: &image,
                frames: &frames,
            }];
            let scan = encoder::scan(&layers).unwrap();
            let searched = clothing_scan_candidates(&scan, "", true, Strategy::Compressed);
            if occupied == 4096 {
                assert_eq!(
                    searched,
                    scan_candidates(&scan, "", true, Strategy::Compressed)
                );
            }
            for candidate in &searched {
                assert_pixels(&layers, candidate, "");
                for table in stages(&candidate.directives).iter().skip(1) {
                    for &(alias, _) in table {
                        assert_eq!(alias, 0xffffffff, "only unoccupied opaque RGB3 may be used");
                    }
                }
            }
        }
    }

    #[test]
    fn clothing_search_keeps_duplicate_source_layer_order_and_simultaneous_maps() {
        let (mut first, frames) = sheet(&["11", "11"]);
        let (mut last, last_frames) = sheet(&["11"]);
        for x in 0..43 {
            paint(&mut first, x, [1, 2, 3, 1]);
            paint(&mut first, x + 43, [18, 52, 86, 128]);
            paint(
                &mut last,
                x,
                if x % 3 == 0 {
                    [0, 0, 0, 255]
                } else {
                    [17, 34, 51, 136]
                },
            );
        }
        let layers = [
            Layer {
                image: &first,
                frames: &frames,
            },
            Layer {
                image: &last,
                frames: &last_frames,
            },
        ];
        let scan = encoder::scan(&layers).unwrap();
        let direct = encoder::direct(
            &scan,
            "",
            encoder::baseline_bytes(&scan, "", encoder::SourceSpelling::Shortest),
            encoder::SourceSpelling::Shortest,
        );
        let original_pairs = stages(&direct.directives).remove(0);
        for candidate in clothing_scan_candidates(&scan, "", true, Strategy::Compressed) {
            assert_pixels(&layers, &candidate, "");
            let mut operations = stages(&candidate.directives);
            let first = operations.remove(0);
            let tail: Vec<HashMap<_, _>> = operations
                .into_iter()
                .map(|pairs| pairs.into_iter().collect())
                .collect();
            let normalized: Vec<_> = first
                .into_iter()
                .map(|(source, target)| {
                    let final_color = tail
                        .iter()
                        .fold(target, |pixel, table| *table.get(&pixel).unwrap_or(&pixel));
                    (source, final_color)
                })
                .collect();
            assert_eq!(
                normalized, original_pairs,
                "duplicate-source stage order changed"
            );
        }
    }
}
