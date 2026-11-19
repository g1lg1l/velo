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
    /// Node, parameter, source, amount (of the parameter's range): what the music moves. A
    /// source "#2" is node 2's first output, a signal node of the template's own.
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

/// Patches agents start from: a look built from Velo's own nodes, the signal nodes moving it.
/// They aren't offered under "New scene"; the templates above are recipes too.
pub const RECIPES: [Template; 8] = [
    Template {
        name: "Ring Tunnel",
        description: "Stripes wrapped round the middle into rings that rush outward, trails behind them; the rings jump on the kick and the whole tunnel turns once every eight beats",
        shader: None,
        nodes: &[("velo.gradient", 0.0, 0.0), ("velo.polar", 1.0, 0.0), ("velo.feedback", 2.0, 0.0), ("velo.output", 3.0, 0.0), (velo_core::LFO, 0.0, 1.0)],
        wires: &[(0, 1, 0), (1, 2, 0), (2, 3, 0)],
        params: &[
            (0, "repeat", &[8.0]),
            (0, "mirror", &[1.0]),
            (0, "scroll", &[0.6]),
            (0, "color_a", &[0.05, 0.0, 0.15, 1.0]),
            (0, "color_b", &[0.2, 0.9, 1.0, 1.0]),
            (1, "twist", &[0.3]),
            (1, "zoom", &[2.5]),
            (2, "decay", &[0.8]),
            (2, "zoom", &[1.03]),
            (4, "shape", &[2.0]),
            (4, "beats", &[8.0]),
        ],
        maps: &[(1, "zoom", "kick", 0.1), (1, "turn", "#4", 1.0)],
        macros: &[],
    },
    Template {
        name: "Beat Cuts",
        description: "Three pictures cut in turn on every beat: a Beat counted round three steps picks which one a Switch shows",
        shader: None,
        nodes: &[("velo.noise", 0.0, 0.0), ("velo.shapes", 0.0, 1.0), ("velo.gradient", 0.0, 2.0), (velo_core::SWITCH, 1.0, 0.0), ("velo.output", 2.0, 0.0), (velo_core::CLOCK, 0.0, 3.0), (velo_core::COUNTER, 1.0, 3.0)],
        wires: &[(0, 3, 0), (1, 3, 1), (2, 3, 2), (3, 4, 0), (5, 6, 0)],
        params: &[(0, "speed", &[0.4]), (1, "rings", &[6.0]), (2, "shape", &[1.0]), (3, "count", &[3.0]), (6, "steps", &[3.0])],
        maps: &[(3, "value", "#6", 1.0)],
        macros: &[],
    },
    Template {
        name: "Breathing Orb",
        description: "A glowing circle that swells with the acid line, smoothed so it breathes instead of jittering, and flares on the kick",
        shader: None,
        nodes: &[("velo.shapes", 0.0, 0.0), ("velo.blur", 1.0, 0.0), ("velo.output", 2.0, 0.0), (velo_core::BAND, 0.0, 1.0), (velo_core::SMOOTH, 1.0, 1.0)],
        wires: &[(0, 1, 0), (1, 2, 0), (3, 4, 0)],
        params: &[(0, "shape", &[1.0]), (0, "rings", &[1.0]), (0, "fill", &[1.0]), (0, "size", &[0.15]), (0, "glow", &[0.7]), (0, "tint", &[1.0, 0.4, 0.8, 1.0]), (1, "radius", &[6.0]), (4, "rise_ms", &[600.0]), (4, "fall_ms", &[1200.0])],
        maps: &[(0, "size", "#4", 0.3), (0, "glow", "kick", 0.3)],
        macros: &[],
    },
    Template {
        name: "Spring Zoom",
        description: "Noise that jumps closer on the bass drum and bounces back on a spring: a band of the lows, an envelope on each hit, a spring after it",
        shader: None,
        nodes: &[("velo.noise", 0.0, 0.0), ("velo.transform", 1.0, 0.0), ("velo.output", 2.0, 0.0), (velo_core::BAND, 0.0, 1.0), (velo_core::ENVELOPE, 1.0, 1.0), (velo_core::SPRING, 2.0, 1.0)],
        wires: &[(0, 1, 0), (1, 2, 0), (3, 4, 0), (4, 5, 0)],
        params: &[(0, "noise", &[3.0]), (0, "speed", &[0.2]), (3, "low_hz", &[40.0]), (3, "high_hz", &[150.0]), (4, "release_ms", &[200.0]), (5, "bounce", &[0.7])],
        maps: &[(1, "zoom", "#5", 0.1)],
        macros: &[],
    },
    Template {
        name: "Color Cycle",
        description: "A radial gradient whose hue goes round once every twenty seconds and that brightens on the kick",
        shader: None,
        nodes: &[("velo.gradient", 0.0, 0.0), ("velo.color", 1.0, 0.0), ("velo.output", 2.0, 0.0), (velo_core::TIME, 0.0, 1.0)],
        wires: &[(0, 1, 0), (1, 2, 0)],
        params: &[(0, "shape", &[1.0]), (0, "color_a", &[1.0, 0.2, 0.3, 1.0]), (0, "color_b", &[0.1, 0.0, 0.3, 1.0]), (1, "saturation", &[1.4]), (3, "seconds", &[20.0])],
        maps: &[(1, "hue", "#3", 0.5), (1, "brightness", "kick", 0.1)],
        macros: &[],
    },
    Template {
        name: "Mirror Smoke",
        description: "Warped smoke folded into a kaleidoscope that sways back and forth over sixteen beats, with trails whose color drifts on the hats",
        shader: None,
        nodes: &[("velo.noise", 0.0, 0.0), ("velo.kaleido", 1.0, 0.0), ("velo.feedback", 2.0, 0.0), ("velo.output", 3.0, 0.0), (velo_core::LFO, 0.0, 1.0)],
        wires: &[(0, 1, 0), (1, 2, 0), (2, 3, 0)],
        params: &[
            (0, "speed", &[0.2]),
            (0, "warp", &[0.6]),
            (0, "contrast", &[1.6]),
            (0, "color_a", &[0.02, 0.0, 0.08, 1.0]),
            (0, "color_b", &[1.0, 0.35, 0.7, 1.0]),
            (1, "segments", &[6.0]),
            (2, "decay", &[0.9]),
            (2, "zoom", &[1.01]),
            (4, "shape", &[1.0]),
            (4, "beats", &[16.0]),
        ],
        maps: &[(1, "spin", "#4", 0.3), (2, "hue", "hat", 0.2)],
        macros: &[],
    },
    Template {
        name: "Spectrum Halo",
        description: "The sound's spectrum drawn round a circle, its echoes spreading outward and turning to a new random angle every four beats",
        shader: None,
        nodes: &[("velo.waveform", 0.0, 0.0), ("velo.feedback", 1.0, 0.0), ("velo.output", 2.0, 0.0), (velo_core::RANDOM, 0.0, 1.0)],
        wires: &[(0, 1, 0), (1, 2, 0)],
        params: &[(0, "mode", &[1.0]), (1, "decay", &[0.85]), (1, "zoom", &[1.02]), (3, "beats", &[4.0]), (3, "glide_ms", &[800.0])],
        maps: &[(1, "rotate", "#3", 0.3)],
        macros: &[],
    },
    Template {
        name: "Particle Bursts",
        description: "Golden sparks streaming from the middle, thrown out in bursts on every kick, leaving trails",
        shader: None,
        nodes: &[(velo_core::PARTICLES, 0.0, 0.0), ("velo.feedback", 1.0, 0.0), ("velo.output", 2.0, 0.0)],
        wires: &[(0, 1, 0), (1, 2, 0)],
        params: &[(0, "emission", &[300.0]), (0, "speed", &[0.5]), (0, "spread", &[1.0]), (0, "size", &[0.012]), (0, "tint", &[1.0, 0.7, 0.3, 1.0]), (1, "decay", &[0.85])],
        maps: &[(0, "emission", "kick", 0.5)],
        macros: &[],
    },
];

impl Template {
    /// The patch in words, for an agent: what's wired into what, what moves what.
    pub fn outline(&self) -> String {
        let label = |i: usize| velo_core::NODE_KINDS.iter().find(|k| k.id == self.nodes[i].0).map_or(self.nodes[i].0, |k| k.label);
        let input = |i: usize, port: u8| velo_core::NODE_KINDS.iter().find(|k| k.id == self.nodes[i].0).and_then(|k| k.inputs.get(port as usize)).copied().unwrap_or("in");
        let mut parts: Vec<String> = self.wires.iter().map(|&(a, b, port)| if port == 0 { format!("{} → {}", label(a), label(b)) } else { format!("{} → {} ({})", label(a), label(b), input(b, port)) }).collect();
        parts.extend(self.maps.iter().map(|&(node, param, source, _)| {
            let source = source.strip_prefix('#').and_then(|i| i.parse().ok()).map_or(source, label);
            format!("{source} moves {} {param}", label(node))
        }));
        parts.join("; ")
    }
}

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
        let source = source.strip_prefix('#').and_then(|i| i.parse::<usize>().ok()).map_or_else(|| source.to_string(), |i| format!("{}/0", ids[i]));
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
        for template in TEMPLATES.iter().chain(&RECIPES) {
            let mut session = Session::default();
            add(&mut session, template, None).unwrap();
            let scene = &session.project.scenes[0];
            let plan = velo_engine::plan(scene);
            assert!(plan.output.is_some(), "{}: something goes out", template.name);
            assert_eq!(scene.edges.len(), template.wires.len(), "{}: every wire holds", template.name);
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
