use crate::{Error, RgbaImage, encoder::append_color_hex};
use indexmap::{IndexMap, map::Entry};

/// One original/edited image pair, in top-down PNG pixel order.
#[derive(Clone, Copy, Debug)]
pub struct RecolorLayer<'a> {
    pub source: &'a RgbaImage,
    pub target: &'a RgbaImage,
}

struct Mapping {
    target: u32,
    layer: usize,
    x: usize,
    y: usize,
}

fn validate(image: &RgbaImage, layer: usize, role: &str) -> Result<(), Error> {
    if image.width == 0 || image.height == 0 {
        return Err(Error::new(format!(
            "Layer {layer} {role}: image dimensions must be nonzero"
        )));
    }
    let expected = image
        .width
        .checked_mul(image.height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| Error::new(format!("Layer {layer} {role}: RGBA buffer size overflow")))?;
    if image.rgba.len() != expected {
        return Err(Error::new(format!(
            "Layer {layer} {role}: RGBA buffer must contain exactly {expected} bytes, got {}",
            image.rgba.len()
        )));
    }
    Ok(())
}

/// Build a single simultaneous `?replace` over all supplied image pairs.
///
/// Every occurrence of a source RGBA color must request the same target RGBA,
/// including unchanged pixels, alpha-zero hidden RGB and alpha-one pixels.
/// Pairs must have equal nonzero dimensions and exact RGBA buffers; different
/// pairs may have different dimensions. Conflicts report one-based layer numbers
/// and zero-based top-left pixel coordinates. No input is modified.
///
/// Unchanged mappings are omitted, so identical inputs return an empty string.
/// Alpha and silhouette changes are allowed when expressible by one color map;
/// they can reveal hidden RGB or affect every matching pixel in other frames.
/// Apply the output to the exact original images represented by the inputs,
/// including every animation frame/layer that will receive these directives.
/// This does not construct equipment, identity patches or a coordinate canvas.
pub fn recolor_directives(layers: &[RecolorLayer<'_>]) -> Result<String, Error> {
    if layers.is_empty() {
        return Err(Error::new(
            "At least one source/target recolor pair is required",
        ));
    }
    let mut mappings = IndexMap::<u32, Mapping>::new();
    for (index, layer) in layers.iter().enumerate() {
        let number = index + 1;
        validate(layer.source, number, "source")?;
        validate(layer.target, number, "target")?;
        if (layer.source.width, layer.source.height) != (layer.target.width, layer.target.height) {
            return Err(Error::new(format!(
                "Layer {number}: source {} × {} and target {} × {} dimensions must match",
                layer.source.width, layer.source.height, layer.target.width, layer.target.height
            )));
        }
        for (pixel, (source, target)) in layer
            .source
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .zip(layer.target.rgba.as_chunks::<4>().0)
            .enumerate()
        {
            let source = u32::from_be_bytes(*source);
            let target = u32::from_be_bytes(*target);
            match mappings.entry(source) {
                Entry::Occupied(entry) => {
                    let first = entry.get();
                    if first.target != target {
                        return Err(Error::new(format!(
                            "Layer {number} pixel ({}, {}): source RGBA {source:08x} maps to {target:08x}, but layer {} pixel ({}, {}) maps it to {:08x}; one source color cannot have multiple targets (coordinates are top-left, zero-based)",
                            pixel % layer.source.width,
                            pixel / layer.source.width,
                            first.layer,
                            first.x,
                            first.y,
                            first.target
                        )));
                    }
                }
                Entry::Vacant(entry) => {
                    entry.insert(Mapping {
                        target,
                        layer: number,
                        x: pixel % layer.source.width,
                        y: pixel / layer.source.width,
                    });
                }
            }
        }
    }
    let changed = mappings
        .iter()
        .filter(|(source, mapping)| **source != mapping.target)
        .count();
    if changed == 0 {
        return Ok(String::new());
    }
    let mut directives = String::with_capacity(8 + changed * 18);
    directives.push_str("?replace");
    for (source, mapping) in mappings {
        if source != mapping.target {
            directives.push(';');
            append_color_hex(&mut directives, source);
            directives.push('=');
            append_color_hex(&mut directives, mapping.target);
        }
    }
    Ok(directives)
}
