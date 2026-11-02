//! Scenes to start from (M4): small audio-reactive patches with the music already moving them
//! and named macros for a controller. Built-in nodes do nothing by themselves (no spin, no
//! audio), so whatever moves here is set or mapped here. "New scene" offers them; the first-run demo is the
//! first, Velo. Built with the operators, like a user would.

use serde_json::json;
use velo_core::{Project, SHADER, Session};

use crate::canvas::{NODE_H, NODE_W, SPACING};

/// What a macro drives: node, parameter, range.
type Drives = &'static [(usize, &'static str, f32, f32)];

pub struct Template {
    pub name: &'static str,
    pub description: &'static str,
    /// The preset its Shader node runs (a slug of `velo_engine::PRESETS`), else the template.
    shader: Option<&'static str>,
    /// Kind, column, row.
    nodes: &'static [(&'static str, f32, f32)],
    /// From node, to node, to port.
    wires: &'static [(usize, usize, u8)],
    /// Node, parameter, value (a number; a color or a vec2 as a list).
    params: &'static [(usize, &'static str, &'static [f32])],
    /// Node, parameter, source, amount (of the parameter's range): what the music moves.
    maps: &'static [(usize, &'static str, &'static str, f32)],
    /// Each macro: its name and what it drives.
    macros: &'static [(&'static str, Drives)],
}

pub const TEMPLATES: [Template; 5] = [
    Template {
        name: "Velo",
        description: "Velo's mark: two beams of light in a V through billowing smoke, thin rays fanning round them; the bass opens it, the kick flashes it and sends a shockwave along the floor, the hats flicker the rays",
        shader: Some("v_beams"),
        nodes: &[(SHADER, 0.0, 0.0), ("velo.output", 1.0, 0.0)],
        wires: &[(0, 1, 0)],
        params: &[(0, "audio", &[1.0])],
        maps: &[(0, "smoke", "mid", 0.2), (0, "rays", "high", 0.2)],
        macros: &[("Spread", &[(0, "spread", 0.2, 0.7)]), ("Smoke", &[(0, "smoke", 0.2, 1.5)]), ("Light", &[(0, "intensity", 0.4, 1.8)]), ("Rays", &[(0, "rays", 0.0, 1.4)])],
    },
    Template {
        name: "Tunnel",
        description: "A Shader node's tunnel with trails: it breathes with the bass and flashes on the kick",
        shader: None,
        nodes: &[(SHADER, 0.0, 0.0), ("velo.feedback", 1.0, 0.0), ("velo.output", 2.0, 0.0)],
        wires: &[(0, 1, 0), (1, 2, 0)],
        params: &[(0, "audio", &[1.0]), (1, "decay", &[0.85]), (1, "zoom", &[1.02])],
        maps: &[(1, "rotate", "hat", 0.15)],
        macros: &[("Speed", &[(0, "speed", 0.1, 2.0)]), ("Trails", &[(1, "decay", 0.6, 0.97)])],
    },
    Template {
        name: "Kaleido Bloom",
        description: "Noise folded into eight mirrored wedges, tinted by a gradient, that zoom on the kick, colors drifting with the bass",
        shader: None,
        nodes: &[("velo.noise", 0.0, 0.0), ("velo.kaleido", 1.0, 0.0), ("velo.gradient", 1.0, 1.0), ("velo.blend", 2.0, 0.0), ("velo.color", 3.0, 0.0), ("velo.output", 4.0, 0.0)],
        wires: &[(0, 1, 0), (1, 3, 0), (2, 3, 1), (3, 4, 0), (4, 5, 0)],
        params: &[(0, "speed", &[0.3]), (0, "contrast", &[1.6]), (1, "segments", &[8.0]), (2, "color_a", &[0.1, 0.9, 1.0, 1.0]), (2, "color_b", &[0.7, 0.2, 1.0, 1.0]), (3, "mode", &[3.0]), (4, "saturation", &[1.3])],
        maps: &[(1, "zoom", "kick", 0.08), (4, "hue", "bass", 0.08)],
        macros: &[("Zoom", &[(1, "zoom", 0.8, 2.6), (0, "scale", 2.0, 8.0)]), ("Hue", &[(4, "hue", -0.4, 0.4)]), ("Spin", &[(1, "spin", -0.3, 0.3)])],
    },
    Template {
        name: "Pulse Rings",
        description: "Square rings that jump on the kick, leaving spinning trails",
        shader: None,
        nodes: &[("velo.shapes", 0.0, 0.0), ("velo.feedback", 1.0, 0.0), ("velo.output", 2.0, 0.0)],
        wires: &[(0, 1, 0), (1, 2, 0)],
        params: &[(0, "sides", &[4.0]), (0, "rings", &[4.0]), (0, "tint", &[0.4, 0.9, 1.0, 1.0]), (1, "decay", &[0.9]), (1, "zoom", &[1.01]), (1, "rotate", &[0.01])],
        maps: &[(0, "size", "kick", 0.12), (1, "zoom", "snare", 0.1)],
        macros: &[("Size", &[(0, "size", 0.05, 0.3)]), ("Spin", &[(0, "spin", -1.0, 1.0)]), ("Trails", &[(1, "decay", 0.7, 0.98)])],
    },
    Template {
        name: "Liquid",
        description: "A radial gradient melted by noise, the warp swelling with the bass",
        shader: None,
        nodes: &[("velo.gradient", 0.0, 0.0), ("velo.noise", 0.0, 1.0), ("velo.displace", 1.0, 0.0), ("velo.color", 2.0, 0.0), ("velo.output", 3.0, 0.0)],
        wires: &[(0, 2, 0), (1, 2, 1), (2, 3, 0), (3, 4, 0)],
        params: &[(0, "shape", &[1.0]), (0, "color_a", &[0.9, 0.2, 0.6, 1.0]), (0, "color_b", &[0.1, 0.1, 0.5, 1.0]), (1, "speed", &[0.15]), (2, "strength", &[0.12])],
        maps: &[(2, "strength", "bass", 0.25), (3, "hue", "mid", 0.04)],
        macros: &[("Warp", &[(2, "strength", 0.0, 0.3)]), ("Hue", &[(3, "hue", -0.4, 0.4)])],
    },
];

/// Adds `template` as a new scene, selected. Every operator runs under the merge key `merge`
/// when there is one, so the caller can make the whole scene one undo step.
pub fn add(session: &mut Session, template: &Template, merge: Option<&str>) -> Result<(), String> {
    let run = |s: &mut Session, id: &str, props: serde_json::Value| match merge {
        Some(key) => s.run_merge(id, props, key),
        None => s.run(id, props),
    };
    let name = crate::i18n::t(template.name);
    run(session, "scene.add", json!({ "name": name }))?;
    let (dx, dy) = (NODE_W + SPACING, NODE_H + SPACING);
    let mut ids = Vec::new();
    for &(kind, x, y) in template.nodes {
        // a Shader node runs its preset or the shader template, named after the scene (its file too)
        let code = template.shader.and_then(|s| velo_engine::PRESETS.iter().find(|p| p.0 == s)).map_or(velo_engine::TEMPLATE, |p| p.1);
        let (code, name) = if kind == SHADER { (Some(code), Some(name)) } else { (None, None) };
        run(session, "node.add", json!({ "kind": kind, "pos": [x * dx, y * dy], "code": code, "name": name }))?;
        ids.push(session.active().and_then(|s| s.nodes.last()).map(|n| n.id.clone()).ok_or("no node")?);
    }
    for &(from, to, port) in template.wires {
        run(session, "edge.connect", json!({ "from": [ids[from], 0], "to": [ids[to], port] }))?;
    }
    for &(node, param, value) in template.params {
        let value = if let [x] = value { json!(x) } else { json!(value) };
        run(session, "node.set_param", json!({ "node": ids[node], "param": param, "value": value }))?;
    }
    for &(node, param, source, amount) in template.maps {
        run(session, "mapping.add", json!({ "node": ids[node], "param": param, "source": source }))?;
        run(session, "mapping.set", json!({ "node": ids[node], "param": param, "source": source, "amount": amount }))?;
    }
    for (index, &(name, targets)) in template.macros.iter().enumerate() {
        run(session, "macro.rename", json!({ "index": index, "name": crate::i18n::t(name) }))?;
        for &(node, param, min, max) in targets {
            run(session, "macro.assign", json!({ "index": index, "node": ids[node], "param": param, "min": min, "max": max }))?;
        }
        run(session, "macro.set", json!({ "index": index, "value": 0.5 }))?;
    }
    Ok(())
}

/// The first-run demo: the first template, Velo, as its one scene (so it goes live).
pub fn demo() -> Project {
    let mut session = Session::default();
    if let Err(e) = add(&mut session, &TEMPLATES[0], None) {
        eprintln!("velo: the {} template: {e}", TEMPLATES[0].name);
    }
    session.project
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_builds_and_renders_something() {
        assert_eq!(demo().scenes.len(), 1, "the demo is Velo alone");
        for template in &TEMPLATES {
            let mut session = Session::default();
            add(&mut session, template, None).unwrap();
            let scene = &session.project.scenes[0];
            let plan = velo_engine::plan(scene);
            assert!(plan.output.is_some(), "{}: something goes out", template.name);
            assert_eq!(scene.nodes.iter().map(|n| n.mappings.len()).sum::<usize>(), template.maps.len(), "{}", template.name);
            assert_eq!(scene.macros.iter().filter(|m| !m.targets.is_empty()).count(), template.macros.len(), "{}", template.name);
            // every value and mapping names a parameter its shader has
            for n in &scene.nodes {
                let Some(shader) = crate::live::shader(&n.kind, n.code.as_deref()) else { continue };
                let has = |p: &str| shader.controls.iter().any(|(c, _)| c.id == p);
                // (an Audio In's `hidden` is its card's, not its shader's)
                assert!(n.params.keys().all(|p| p == "hidden" || has(p)), "{}: {:?}", template.name, n.params.keys());
                assert!(n.mappings.iter().all(|m| has(&m.param)), "{}", template.name);
            }
        }
    }
}
