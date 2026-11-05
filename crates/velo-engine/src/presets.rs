//! Ready-made code for a Shader node (docs/plugins.md §2): generators in Velo's own format, to
//! play as they are or to start from. Each moves with the music through its `audio` param.

/// The presets as (slug, source), in the order a menu shows them: Tunnel (the new node's own
/// code) first, then color fields, nature, lines and waves, radial, space, retro, patterns.
pub const PRESETS: [(&str, &str); 39] = [
    ("tunnel", include_str!("../shaders/presets/tunnel.wgsl")),
    ("plasma", include_str!("../shaders/presets/plasma.wgsl")),
    ("liquid", include_str!("../shaders/presets/liquid.wgsl")),
    ("mesh_gradient", include_str!("../shaders/presets/mesh_gradient.wgsl")),
    ("bokeh", include_str!("../shaders/presets/bokeh.wgsl")),
    ("iridescence", include_str!("../shaders/presets/iridescence.wgsl")),
    ("marble", include_str!("../shaders/presets/marble.wgsl")),
    ("clouds", include_str!("../shaders/presets/clouds.wgsl")),
    ("aurora", include_str!("../shaders/presets/aurora.wgsl")),
    ("fire", include_str!("../shaders/presets/fire.wgsl")),
    ("flow", include_str!("../shaders/presets/flow.wgsl")),
    ("waves", include_str!("../shaders/presets/waves.wgsl")),
    ("ridgelines", include_str!("../shaders/presets/ridgelines.wgsl")),
    ("ripples", include_str!("../shaders/presets/ripples.wgsl")),
    ("rings", include_str!("../shaders/presets/rings.wgsl")),
    ("spiral", include_str!("../shaders/presets/spiral.wgsl")),
    ("sunburst", include_str!("../shaders/presets/sunburst.wgsl")),
    ("light_rays", include_str!("../shaders/presets/light_rays.wgsl")),
    ("v_beams", include_str!("../shaders/presets/v_beams.wgsl")),
    ("lasers", include_str!("../shaders/presets/lasers.wgsl")),
    ("starfield", include_str!("../shaders/presets/starfield.wgsl")),
    ("hyperspace", include_str!("../shaders/presets/hyperspace.wgsl")),
    ("galaxy", include_str!("../shaders/presets/galaxy.wgsl")),
    ("fractal", include_str!("../shaders/presets/fractal.wgsl")),
    ("crystal_ball", include_str!("../shaders/presets/crystal_ball.wgsl")),
    ("ferrofluid", include_str!("../shaders/presets/ferrofluid.wgsl")),
    ("liquid_chrome", include_str!("../shaders/presets/liquid_chrome.wgsl")),
    ("fluid_sphere", include_str!("../shaders/presets/fluid_sphere.wgsl")),
    ("lightning", include_str!("../shaders/presets/lightning.wgsl")),
    ("traces", include_str!("../shaders/presets/traces.wgsl")),
    ("synthwave", include_str!("../shaders/presets/synthwave.wgsl")),
    ("code_rain", include_str!("../shaders/presets/code_rain.wgsl")),
    ("metaballs", include_str!("../shaders/presets/metaballs.wgsl")),
    ("voronoi", include_str!("../shaders/presets/voronoi.wgsl")),
    ("truchet", include_str!("../shaders/presets/truchet.wgsl")),
    ("hex_grid", include_str!("../shaders/presets/hex_grid.wgsl")),
    ("dot_grid", include_str!("../shaders/presets/dot_grid.wgsl")),
    ("stripes", include_str!("../shaders/presets/stripes.wgsl")),
    ("moire", include_str!("../shaders/presets/moire.wgsl")),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ShaderNode;

    #[test]
    fn every_preset_is_a_generator_that_compiles() {
        let mut slugs = std::collections::HashSet::new();
        for (slug, source) in PRESETS {
            assert!(slugs.insert(slug), "{slug} is in the list twice");
            let node = ShaderNode::parse(source).unwrap_or_else(|e| panic!("{slug}: {e}"));
            if let Err(e) = node.validate() {
                panic!("{slug}: {e}");
            }
            assert_eq!(node.category, "generator", "{slug}");
            assert!(node.inputs.is_empty() && !node.feedback, "{slug} reads no texture");
            assert!(!node.name.is_empty() && !node.description.is_empty(), "{slug} has a name and a description");
            // a handful to play with; a look with more to it (Crystal Ball) shows them all
            let own = node.params.iter().filter(|p| p.id != "opacity").count();
            assert!((3..=17).contains(&own), "{slug} has {own} params");
            assert!(node.params.iter().any(|p| p.id == "audio"), "{slug} moves with the music");
        }
    }
}
