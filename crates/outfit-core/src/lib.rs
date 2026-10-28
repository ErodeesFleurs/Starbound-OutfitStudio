//! Exact, local-only Starbound outfit encoding.
//!
//! Template constants originate from Silverfeelin/Starbound-NgOutfitGenerator
//! (MIT, including Nettle Boy's template attribution); helper profiles leave the
//! original frame-fingerprint chain intact.
mod assets;
mod character;
mod encoder;
mod garment;
mod garment_fingerprints;
mod hat;
mod optimizer;
mod png_decode;
mod recolor;

use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

pub use assets::{descriptor, hat_descriptor, layout, spawn_command, template};
pub use character::{
    CharacterFrame, CharacterLayer, CharacterLayout, CharacterPart, apply_identity_patch,
    character_candidates, character_layout, character_prefix, character_suppressed_sources,
    compose_character_head, identity_command, identity_patch, normalize_character_head,
    optimize_character_native, validate_character_head, validate_character_layer,
};
pub use encoder::encode;
pub use garment::{
    GarmentCandidate, GarmentCandidates, GarmentEncoding, garment_candidates,
    optimize_garment_native, select_garment,
};
pub use hat::{hat_candidates, hat_geometry, optimize_hat_native};
pub use optimizer::{candidates, optimize_native, select};
pub use png_decode::{decode_png, normalize_sheet};
pub use recolor::{RecolorLayer, recolor_directives};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);
impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}

macro_rules! keyed_enum {
    ($name:ident, $label:literal, {$($variant:ident => $key:literal),+ $(,)?}) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum $name { $(#[serde(rename = $key)] $variant),+ }
        impl $name {
            pub const fn key(self) -> &'static str { match self { $(Self::$variant => $key),+ } }
        }
        impl FromStr for $name {
            type Err = Error;
            fn from_str(value: &str) -> Result<Self, Error> {
                match value { $($key => Ok(Self::$variant),)+ _ => Err(Error::new(format!("Unknown {}: {value}", $label))) }
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(self.key()) }
        }
    };
}
keyed_enum!(Garment, "template kind", {Pants => "pants", HidingPants => "hidingPants", Chest => "chest", Back => "back"});
keyed_enum!(Profile, "asset profile", {Original => "original", Conservative => "conservative", Compact => "compact"});
keyed_enum!(ProfileChoice, "asset profile choice", {Auto => "auto", Original => "original", Conservative => "conservative", Compact => "compact"});
keyed_enum!(Strategy, "optimization strategy", {Text => "text", Compressed => "compressed", Passes => "passes"});
keyed_enum!(Sheet, "sheet", {Pants => "pants", Body => "chest", FrontSleeves => "frontSleeves", BackSleeves => "backSleeves", Back => "back"});
keyed_enum!(HatRarity, "hat rarity", {Common => "common", Uncommon => "uncommon", Rare => "rare", Legendary => "legendary", Essential => "essential"});

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HatMetadata {
    pub shortdescription: String,
    pub description: String,
    pub rarity: HatRarity,
    pub remove_hair: bool,
    pub hide_body: bool,
}

impl Default for HatMetadata {
    fn default() -> Self {
        Self {
            shortdescription: "^orange;Custom Hat".to_owned(),
            description: "A custom hat created by me!".to_owned(),
            rarity: HatRarity::Common,
            remove_hair: false,
            hide_body: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HatGeometry {
    pub width: usize,
    pub height: usize,
    pub offset_x: usize,
    pub offset_y: usize,
    pub cropped: bool,
}

pub type FrameLayout = Vec<Vec<String>>;
#[derive(Debug, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}
#[derive(Clone, Copy, Debug)]
pub struct Layer<'a> {
    pub image: &'a RgbaImage,
    pub frames: &'a FrameLayout,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Encoding {
    pub directives: String,
    pub baseline_bytes: usize,
    pub output_bytes: usize,
    pub pixel_count: usize,
    pub color_count: usize,
    pub palette_applied: bool,
    pub factored_color_count: usize,
}
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimizedEncoding {
    #[serde(flatten)]
    pub encoding: Encoding,
    pub strategy: Strategy,
    /// Real complete-string zlib DEFLATE size, not equipment wire bytes.
    pub estimated_deflate_bytes: usize,
    pub candidate_count: usize,
}
