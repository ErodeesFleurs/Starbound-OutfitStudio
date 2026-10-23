use outfit_core::{RgbaImage, Strategy, hat_candidates};
use std::collections::HashMap;

fn color(hex: &str) -> [u8; 4] {
    let mut expanded = if hex.len() <= 4 {
        hex.chars().flat_map(|c| [c, c]).collect::<String>()
    } else {
        hex.to_owned()
    };
    if expanded.len() == 6 {
        expanded.push_str("ff");
    }
    u32::from_str_radix(&expanded, 16).unwrap().to_be_bytes()
}
fn mapping(operation: &str) -> HashMap<[u8; 4], [u8; 4]> {
    operation
        .split(';')
        .skip(1)
        .map(|pair| {
            let (from, to) = pair.split_once('=').unwrap();
            (color(from), color(to))
        })
        .collect()
}
#[derive(Clone, Copy, Debug)]
enum Division {
    Exact,
    Reciprocal,
    RefinedLowEstimate,
    RefinedHighEstimate,
}
fn coordinate(n: usize, scale: f32, division: Division) -> f32 {
    let n = n as f32;
    match division {
        Division::Exact => n / scale,
        Division::Reciprocal => n * scale.recip(),
        Division::RefinedLowEstimate | Division::RefinedHighEstimate => {
            // Exact failing OpenSB ELF: RCPPS, q=n*r, q+(n-q*scale)*r.
            // These two coarse estimates exercise either side of 1/scale;
            // scale=256's low estimate reproduces q(256)=1-2^-24.
            let error = if matches!(division, Division::RefinedLowEstimate) {
                -1.0 / 4096.0
            } else {
                1.0 / 4096.0
            };
            let estimate = scale.recip() * (1.0 + error);
            let quotient = n * estimate;
            quotient + (n - quotient * scale) * estimate
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Lerp {
    Weighted,
    Difference,
    FusedLeft,
    FusedRight,
    FusedDifference,
}
fn mix(a: f32, b: f32, t: f32, arithmetic: Lerp) -> f32 {
    match arithmetic {
        Lerp::Weighted => a * (1.0 - t) + b * t,
        Lerp::Difference => a + (b - a) * t,
        Lerp::FusedLeft => a.mul_add(1.0 - t, b * t),
        Lerp::FusedRight => b.mul_add(t, a * (1.0 - t)),
        Lerp::FusedDifference => (b - a).mul_add(t, a),
    }
}

// Execute the parsed canvas and pixel replacement, not just its ideal address
// formula. Raw-byte arithmetic matches StarImageScaling; normalized arithmetic
// and FMA forms add coverage beyond the observed mul/add implementation.
// Include reciprocal-estimate refinement,
// which actually moved the old scale=256 endpoint below 255 in OpenSB.
// The full square covers every address any supported rectangle can consume.
#[test]
fn hat_canvas_addresses_are_bijective_across_float_implementations() {
    let dimensions = (1usize..=256)
        .flat_map(|extent| [(extent, 1), (1, extent)])
        .chain([(43, 43), (256, 256), (86, 215)]);
    for (width, height) in dimensions {
        let rgba: Vec<u8> = (0..height)
            .flat_map(|y| {
                (0..width).flat_map(move |x| {
                    [
                        x as u8,
                        y as u8,
                        (x ^ y) as u8,
                        [0, 1, 128, 255][(x + y) % 4],
                    ]
                })
            })
            .collect();
        let image = RgbaImage {
            width,
            height,
            rgba,
        };
        let geometry = outfit_core::hat_geometry(&image).unwrap();
        let encoded = hat_candidates(&image, false, Strategy::Text)
            .unwrap()
            .remove(0);
        let operations: Vec<_> = encoded.directives.split('?').skip(1).collect();
        let blend = operations
            .iter()
            .position(|op| op.starts_with("blendmult="))
            .unwrap();
        let corner_map = mapping(operations[blend + 1]);
        // Captured original xsmall.png pixels at bottom-left (0, 3).
        // A simultaneous replace leaves an unmapped source unchanged.
        let corners = [
            [0, 0, 0, 0],
            [227, 227, 227, 255],
            [255, 255, 255, 0],
            [0, 0, 0, 255],
        ]
        .map(|source| corner_map.get(&source).copied().unwrap_or(source));
        let scale_at = operations
            .iter()
            .position(|op| op.starts_with("scale="))
            .unwrap();
        let scales: Vec<f32> = operations[scale_at]
            .strip_prefix("scale=")
            .unwrap()
            .split(';')
            .map(|n| n.parse().unwrap())
            .collect();
        let crop: Vec<usize> = operations[scale_at + 1]
            .strip_prefix("crop;")
            .unwrap()
            .split(';')
            .map(|n| n.parse().unwrap())
            .collect();
        let nearest: f32 = operations[scale_at + 2]
            .strip_prefix("scalenearest=")
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(
            ((crop[2] - crop[0]) as f32 * nearest).round() as usize,
            geometry.width
        );
        assert_eq!(
            ((crop[3] - crop[1]) as f32 * nearest).round() as usize,
            geometry.height
        );
        let paint = mapping(operations[scale_at + 3]);
        for normalized in [false, true] {
            for division in [
                Division::Exact,
                Division::Reciprocal,
                Division::RefinedLowEstimate,
                Division::RefinedHighEstimate,
            ] {
                for arithmetic in [
                    Lerp::Weighted,
                    Lerp::Difference,
                    Lerp::FusedLeft,
                    Lerp::FusedRight,
                    Lerp::FusedDifference,
                ] {
                    let samples = corners.map(|pixel| {
                        pixel.map(|n| {
                            if normalized {
                                n as f32 / 255.0
                            } else {
                                n as f32
                            }
                        })
                    });
                    for y in 0..geometry.height {
                        for x in 0..geometry.width {
                            let sx = (coordinate(x, nearest, division).round() as usize)
                                .min(crop[2] - crop[0] - 1)
                                + crop[0];
                            let sy = (coordinate(y, nearest, division).round() as usize)
                                .min(crop[3] - crop[1] - 1)
                                + crop[1];
                            let px = coordinate(sx, scales[0], division);
                            let py = coordinate(sy, scales[1], division);
                            let ix = px.floor() as usize;
                            let iy = py.floor() as usize;
                            let fetch = |dx: usize, dy: usize| {
                                samples[(iy + dy).min(1) * 2 + (ix + dx).min(1)]
                            };
                            let [a, b, c, d] = [fetch(0, 0), fetch(1, 0), fetch(0, 1), fetch(1, 1)];
                            let actual_address = std::array::from_fn(|channel| {
                                let bottom =
                                    mix(a[channel], b[channel], px - ix as f32, arithmetic);
                                let top = mix(c[channel], d[channel], px - ix as f32, arithmetic);
                                let value = mix(bottom, top, py - iy as f32, arithmetic);
                                (if normalized { value * 255.0 } else { value }) as u8
                            });
                            let actual = paint
                                .get(&actual_address)
                                .copied()
                                .unwrap_or(actual_address);
                            let offset = ((geometry.offset_y + geometry.height - 1 - y) * width
                                + geometry.offset_x
                                + x)
                                * 4;
                            let input: [u8; 4] = image.rgba[offset..offset + 4].try_into().unwrap();
                            let expected = if input[3] == 0 {
                                [x as u8, 0, y as u8, 0]
                            } else {
                                input
                            };
                            assert_eq!(
                                actual, expected,
                                "{width}x{height} ({x},{y}) normalized={normalized} division={division:?} arithmetic={arithmetic:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}
