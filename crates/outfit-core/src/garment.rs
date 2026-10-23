use crate::{
    Encoding, Error, Garment, Layer, OptimizedEncoding, Profile, ProfileChoice, Strategy, encoder,
    optimizer, template,
};
use serde::{Deserialize, Serialize};

impl ProfileChoice {
    /// Conservative comes first so exact ties preserve the previous default.
    pub fn profiles(self) -> &'static [Profile] {
        match self {
            Self::Auto => &[Profile::Conservative, Profile::Original, Profile::Compact],
            Self::Original => &[Profile::Original],
            Self::Conservative => &[Profile::Conservative],
            Self::Compact => &[Profile::Compact],
        }
    }
}

#[derive(Debug)]
pub struct GarmentCandidate {
    pub profile: Profile,
    pub encoding: Encoding,
}

/// Raw-length strategies retain only their stable winner for DEFLATE reporting.
/// candidate_count counts all complete strings compared before that reduction.
#[derive(Debug)]
pub struct GarmentCandidates {
    pub candidates: Vec<GarmentCandidate>,
    pub candidate_count: usize,
    pub profile_count: usize,
    pub candidate_limit: usize,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GarmentEncoding {
    pub profile: Profile,
    pub profile_count: usize,
    pub candidate_limit: usize,
    #[serde(flatten)]
    pub result: OptimizedEncoding,
}

/// Scan the entire ordered garment once, sharing color accounting across every
/// layer and legal helper profile. The old profile candidate family stays first.
pub fn garment_candidates(
    layers: &[Layer<'_>],
    garment: Garment,
    choice: ProfileChoice,
    palette: bool,
    strategy: Strategy,
) -> Result<GarmentCandidates, Error> {
    let scan = encoder::scan(layers)?;
    let profiles = choice.profiles();
    let mut candidates = Vec::new();
    for &profile in profiles {
        candidates.extend(
            optimizer::clothing_scan_candidates(
                &scan,
                template(garment, profile),
                palette,
                strategy,
            )
            .into_iter()
            .map(|encoding| GarmentCandidate { profile, encoding }),
        );
    }
    let candidate_count = candidates.len();
    if strategy != Strategy::Compressed {
        let winner = candidates
            .iter()
            .enumerate()
            .min_by_key(|(index, candidate)| (candidate.encoding.output_bytes, *index))
            .map(|(index, _)| index)
            .ok_or_else(|| Error::new("Garment optimization requires at least one candidate"))?;
        let selected = candidates.swap_remove(winner);
        candidates.clear();
        candidates.push(selected);
    }
    Ok(GarmentCandidates {
        candidates,
        candidate_count,
        profile_count: profiles.len(),
        candidate_limit: profiles.len() * optimizer::clothing_candidate_limit(palette, strategy),
    })
}

/// Compressed compares full-string DEFLATE then raw length; text/passes have
/// already selected by full raw length and only require the winner's estimate.
pub fn select_garment(
    candidates: GarmentCandidates,
    estimates: &[usize],
    strategy: Strategy,
) -> Result<GarmentEncoding, Error> {
    if candidates.candidates.len() != estimates.len() {
        return Err(Error::new(
            "Garment optimization requires one DEFLATE estimate per scoring candidate",
        ));
    }
    let winner = candidates
        .candidates
        .iter()
        .zip(estimates)
        .enumerate()
        .min_by_key(|(index, (candidate, estimate))| {
            let raw = candidate.encoding.output_bytes;
            (
                if strategy == Strategy::Compressed {
                    **estimate
                } else {
                    raw
                },
                raw,
                *index,
            )
        })
        .map(|(index, _)| index)
        .ok_or_else(|| Error::new("Garment optimization requires at least one candidate"))?;
    let selected = candidates
        .candidates
        .into_iter()
        .nth(winner)
        .ok_or_else(|| Error::new("Missing garment optimization winner"))?;
    Ok(GarmentEncoding {
        profile: selected.profile,
        profile_count: candidates.profile_count,
        candidate_limit: candidates.candidate_limit,
        result: OptimizedEncoding {
            encoding: selected.encoding,
            strategy,
            estimated_deflate_bytes: estimates[winner],
            candidate_count: candidates.candidate_count,
        },
    })
}

/// Native scoring uses the same zlib level 6 implementation as other modes.
pub fn optimize_garment_native(
    layers: &[Layer<'_>],
    garment: Garment,
    choice: ProfileChoice,
    palette: bool,
    strategy: Strategy,
) -> Result<GarmentEncoding, Error> {
    let candidates = garment_candidates(layers, garment, choice, palette, strategy)?;
    let result = optimizer::optimize_candidates_native(
        candidates
            .candidates
            .into_iter()
            .map(|candidate| candidate.encoding)
            .collect(),
        strategy,
    )?;
    let profile = choice
        .profiles()
        .iter()
        .copied()
        .find(|&profile| {
            result
                .encoding
                .directives
                .starts_with(template(garment, profile))
        })
        .ok_or_else(|| Error::new("Selected garment has no matching helper profile"))?;
    Ok(GarmentEncoding {
        profile,
        profile_count: candidates.profile_count,
        candidate_limit: candidates.candidate_limit,
        result: OptimizedEncoding {
            candidate_count: candidates.candidate_count,
            ..result
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FrameLayout, RgbaImage};

    fn image(color: [u8; 4]) -> RgbaImage {
        RgbaImage {
            width: 43,
            height: 43,
            rgba: color.repeat(43 * 43),
        }
    }

    #[test]
    fn auto_text_compares_complete_ordered_multilayer_garment() {
        let images = [
            image([0x12, 0x34, 0x56, 255]),
            image([0x78, 0x9a, 0xbc, 128]),
            image([0xde, 0xf0, 0x12, 1]),
        ];
        let layouts: [FrameLayout; 3] = [
            vec![vec!["01".into()]],
            vec![vec!["02".into()]],
            vec![vec!["03".into()]],
        ];
        let layers: Vec<_> = images
            .iter()
            .zip(&layouts)
            .map(|(image, frames)| Layer { image, frames })
            .collect();
        let automatic = optimize_garment_native(
            &layers,
            Garment::Chest,
            ProfileChoice::Auto,
            true,
            Strategy::Text,
        )
        .unwrap();
        let fixed: Vec<_> = [
            ProfileChoice::Conservative,
            ProfileChoice::Original,
            ProfileChoice::Compact,
        ]
        .into_iter()
        .map(|profile| {
            optimize_garment_native(&layers, Garment::Chest, profile, true, Strategy::Text).unwrap()
        })
        .collect();
        let expected = fixed
            .iter()
            .enumerate()
            .min_by_key(|(index, value)| (value.result.encoding.output_bytes, *index))
            .unwrap()
            .1;
        assert_eq!(automatic.profile, expected.profile);
        assert_eq!(
            automatic.result.encoding.directives,
            expected.result.encoding.directives
        );
        assert_eq!(automatic.result.encoding.pixel_count, 3 * 43 * 43);
        assert_eq!(automatic.result.encoding.color_count, 3);
        assert!(
            automatic
                .result
                .encoding
                .directives
                .starts_with(template(Garment::Chest, automatic.profile),)
        );
    }

    #[test]
    fn explicit_profile_cannot_escape_even_with_favorable_compression_scores() {
        let image = image([0x12, 0x34, 0x56, 128]);
        let frames = vec![vec!["01".into()]];
        let layers = [Layer {
            image: &image,
            frames: &frames,
        }];
        for (choice, profile) in [
            (ProfileChoice::Original, Profile::Original),
            (ProfileChoice::Conservative, Profile::Conservative),
            (ProfileChoice::Compact, Profile::Compact),
        ] {
            let candidates =
                garment_candidates(&layers, Garment::Pants, choice, true, Strategy::Compressed)
                    .unwrap();
            assert!(
                candidates
                    .candidates
                    .iter()
                    .all(|candidate| candidate.profile == profile)
            );
            let estimates: Vec<_> = (0..candidates.candidates.len()).rev().collect();
            let selected = select_garment(candidates, &estimates, Strategy::Compressed).unwrap();
            assert_eq!(selected.profile, profile);
            assert!(
                selected
                    .result
                    .encoding
                    .directives
                    .starts_with(template(Garment::Pants, profile))
            );
        }
    }

    #[test]
    fn browser_scores_choose_actual_profile_and_preserve_baseline_ties() {
        let image = image([0x12, 0x34, 0x56, 255]);
        let frames = vec![vec!["01".into()]];
        let layers = [Layer {
            image: &image,
            frames: &frames,
        }];
        let candidates = garment_candidates(
            &layers,
            Garment::Back,
            ProfileChoice::Auto,
            true,
            Strategy::Compressed,
        )
        .unwrap();
        let target = candidates
            .candidates
            .iter()
            .position(|candidate| candidate.profile == Profile::Original)
            .unwrap();
        let expected = candidates.candidates[target].encoding.directives.clone();
        let mut estimates = vec![1000; candidates.candidates.len()];
        estimates[target] = 1;
        let selected = select_garment(candidates, &estimates, Strategy::Compressed).unwrap();
        assert_eq!(selected.profile, Profile::Original);
        assert_eq!(selected.result.encoding.directives, expected);

        let baseline = crate::candidates(
            &layers,
            template(Garment::Back, Profile::Conservative),
            true,
            Strategy::Compressed,
        )
        .unwrap()
        .remove(0);
        let candidates = GarmentCandidates {
            candidates: vec![
                GarmentCandidate {
                    profile: Profile::Conservative,
                    encoding: baseline.clone(),
                },
                GarmentCandidate {
                    profile: Profile::Compact,
                    encoding: baseline.clone(),
                },
            ],
            candidate_count: 2,
            profile_count: 2,
            candidate_limit: 2,
        };
        let selected = select_garment(candidates, &[50, 50], Strategy::Compressed).unwrap();
        assert_eq!(selected.profile, Profile::Conservative);
        assert_eq!(selected.result.encoding, baseline);
    }

    #[test]
    fn text_and_passes_ignore_irrelevant_compression_order() {
        let make = |directives: &str| Encoding {
            directives: directives.into(),
            output_bytes: directives.len(),
            baseline_bytes: 100,
            pixel_count: 0,
            color_count: 0,
            palette_applied: false,
            factored_color_count: 0,
        };
        for strategy in [Strategy::Text, Strategy::Passes] {
            let candidates = GarmentCandidates {
                candidates: vec![
                    GarmentCandidate {
                        profile: Profile::Original,
                        encoding: make("long"),
                    },
                    GarmentCandidate {
                        profile: Profile::Compact,
                        encoding: make("s"),
                    },
                ],
                candidate_count: 2,
                profile_count: 2,
                candidate_limit: 2,
            };
            let selected = select_garment(candidates, &[1, 100], strategy).unwrap();
            assert_eq!(selected.profile, Profile::Compact);
            assert_eq!(selected.result.encoding.directives, "s");
        }
    }
}
