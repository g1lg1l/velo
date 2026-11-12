//! Input effects for the Shader node.

pub const EFFECT_PRESETS: [(&str, &str); 25] = [
    ("glitch", include_str!("../shaders/effects/glitch.wgsl")),
    ("vhs", include_str!("../shaders/effects/vhs.wgsl")),
    ("pixelate", include_str!("../shaders/effects/pixelate.wgsl")),
    ("halftone", include_str!("../shaders/effects/halftone.wgsl")),
    ("dither", include_str!("../shaders/effects/dither.wgsl")),
    ("chromatic-aberration", include_str!("../shaders/effects/chromatic-aberration.wgsl")),
    ("crt", include_str!("../shaders/effects/crt.wgsl")),
    ("film-grain", include_str!("../shaders/effects/film-grain.wgsl")),
    ("ascii", include_str!("../shaders/effects/ascii.wgsl")),
    ("posterize", include_str!("../shaders/effects/posterize.wgsl")),
    ("edge-glow", include_str!("../shaders/effects/edge-glow.wgsl")),
    ("mirror", include_str!("../shaders/effects/mirror.wgsl")),
    ("bloom", include_str!("../shaders/effects/bloom.wgsl")),
    ("strobe", include_str!("../shaders/effects/strobe.wgsl")),
    ("zoom-blur", include_str!("../shaders/effects/zoom-blur.wgsl")),
    ("tile", include_str!("../shaders/effects/tile.wgsl")),
    ("led-wall", include_str!("../shaders/effects/led-wall.wgsl")),
    ("gradient-map", include_str!("../shaders/effects/gradient-map.wgsl")),
    ("hex-mosaic", include_str!("../shaders/effects/hex-mosaic.wgsl")),
    ("god-rays", include_str!("../shaders/effects/god-rays.wgsl")),
    ("wave-warp", include_str!("../shaders/effects/wave-warp.wgsl")),
    ("twirl", include_str!("../shaders/effects/twirl.wgsl")),
    ("fisheye", include_str!("../shaders/effects/fisheye.wgsl")),
    ("shake", include_str!("../shaders/effects/shake.wgsl")),
    ("vignette", include_str!("../shaders/effects/vignette.wgsl")),
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::EFFECT_PRESETS;
    use crate::{EFFECT_TEMPLATE, ShaderNode};

    #[test]
    fn the_effect_template_reads_its_input() {
        let node = ShaderNode::parse(EFFECT_TEMPLATE).unwrap();
        node.validate().unwrap();
        assert_eq!((node.category.as_str(), node.inputs.as_slice()), ("effect", ["src".to_string()].as_slice()));
    }

    #[test]
    fn every_effect_parses_and_validates() {
        let mut slugs = HashSet::new();
        for (slug, source) in EFFECT_PRESETS {
            assert!(slugs.insert(slug), "duplicate effect slug: {slug}");
            let node = ShaderNode::parse(source).unwrap_or_else(|e| panic!("{slug}: {e}"));
            node.validate().unwrap_or_else(|e| panic!("{slug}: {e}\n{}", node.wgsl));
            assert_eq!(node.category, "effect", "{slug}");
            assert_eq!(node.inputs, ["src"], "{slug}");

            let json = source.trim_start().strip_prefix("/*").unwrap().split_once("*/").unwrap().0;
            let header: serde_json::Value = serde_json::from_str(json).unwrap();
            assert_eq!(header["inputs"], serde_json::json!([{ "id": "src", "type": "texture" }]), "{slug}");
            assert!((2..=7).contains(&header["params"].as_array().unwrap().len()), "{slug}");
            assert!(!node.name.is_empty(), "{slug}");
            assert!(!node.description.is_empty() && !node.description.contains('\n'), "{slug}");
            assert!(node.params.iter().any(|p| p.id == "music"), "{slug}");
        }
    }
}
