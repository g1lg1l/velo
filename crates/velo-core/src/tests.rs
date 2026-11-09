use serde_json::{Value, json};

use crate::*;

fn ok(s: &mut Session, id: &str, props: Value) {
    s.run(id, props).unwrap_or_else(|e| panic!("{id}: {e}"));
}

fn names(s: &Session) -> Vec<&str> {
    s.project.scenes.iter().map(|c| c.name.as_str()).collect()
}

/// A session with one scene, selected.
fn session() -> Session {
    let mut s = Session::default();
    ok(&mut s, "scene.add", json!({}));
    Session::with_project(s.project)
}

#[test]
fn undo_redo_round_trip() {
    let mut s = session();
    let original = s.project.clone();
    ok(&mut s, "scene.rename", json!({"name": "Intro"}));
    assert_eq!(names(&s), ["Intro"]);
    assert_eq!(s.undo_label(), Some("Rename Scene"));
    ok(&mut s, "edit.undo", json!({}));
    assert_eq!(s.project, original);
    assert_eq!(s.redo_label(), Some("Rename Scene"));
    ok(&mut s, "edit.redo", json!({}));
    assert_eq!(names(&s), ["Intro"]);
    assert!(s.run("edit.redo", json!({})).is_err(), "nothing left to redo");
}

#[test]
fn drags_merge_into_one_step() {
    let mut s = Session::default();
    for fps in [30.0, 40.0, 50.0] {
        s.run_merge("project.set_canvas", json!({"fps": fps}), "fps").unwrap();
    }
    s.close_merge();
    s.run_merge("project.set_canvas", json!({"fps": 24.0}), "fps").unwrap();
    ok(&mut s, "edit.undo", json!({}));
    assert_eq!(s.project.canvas.fps, 50);
    ok(&mut s, "edit.undo", json!({}));
    assert_eq!(s.project, Project::default());
}

#[test]
fn bad_input_changes_nothing() {
    let mut s = session();
    let original = s.project.clone();
    assert!(s.run("scene.rename", json!({})).is_err(), "a name is required");
    assert!(s.run("scene.rename", json!({"name": "   "})).is_err());
    assert!(s.run("scene.rename", json!({"scene": "s_nope", "name": "X"})).is_err());
    assert!(s.run("scene.select", json!({})).is_err());
    assert!(s.run("scene.select", json!({"scene": "s_nope"})).is_err());
    assert!(s.run("scene.remove", json!({"scene": "s_nope"})).is_err());
    assert!(s.run("project.set_canvas", json!({"fps": "fast"})).is_err());
    assert!(s.run("does.not_exist", json!({})).is_err());
    assert_eq!(s.project, original);
    assert_eq!(s.undo_label(), None);
}

#[test]
fn values_are_clamped_and_noops_skip_history() {
    let mut s = Session::default();
    ok(&mut s, "project.set_canvas", json!({"width": 1e9, "height": 0, "fps": 1000}));
    assert_eq!(s.project.canvas, Canvas { width: 8192, height: 16, fps: 240 });
    ok(&mut s, "edit.undo", json!({}));
    ok(&mut s, "project.set_canvas", json!({"fps": 60}));
    assert_eq!(s.undo_label(), None, "setting the same value is not an undo step");
}

#[test]
fn scenes_keep_their_ids_through_add_select_rename_remove() {
    let mut s = Session::default();
    assert!(s.run("scene.remove", json!({})).is_err(), "nothing to delete");
    ok(&mut s, "scene.add", json!({}));
    ok(&mut s, "scene.add", json!({}));
    ok(&mut s, "scene.add", json!({"name": "Drop"}));
    assert_eq!(names(&s), ["Scene 1", "Scene 2", "Drop"]);
    let ids: Vec<String> = s.project.scenes.iter().map(|c| c.id.clone()).collect();
    for id in &ids {
        assert!(id.len() == 6 && id.starts_with("s_") && id[2..].chars().all(|c| c.is_ascii_alphanumeric()), "{id}");
    }
    assert!(ids.iter().enumerate().all(|(i, id)| !ids[..i].contains(id)), "ids are unique");
    assert_eq!(s.active_scene.as_ref(), Some(&ids[2]), "a new scene is selected");
    // selecting is not an edit: no undo step for it
    let before = s.project.clone();
    ok(&mut s, "scene.select", json!({"scene": ids[0]}));
    assert_eq!(s.undo_label(), Some("New Scene"));
    assert_eq!(s.project, before, "selecting doesn't touch the document (nothing to save)");
    ok(&mut s, "scene.rename", json!({"scene": ids[1], "name": "Build"}));
    assert_eq!(s.project.scenes[1].id, ids[1], "renaming keeps the id");
    ok(&mut s, "scene.remove", json!({}));
    assert_eq!(names(&s), ["Build", "Drop"]);
    assert_eq!(s.active_scene.as_ref(), Some(&ids[1]), "the next one takes over");
    ok(&mut s, "scene.remove", json!({"scene": ids[2]}));
    assert_eq!(s.active_scene.as_ref(), Some(&ids[1]), "deleting another keeps the selection");
    ok(&mut s, "scene.remove", json!({}));
    assert!(s.project.scenes.is_empty() && s.active_scene.is_none());
    ok(&mut s, "edit.undo", json!({}));
    assert_eq!(names(&s), ["Build"]);
    assert_eq!(s.active().map(|c| c.id.as_str()), Some(ids[1].as_str()), "undo brings the scene back, selected");
}

#[test]
fn text_files_become_text_list_items() {
    assert_eq!(file_items(b"One\r\n  Two\n\n", "txt"), ["One", "Two"], "a line an item");
    assert_eq!(file_items(b"Intro\nfirst line\n\n\nDrop\n", "TXT"), ["Intro\nfirst line", "Drop"], "empty lines part blocks");
    // Excel's Italian CSV: semicolons, quotes, a newline in a cell, Windows-1252
    let csv = b"Nome;Ruolo\n\"Rossi; Mario\";\"DJ \"\"set\"\"\nlive\"\n\xC8 l\x92ora;\n";
    assert_eq!(file_items(csv, "csv"), ["Nome\nRuolo", "Rossi; Mario\nDJ \"set\"\nlive", "È l’ora"]);
    assert_eq!(file_items("\u{FEFF}a,b\tc\r\n".as_bytes(), "tsv"), ["a,b\nc"]);
}

#[test]
fn scenes_move_in_one_undo_step() {
    let mut s = Session::default();
    for name in ["A", "B", "C"] {
        ok(&mut s, "scene.add", json!({"name": name}));
    }
    let a = s.project.scenes[0].id.clone();
    ok(&mut s, "scene.move", json!({"scene": a, "to": 2}));
    assert_eq!(names(&s), ["B", "C", "A"]);
    assert_eq!(s.active().map(|c| c.name.as_str()), Some("C"), "the selection stays on its scene");
    ok(&mut s, "scene.move", json!({"to": 99}));
    assert_eq!(names(&s), ["B", "A", "C"], "past the end is the last place");
    ok(&mut s, "edit.undo", json!({}));
    assert_eq!(names(&s), ["B", "C", "A"]);
    ok(&mut s, "edit.undo", json!({}));
    assert_eq!(names(&s), ["A", "B", "C"]);
    assert!(s.run("scene.move", json!({"scene": "s_nope", "to": 0})).is_err());
}

#[test]
fn project_round_trips_through_ron() {
    let mut s = session();
    ok(&mut s, "scene.add", json!({"name": "Drop"}));
    ok(&mut s, "project.set_canvas", json!({"width": 1280, "height": 720, "fps": 30}));
    let text = ron::ser::to_string_pretty(&s.project, ron::ser::PrettyConfig::default()).unwrap();
    assert_eq!(ron::from_str::<Project>(&text).unwrap(), s.project);
    // a file missing fields takes the defaults
    let sparse: Project = ron::from_str(r#"(scenes: [(id: "s_7Kq2", name: "Drop")])"#).unwrap();
    assert_eq!((sparse.format, sparse.canvas), (FORMAT, Canvas::default()));
}

#[test]
fn registry_is_consistent() {
    for (i, op) in OPERATORS.iter().enumerate() {
        assert!(OPERATORS[..i].iter().all(|o| o.id != op.id), "duplicate id {}", op.id);
        assert!(serde_json::from_str::<Value>(op.example).is_ok(), "{} example", op.id);
    }
}

#[test]
fn norm_round_trips() {
    for info in &CANVAS {
        let v = info.from_norm(0.37);
        assert!((info.from_norm(info.to_norm(v)) - v).abs() <= v.abs() * 1e-4 + 1e-4);
        assert_eq!(info.clamp(info.default), info.default);
    }
}

// ---- graph ----------------------------------------------------------------

/// Adds a node to the active scene and returns its id.
fn add(s: &mut Session, kind: &str, x: f32) -> String {
    ok(s, "node.add", json!({ "kind": kind, "pos": [x, 0.0] }));
    s.active().unwrap().nodes.last().unwrap().id.clone()
}

fn connect(s: &mut Session, from: &str, to: &str, port: u8) -> Result<(), String> {
    s.run("edge.connect", json!({ "from": [from, 0], "to": [to, port] }))
}

fn wires(s: &Session) -> Vec<(String, String, u8)> {
    s.active().unwrap().edges.iter().map(|e| (e.from.0.clone(), e.to.0.clone(), e.to.1)).collect()
}

#[test]
fn nodes_add_with_fresh_ids_and_names() {
    let mut s = session();
    let a = add(&mut s, "velo.noise", 0.0);
    let b = add(&mut s, "velo.noise", 200.0);
    ok(&mut s, "node.add", json!({ "kind": "velo.blur", "name": "Rumore" }));
    let names: Vec<&str> = s.active().unwrap().nodes.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["Noise", "Noise 2", "Rumore"], "a taken name gets a number; the UI may pass its own (translated) name");
    assert!(a != b && a.len() == 6 && a.starts_with("n_"), "{a} {b}");
    assert_eq!(s.undo_label(), Some("Add Node"));
    assert!(s.run("node.add", json!({ "kind": "velo.nope" })).is_err());
    assert!(s.run("node.add", json!({ "kind": "velo.noise", "scene": "s_nope" })).is_err());
    ok(&mut s, "node.rename", json!({ "node": a, "name": " Tunnel " }));
    assert_eq!(s.active().unwrap().node(&a).unwrap().name, "Tunnel");
    assert!(s.run("node.rename", json!({ "node": a, "name": "" })).is_err());
}

#[test]
fn cables_tie_slide_and_untie() {
    let mut s = session();
    let a = add(&mut s, "velo.noise", 0.0);
    let cables = [format!("{a}/0"), Tie::line(&a, "n_x/1")];
    ok(&mut s, "tie.add", json!({ "pos": [100.0, 20.0], "cables": cables }));
    let tie = s.active().unwrap().ties[0].clone();
    assert!(tie.id.starts_with("t_") && tie.cables == cables, "{tie:?}");
    assert!(s.run("tie.add", json!({ "pos": [0, 0], "cables": [] })).is_err(), "a tie holds something");
    for x in [110.0, 130.0] {
        s.run_merge("tie.move", json!({ "tie": tie.id, "pos": [x, 20.0] }), "tie.move").unwrap();
    }
    s.close_merge();
    assert_eq!(s.active().unwrap().ties[0].pos, [130.0, 20.0]);
    ok(&mut s, "edit.undo", json!({}));
    assert_eq!(s.active().unwrap().ties[0].pos, [100.0, 20.0], "the slide undoes at once");
    ok(&mut s, "tie.remove", json!({ "tie": tie.id }));
    assert!(s.active().unwrap().ties.is_empty());
    assert!(s.run("tie.remove", json!({ "tie": tie.id })).is_err());
    // a project with ties saves them, one without has no trace of them
    ok(&mut s, "edit.undo", json!({}));
    let text = serde_json::to_string(&s.project).unwrap();
    assert!(text.contains("\"ties\""));
    let back: Project = serde_json::from_str(&text).unwrap();
    assert_eq!(back.scenes[0].ties, s.active().unwrap().ties);
}

#[test]
fn a_node_drag_is_one_undo_step() {
    let mut s = session();
    let a = add(&mut s, "velo.noise", 0.0);
    for x in [10.0, 20.0, 30.0] {
        s.run_merge("node.move", json!({ "node": a, "pos": [x, 5.0] }), "node.move").unwrap();
    }
    s.close_merge();
    assert_eq!(s.active().unwrap().node(&a).unwrap().pos, [30.0, 5.0]);
    assert_eq!(s.undo_label(), Some("Move Node"));
    ok(&mut s, "edit.undo", json!({}));
    assert_eq!(s.active().unwrap().node(&a).unwrap().pos, [0.0, 0.0], "the whole drag undoes at once");
    // a group drag moves every node by the same offset
    let b = add(&mut s, "velo.blur", 200.0);
    for _ in 0..2 {
        s.run_merge("node.move", json!({ "nodes": [a, b], "by": [5.0, -1.0] }), "node.move").unwrap();
    }
    s.close_merge();
    let pos = |s: &Session, id: &str| s.active().unwrap().node(id).unwrap().pos;
    assert_eq!((pos(&s, &a), pos(&s, &b)), ([10.0, -2.0], [210.0, -2.0]));
    assert!(s.run("node.move", json!({ "node": a })).is_err(), "pos is required");
    assert!(s.run("node.move", json!({ "nodes": [a, "n_nope"], "by": [1, 1] })).is_err());
    assert_eq!(pos(&s, &a), [10.0, -2.0], "a failed move moves nothing");
    ok(&mut s, "edit.undo", json!({}));
    ok(&mut s, "edit.undo", json!({}));
    ok(&mut s, "edit.undo", json!({}));
    assert!(s.active().unwrap().nodes.is_empty());
}

#[test]
fn a_blend_shows_the_inputs_it_asks_for_and_every_wired_one() {
    let mut s = session();
    let noise = add(&mut s, "velo.noise", 0.0);
    let blend = add(&mut s, BLEND, 200.0);
    let shown = |s: &Session| inputs_shown(s.active().unwrap(), s.active().unwrap().node(&blend).unwrap());
    assert_eq!(shown(&s), 2, "two to start");
    ok(&mut s, "node.set_param", json!({ "node": blend, "param": "inputs", "value": 5.0 }));
    assert_eq!(shown(&s), 5);
    connect(&mut s, &noise, &blend, 9).unwrap();
    assert_eq!(shown(&s), 10, "a wire into the tenth keeps it shown");
    let scene = s.active().unwrap();
    assert_eq!(engine_param(scene, scene.node(&blend).unwrap(), "inputs"), Some([10.0, 0.0, 0.0, 0.0]), "and the engine reads it");
    let blur = add(&mut s, "velo.blur", 400.0);
    assert_eq!(inputs_shown(s.active().unwrap(), s.active().unwrap().node(&blur).unwrap()), 1, "any other kind: all it has");
    // a Switch shows its count, up to sixteen
    let switch = add(&mut s, SWITCH, 600.0);
    ok(&mut s, "node.set_param", json!({ "node": switch, "param": "count", "value": 12.0 }));
    assert_eq!(inputs_shown(s.active().unwrap(), s.active().unwrap().node(&switch).unwrap()), 12);
    connect(&mut s, &noise, &switch, 15).unwrap();
    assert_eq!(inputs_shown(s.active().unwrap(), s.active().unwrap().node(&switch).unwrap()), 16, "the sixteenth wired");
}

#[test]
fn wires_follow_the_rules() {
    let mut s = session();
    let noise = add(&mut s, "velo.noise", 0.0);
    let blur = add(&mut s, "velo.blur", 200.0);
    let color = add(&mut s, "velo.color", 400.0);
    let feedback = add(&mut s, "velo.feedback", 600.0);
    let blend = add(&mut s, "velo.blend", 800.0);
    connect(&mut s, &noise, &blur, 0).unwrap();
    connect(&mut s, &blur, &color, 0).unwrap();
    assert!(connect(&mut s, &blur, &blur, 0).is_err(), "no node feeds itself");
    assert!(connect(&mut s, &color, &blur, 1).is_err(), "Blur has one input");
    assert!(s.run("edge.connect", json!({ "from": [noise, 1], "to": [blur, 0] })).is_err(), "Noise has one output");
    assert!(connect(&mut s, &color, &noise, 0).is_err(), "Noise has no input");
    // color → blur would close blur → color → blur
    let before = s.project.clone();
    assert!(connect(&mut s, &color, &blur, 0).unwrap_err().contains("Feedback"));
    assert_eq!(s.project, before, "a refused wire changes nothing");
    // …but a loop through Feedback is the point of Feedback
    connect(&mut s, &color, &feedback, 0).unwrap();
    connect(&mut s, &feedback, &blend, 0).unwrap();
    connect(&mut s, &blend, &feedback, 0).unwrap();
    connect(&mut s, &noise, &blend, 1).unwrap();
    // an input takes one wire: connecting again replaces it
    connect(&mut s, &blur, &blend, 1).unwrap();
    assert_eq!(s.active().unwrap().wire_into(&blend, 1).unwrap().from.0, blur);
    assert_eq!(wires(&s).len(), 5);
    ok(&mut s, "edge.disconnect", json!({ "to": [blend, 1] }));
    assert!(s.active().unwrap().wire_into(&blend, 1).is_none());
    assert!(s.run("edge.disconnect", json!({ "to": [blend, 1] })).is_err(), "nothing left to unplug");
    assert_eq!(s.undo_label(), Some("Disconnect"));
    // numbers into numbers, pictures into pictures; a Probe is the signal node a picture goes into
    let (probe, remap) = (add(&mut s, PROBE, 1000.0), add(&mut s, REMAP, 1200.0));
    connect(&mut s, &noise, &probe, 0).unwrap();
    assert!(connect(&mut s, &noise, &remap, 0).unwrap_err().contains("Probe"));
    connect(&mut s, &probe, &remap, 0).unwrap();
    assert!(connect(&mut s, &remap, &probe, 0).is_err(), "a signal doesn't go into a Probe");
    assert!(connect(&mut s, &probe, &blur, 0).is_err(), "nor its signal into a picture's input");
}

#[test]
fn removing_a_node_takes_its_wires_and_undo_brings_both_back() {
    let mut s = session();
    let noise = add(&mut s, "velo.noise", 0.0);
    let blur = add(&mut s, "velo.blur", 200.0);
    let out = add(&mut s, "velo.output", 400.0);
    connect(&mut s, &noise, &blur, 0).unwrap();
    connect(&mut s, &blur, &out, 0).unwrap();
    let before = s.project.clone();
    ok(&mut s, "node.remove", json!({ "node": blur }));
    assert!(s.active().unwrap().node(&blur).is_none() && wires(&s).is_empty());
    assert!(s.run("node.remove", json!({ "node": blur })).is_err());
    ok(&mut s, "edit.undo", json!({}));
    assert_eq!(s.project, before);
}

#[test]
fn graphs_round_trip_through_ron() {
    let mut s = session();
    let noise = add(&mut s, "velo.noise", 0.0);
    let out = add(&mut s, "velo.output", 240.5);
    connect(&mut s, &noise, &out, 0).unwrap();
    let text = ron::ser::to_string_pretty(&s.project, ron::ser::PrettyConfig::default()).unwrap();
    assert_eq!(ron::from_str::<Project>(&text).unwrap(), s.project);
    // scenes from before the canvas load as empty graphs
    let old: Project = ron::from_str(r#"(scenes: [(id: "s_7Kq2", name: "Drop")])"#).unwrap();
    assert!(old.scenes[0].nodes.is_empty() && old.scenes[0].edges.is_empty());
}

#[test]
fn the_catalog_is_consistent() {
    for (i, k) in NODE_KINDS.iter().enumerate() {
        assert!(NODE_KINDS[..i].iter().all(|o| o.id != k.id), "duplicate kind {}", k.id);
        assert!(k.id.starts_with("velo."), "{}", k.id);
        let (ins, outs) = (k.inputs.len(), k.outputs.len());
        match k.category {
            // a generator reads no picture, bar Particles': its emit and sprite feed it, they aren't what it shows
            Category::Generator => assert_eq!((ins, outs), (if k.id == PARTICLES { 2 } else { 0 }, 1), "{}", k.id),
            Category::Effect => assert!(ins >= 1 && outs == 1, "{}", k.id),
            Category::Composite | Category::Control => assert!(ins >= 2 && outs == 1, "{}", k.id),
            Category::Output => assert!(ins >= 1 && outs == 0, "{}", k.id),
            // the signal engine reads two inputs at most
            Category::Signal => assert!(ins <= 2 && outs >= 1, "{}", k.id),
        }
    }
    assert!(node_kind(FEEDBACK).is_some());
}

#[test]
fn the_menu_shows_each_group_once_logic_after_the_signals_that_feed_it() {
    let mut groups: Vec<&str> = Vec::new();
    for g in NODE_KINDS.iter().map(NodeKind::group) {
        if groups.last() != Some(&g) {
            assert!(!groups.contains(&g), "{g} comes twice in the catalog's order");
            groups.push(g);
        }
    }
    assert_eq!(groups[groups.len() - 2..], ["Signals", "Logic"]);
    assert_eq!((node_kind(AUDIO).unwrap().group(), node_kind(COMPARE).unwrap().group(), node_kind(TIMER).unwrap().group()), ("Signals", "Logic", "Logic"));
}

#[test]
fn a_parameter_drag_is_one_undo_step_and_values_round_trip() {
    let mut s = session();
    let noise = add(&mut s, "velo.noise", 0.0);
    for v in [1.0, 2.0, 3.5] {
        s.run_merge("node.set_param", json!({ "node": noise, "param": "scale", "value": v }), "param").unwrap();
    }
    s.close_merge();
    let params = |s: &Session| s.active().unwrap().node(&noise).unwrap().params.clone();
    assert_eq!(params(&s)["scale"], [3.5, 0.0, 0.0, 0.0]);
    assert_eq!(s.undo_label(), Some("Set Parameter"));
    ok(&mut s, "node.set_param", json!({ "node": noise, "param": "tint", "value": [1.0, 0.5, 0.25, 1.0] }));
    ok(&mut s, "node.set_param", json!({ "node": noise, "param": "radial", "value": true }));
    assert_eq!((params(&s)["tint"], params(&s)["radial"][0]), ([1.0, 0.5, 0.25, 1.0], 1.0));
    // bad values change nothing
    let before = s.project.clone();
    assert!(s.run("node.set_param", json!({ "node": noise, "param": "scale", "value": [] })).is_err());
    assert!(s.run("node.set_param", json!({ "node": noise, "param": "scale", "value": [1, 2, 3, 4, 5] })).is_err());
    assert!(s.run("node.set_param", json!({ "node": noise, "param": "", "value": 1 })).is_err());
    assert!(s.run("node.set_param", json!({ "node": "n_nope", "param": "scale", "value": 1 })).is_err());
    assert_eq!(s.project, before);
    // saved and loaded; a node without changes writes no params at all
    let text = ron::ser::to_string_pretty(&s.project, ron::ser::PrettyConfig::default()).unwrap();
    assert_eq!(ron::from_str::<Project>(&text).unwrap(), s.project);
    add(&mut s, "velo.blur", 200.0);
    let plain = ron::ser::to_string(&s.active().unwrap().nodes[1]).unwrap();
    assert!(!plain.contains("params"), "{plain}");
    // the drag undoes at once, the later edits one by one
    ok(&mut s, "edit.undo", json!({}));
    ok(&mut s, "edit.undo", json!({}));
    ok(&mut s, "edit.undo", json!({}));
    assert_eq!(params(&s).len(), 1);
    ok(&mut s, "edit.undo", json!({}));
    assert!(params(&s).is_empty());
    // a duplicate starts from its original's values
    ok(&mut s, "node.add", json!({ "kind": "velo.noise", "params": { "scale": [9.0, 0.0, 0.0, 0.0] } }));
    assert_eq!(s.active().unwrap().nodes.last().unwrap().params["scale"][0], 9.0);
}

#[test]
fn mappings_add_once_change_in_one_drag_step_and_go() {
    let mut s = session();
    let kaleido = add(&mut s, "velo.kaleido", 0.0);
    let maps = |s: &Session| s.active().unwrap().node(&kaleido).unwrap().mappings.clone();
    let zoom_kick = json!({ "node": kaleido, "param": "zoom", "source": "kick" });
    ok(&mut s, "mapping.add", zoom_kick.clone());
    // the kick of the scene's Audio In, added for it
    let audio = s.active().unwrap().nodes.iter().find(|n| n.kind == AUDIO).map(|n| n.id.clone()).expect("an Audio In");
    assert_eq!(maps(&s), vec![Mapping { param: "zoom".into(), source: format!("{audio}/4"), amount: 0.5, attack_ms: 5.0, release_ms: 150.0, invert: false }]);
    assert_eq!(s.undo_label(), Some("Map Source"));
    // the same source again: nothing new, no undo step
    ok(&mut s, "mapping.add", zoom_kick.clone());
    assert_eq!(maps(&s).len(), 1);
    ok(&mut s, "mapping.add", json!({ "node": kaleido, "param": "zoom", "source": "bass" }));
    assert_eq!(maps(&s).len(), 2, "several sources on one parameter");
    assert_eq!(s.active().unwrap().nodes.iter().filter(|n| n.kind == AUDIO).count(), 1, "one Audio In for both");
    // an Alt-drag of the amount is one step, clamped
    for a in [0.6, 0.9, 1.7] {
        s.run_merge("mapping.set", json!({ "node": kaleido, "param": "zoom", "source": "kick", "amount": a }), "amount").unwrap();
    }
    s.close_merge();
    assert_eq!(maps(&s)[0].amount, 1.0);
    ok(&mut s, "mapping.set", json!({ "node": kaleido, "param": "zoom", "source": "kick", "attack_ms": -3.0, "release_ms": 1e9, "invert": true }));
    assert_eq!((maps(&s)[0].attack_ms, maps(&s)[0].release_ms, maps(&s)[0].invert), (0.0, Mapping::MAX_MS, true));
    // bad input changes nothing
    let before = s.project.clone();
    assert!(s.run("mapping.add", json!({ "node": kaleido, "param": "zoom", "source": "cowbell" })).is_err());
    assert!(s.run("mapping.add", json!({ "node": kaleido, "param": " ", "source": "kick" })).is_err());
    assert!(s.run("mapping.add", json!({ "node": "n_nope", "param": "zoom", "source": "kick" })).is_err());
    assert!(s.run("mapping.set", json!({ "node": kaleido, "param": "spin", "source": "kick", "amount": 0.1 })).is_err(), "not mapped");
    assert!(s.run("mapping.remove", json!({ "node": kaleido, "param": "spin", "source": "kick" })).is_err());
    assert_eq!(s.project, before);
    // saved and loaded
    let text = ron::ser::to_string_pretty(&s.project, ron::ser::PrettyConfig::default()).unwrap();
    assert_eq!(ron::from_str::<Project>(&text).unwrap(), s.project);
    ok(&mut s, "mapping.remove", zoom_kick);
    assert_eq!(maps(&s).iter().map(|m| m.source.clone()).collect::<Vec<_>>(), [format!("{audio}/1")]);
    // undo walks back: remove, the settings, the drag, the second source
    for left in [2, 2, 2, 1] {
        ok(&mut s, "edit.undo", json!({}));
        assert_eq!(maps(&s).len(), left);
    }
    assert_eq!(maps(&s)[0].amount, 0.5, "the drag undoes at once");
}

#[test]
fn files_from_before_mappings_load_and_nodes_without_them_write_none() {
    let old: Project = ron::from_str(r#"(scenes: [(id: "s_1", name: "Drop", nodes: [(id: "n_1", kind: "velo.noise", name: "Noise", pos: (0.0, 0.0))])])"#).unwrap();
    assert!(old.scenes[0].nodes[0].mappings.is_empty());
    let plain = ron::ser::to_string(&old.scenes[0].nodes[0]).unwrap();
    assert!(!plain.contains("mappings"), "{plain}");
    let m = Mapping { param: "zoom".into(), source: "kick".into(), amount: 0.5, attack_ms: 5.0, release_ms: 150.0, invert: false };
    assert!(!ron::ser::to_string(&m).unwrap().contains("invert"), "invert is written only when on");
}

#[test]
fn launch_settings_set_clamp_undo_and_load_from_old_files() {
    let mut s = session();
    let launch = |s: &Session| s.active().unwrap().launch;
    assert_eq!(launch(&s), Launch { transition: Transition::Crossfade, duration: 4.0, quantize: Quantize::Bar }, "a bar's wait, a crossfade of a bar");
    ok(&mut s, "scene.set_launch", json!({ "transition": "Cut", "quantize": "Bars4" }));
    assert_eq!(launch(&s), Launch { transition: Transition::Cut, duration: 4.0, quantize: Quantize::Bars4 });
    assert_eq!(s.undo_label(), Some("Set Launch"));
    ok(&mut s, "scene.set_launch", json!({ "duration": 99.6 }));
    assert_eq!(launch(&s).duration, 32.0, "clamped to whole beats");
    ok(&mut s, "scene.set_launch", json!({ "duration": 2.4 }));
    assert_eq!(launch(&s).duration, 2.0);
    let before = s.project.clone();
    assert!(s.run("scene.set_launch", json!({ "quantize": "Sometimes" })).is_err());
    assert!(s.run("scene.set_launch", json!({ "scene": "s_nope", "duration": 1 })).is_err());
    assert_eq!(s.project, before);
    ok(&mut s, "edit.undo", json!({}));
    ok(&mut s, "edit.undo", json!({}));
    assert_eq!(launch(&s).duration, 4.0);
    // saved and loaded; files from before launching take the defaults
    let text = ron::ser::to_string_pretty(&s.project, ron::ser::PrettyConfig::default()).unwrap();
    assert_eq!(ron::from_str::<Project>(&text).unwrap(), s.project);
    let old: Project = ron::from_str(r#"(scenes: [(id: "s_1", name: "Drop")])"#).unwrap();
    assert_eq!(old.scenes[0].launch, Launch::default());
    assert_eq!(Quantize::ALL.map(Quantize::beats), [0.0, 1.0, 4.0, 16.0]);
}

// ---- macros, variations, bindings -----------------------------------------

#[test]
fn macros_set_rename_assign_and_undo() {
    let mut s = session();
    let kaleido = add(&mut s, "velo.kaleido", 0.0);
    let macros = |s: &Session| s.active().unwrap().macros.clone();
    assert_eq!(macros(&s), <[Macro; MACROS]>::default(), "eight unnamed macros at 0");
    // a knob drag is one undo step, clamped
    for v in [0.2, 0.5, 1.4] {
        s.run_merge("macro.set", json!({ "index": 2, "value": v }), "macro 2").unwrap();
    }
    s.close_merge();
    assert_eq!(macros(&s)[2].value, 1.0);
    assert_eq!(s.undo_label(), Some("Set Macro"));
    ok(&mut s, "macro.rename", json!({ "index": 2, "name": " Energy " }));
    assert_eq!(macros(&s)[2].name, "Energy");
    // assigned from a parameter: its stored value → the maximum the inspector passes
    ok(&mut s, "node.set_param", json!({ "node": kaleido, "param": "zoom", "value": 1.5 }));
    ok(&mut s, "macro.assign", json!({ "index": 2, "node": kaleido, "param": "zoom", "max": 4.0 }));
    assert_eq!(macros(&s)[2].targets, [MacroTarget { node: kaleido.clone(), param: "zoom".into(), min: 1.5, max: 4.0 }]);
    assert_eq!(s.active().unwrap().driver(&kaleido, "zoom").map(|(i, t)| (i, t.at(0.5))), Some((2, 2.75)));
    // another macro takes it over: one driver per parameter
    ok(&mut s, "macro.assign", json!({ "index": 0, "node": kaleido, "param": "zoom", "min": 1.0, "max": 2.0 }));
    assert!(macros(&s)[2].targets.is_empty() && macros(&s)[0].targets.len() == 1);
    ok(&mut s, "macro.assign", json!({ "index": 0, "node": kaleido, "param": "spin" }));
    assert_eq!((macros(&s)[0].targets[1].min, macros(&s)[0].targets[1].max), (0.0, 1.0), "nothing stored, no range given: 0 → 1");
    // a range drag is one step
    for max in [2.5, 3.0] {
        s.run_merge("macro.target", json!({ "index": 0, "node": kaleido, "param": "zoom", "max": max }), "range").unwrap();
    }
    s.close_merge();
    assert_eq!(macros(&s)[0].targets[0].max, 3.0);
    // bad input changes nothing
    let before = s.project.clone();
    assert!(s.run("macro.set", json!({ "index": 8, "value": 0.5 })).is_err(), "eight macros");
    assert!(s.run("macro.set", json!({ "index": 0 })).is_err(), "a value is required");
    assert!(s.run("macro.assign", json!({ "index": 0, "node": "n_nope", "param": "zoom" })).is_err());
    assert!(s.run("macro.assign", json!({ "index": 9, "node": kaleido, "param": "zoom" })).is_err());
    assert!(s.run("macro.target", json!({ "index": 1, "node": kaleido, "param": "zoom", "max": 2 })).is_err(), "macro 1 doesn't drive it");
    assert!(s.run("macro.unassign", json!({ "index": 1, "node": kaleido, "param": "zoom" })).is_err());
    assert_eq!(s.project, before);
    ok(&mut s, "macro.unassign", json!({ "index": 0, "node": kaleido, "param": "spin" }));
    assert_eq!(macros(&s)[0].targets.len(), 1);
    // deleting the node takes its targets; undo brings both back
    let before = s.project.clone();
    ok(&mut s, "node.remove", json!({ "node": kaleido }));
    assert!(macros(&s)[0].targets.is_empty());
    ok(&mut s, "edit.undo", json!({}));
    assert_eq!(s.project, before);
    // undo walks back to the start
    while s.undo_label().is_some() {
        ok(&mut s, "edit.undo", json!({}));
    }
    assert_eq!(macros(&s), <[Macro; MACROS]>::default());
}

#[test]
fn variations_store_recall_morph_and_go() {
    let mut s = session();
    let kaleido = add(&mut s, "velo.kaleido", 0.0);
    let values = |s: &Session| s.active().unwrap().macros.each_ref().map(|m| m.value);
    ok(&mut s, "macro.set", json!({ "index": 0, "value": 0.1 }));
    ok(&mut s, "variation.store", json!({ "name": "Calm" }));
    ok(&mut s, "macro.set", json!({ "index": 0, "value": 0.9 }));
    ok(&mut s, "macro.set", json!({ "index": 3, "value": 0.4 }));
    ok(&mut s, "variation.store", json!({}));
    let scene = s.active().unwrap();
    assert_eq!(scene.variations.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["Calm", ""]);
    assert_eq!(scene.variations[1].macros, [0.9, 0.0, 0.0, 0.4, 0.0, 0.0, 0.0, 0.0]);
    // recall: at once, nothing for the engine to morph
    ok(&mut s, "variation.recall", json!({ "index": 0 }));
    assert_eq!(values(&s), [0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    assert_eq!(s.morph, None);
    assert_eq!(s.undo_label(), Some("Recall Variation"));
    // with a morph: the document jumps, the session tells the engine to glide over 4 beats
    ok(&mut s, "variation.recall", json!({ "index": 1, "morph_beats": 4 }));
    assert_eq!(values(&s)[0], 0.9);
    assert_eq!(s.morph, Some((s.active_scene.clone().unwrap(), 4.0)));
    ok(&mut s, "edit.undo", json!({}));
    assert_eq!(values(&s)[0], 0.1, "a recall undoes like any edit");
    assert!(s.run("variation.recall", json!({ "index": 5 })).is_err());
    assert!(s.run("variation.recall", json!({})).is_err(), "index is required");
    // at most eight (A to H)
    for _ in 2..VARIATIONS {
        ok(&mut s, "variation.store", json!({}));
    }
    assert!(s.run("variation.store", json!({})).is_err());
    ok(&mut s, "variation.remove", json!({ "index": 0 }));
    assert_eq!(s.active().unwrap().variations.len(), VARIATIONS - 1);
    assert_eq!(s.active().unwrap().variations[0].macros[0], 0.9, "B became A");
    assert!(s.run("variation.remove", json!({ "index": 7 })).is_err());
    // randomize moves only the macros that drive something, the same way for a seed
    assert!(s.run("macro.randomize", json!({})).is_err(), "nothing to move yet");
    ok(&mut s, "macro.assign", json!({ "index": 5, "node": kaleido, "param": "zoom", "min": 1, "max": 3 }));
    let before = values(&s);
    ok(&mut s, "macro.randomize", json!({ "seed": 7 }));
    let after = values(&s);
    assert!((0..MACROS).filter(|&i| i != 5).all(|i| after[i] == before[i]));
    assert!((0.0..=1.0).contains(&after[5]) && (after[5] * 100.0).fract().abs() < 1e-3, "{}", after[5]);
    ok(&mut s, "edit.undo", json!({}));
    ok(&mut s, "macro.randomize", json!({ "seed": 7 }));
    assert_eq!(values(&s), after, "a seed repeats");
}

#[test]
fn bindings_replace_their_control_and_target_and_go() {
    let mut s = session();
    let cc = |n: u8| json!({ "Midi": { "port": "Launch Control XL", "channel": 0, "msg": { "Cc": n } } });
    let pickup = json!({ "Absolute": { "takeover": "Pickup" } });
    ok(&mut s, "binding.add", json!({ "control": cc(21), "target": { "Macro": 0 }, "behavior": pickup }));
    ok(&mut s, "binding.add", json!({ "control": { "Midi": { "port": null, "channel": 9, "msg": { "Note": 36 } } }, "target": { "LaunchScene": 0 }, "behavior": "Trigger" }));
    ok(&mut s, "binding.add", json!({ "control": { "Midi": { "port": null, "channel": 9, "msg": "ProgramChange" } }, "target": "LaunchSelected", "behavior": "Trigger" }));
    assert_eq!(s.project.bindings.len(), 3);
    assert_eq!(s.undo_label(), Some("Bind Control"));
    let b = s.project.binding(&Target::Macro(0)).unwrap();
    assert_eq!(b.control, Control::Midi { port: Some("Launch Control XL".into()), channel: 0, msg: MidiMsg::Cc(21) });
    assert_eq!(b.behavior, Behavior::Absolute { takeover: Takeover::Pickup });
    // the same knob on another macro moves over; the same macro on another knob too
    ok(&mut s, "binding.add", json!({ "control": cc(21), "target": { "Macro": 1 }, "behavior": pickup }));
    assert!(s.project.binding(&Target::Macro(0)).is_none() && s.project.bindings.len() == 3);
    ok(&mut s, "binding.add", json!({ "control": cc(22), "target": { "Macro": 1 }, "behavior": pickup }));
    assert_eq!(s.project.bindings.len(), 3);
    // bad input changes nothing
    let before = s.project.clone();
    assert!(s.run("binding.add", json!({ "control": { "Midi": { "port": null, "channel": 16, "msg": { "Cc": 1 } } }, "target": "Blackout", "behavior": "Toggle" })).is_err());
    assert!(s.run("binding.add", json!({ "control": cc(200), "target": "Blackout", "behavior": "Toggle" })).is_err());
    assert!(s.run("binding.add", json!({ "control": cc(1), "target": { "Macro": 8 }, "behavior": "Trigger" })).is_err());
    assert!(s.run("binding.add", json!({ "control": cc(1), "target": "Sometimes", "behavior": "Trigger" })).is_err());
    assert!(s.run("binding.remove", json!({ "index": 9 })).is_err());
    assert!(s.run("binding.remove", json!({ "target": "TapTempo" })).is_err());
    assert_eq!(s.project, before);
    // saved and loaded
    let text = ron::ser::to_string_pretty(&s.project, ron::ser::PrettyConfig::default()).unwrap();
    assert_eq!(ron::from_str::<Project>(&text).unwrap(), s.project);
    ok(&mut s, "binding.remove", json!({ "target": "LaunchSelected" }));
    ok(&mut s, "binding.remove", json!({ "index": 0 }));
    assert_eq!(s.project.bindings.len(), 1);
    ok(&mut s, "edit.undo", json!({}));
    ok(&mut s, "edit.undo", json!({}));
    assert_eq!(s.project, before);
    // a parameter's binding goes with its node
    let noise = add(&mut s, "velo.noise", 0.0);
    ok(&mut s, "binding.add", json!({ "control": cc(30), "target": { "Param": { "node": noise, "param": "scale" } }, "behavior": { "Absolute": { "takeover": "Jump" } } }));
    ok(&mut s, "node.remove", json!({ "node": noise }));
    assert!(s.project.bindings.iter().all(|b| !matches!(b.target, Target::Param { .. })));
}

#[test]
fn macros_variations_and_bindings_round_trip_and_old_files_load() {
    let mut s = session();
    let kaleido = add(&mut s, "velo.kaleido", 0.0);
    ok(&mut s, "macro.assign", json!({ "index": 1, "node": kaleido, "param": "offset.x", "min": -0.5, "max": 0.5 }));
    ok(&mut s, "macro.set", json!({ "index": 1, "value": 0.25 }));
    ok(&mut s, "macro.rename", json!({ "index": 1, "name": "Drift" }));
    ok(&mut s, "variation.store", json!({ "name": "Build" }));
    ok(&mut s, "binding.add", json!({ "control": { "Midi": { "port": null, "channel": 0, "msg": { "Cc": 21 } } }, "target": { "Macro": 1 }, "behavior": { "Absolute": { "takeover": "Pickup" } } }));
    let text = ron::ser::to_string_pretty(&s.project, ron::ser::PrettyConfig::default()).unwrap();
    assert_eq!(ron::from_str::<Project>(&text).unwrap(), s.project);
    assert_eq!(s.project.format, FORMAT, "the format stays: everything new has a default");
    // files from before macros: eight blank ones, no variations, no bindings
    let old: Project = ron::from_str(r#"(format: 1, scenes: [(id: "s_1", name: "Drop", nodes: [], edges: [])])"#).unwrap();
    assert_eq!(old.scenes[0].macros, <[Macro; MACROS]>::default());
    assert!(old.scenes[0].variations.is_empty() && old.bindings.is_empty());
    // and an untouched scene writes none of it
    let plain = ron::ser::to_string(&old).unwrap();
    assert!(!plain.contains("macros") && !plain.contains("variations") && !plain.contains("bindings"), "{plain}");
}

#[test]
fn shader_code_is_kept_edited_undone_and_saved() {
    let mut s = session();
    let code = "/*{ \"name\": \"Red\" }*/\nfn main_image(uv: vec2f, px: vec2f) -> vec4f { return vec4f(1.0, 0.0, 0.0, 1.0); }";
    ok(&mut s, "node.add", json!({ "kind": SHADER, "code": code }));
    let id = s.active().unwrap().nodes[0].id.clone();
    // typing merges into one step
    s.run_merge("node.set_code", json!({ "node": id, "code": "a" }), "code").unwrap();
    s.run_merge("node.set_code", json!({ "node": id, "code": "ab" }), "code").unwrap();
    assert_eq!(s.active().unwrap().nodes[0].code.as_deref(), Some("ab"));
    ok(&mut s, "edit.undo", json!({}));
    assert_eq!(s.active().unwrap().nodes[0].code.as_deref(), Some(code));
    // only a Shader node has code
    let noise = add(&mut s, "velo.noise", 0.0);
    assert!(s.run("node.set_code", json!({ "node": noise, "code": "x" })).is_err());
    let text = ron::ser::to_string(&s.project).unwrap();
    assert_eq!(ron::from_str::<Project>(&text).unwrap(), s.project);
    assert_eq!(text.matches("code:").count(), 1, "nodes without code write none: {text}");
}

#[test]
fn shader_nodes_get_a_file_of_their_own() {
    let mut s = session();
    ok(&mut s, "node.add", json!({ "kind": SHADER, "name": "Tunnel / Warp!" }));
    ok(&mut s, "node.add", json!({ "kind": SHADER, "name": "Tunnel / Warp!" }));
    ok(&mut s, "scene.add", json!({}));
    ok(&mut s, "node.add", json!({ "kind": SHADER, "name": "Tunnel / Warp!" }));
    ok(&mut s, "node.add", json!({ "kind": SHADER, "name": "!!" }));
    ok(&mut s, "node.add", json!({ "kind": "velo.noise" }));
    let files: Vec<Option<&str>> = s.project.scenes.iter().flat_map(|c| &c.nodes).map(|n| n.file.as_deref()).collect();
    // the second is "Tunnel / Warp! 2" in the same scene; the third, in another scene, takes the next free file
    assert_eq!(files, [Some("tunnel-warp.wgsl"), Some("tunnel-warp-2.wgsl"), Some("tunnel-warp-3.wgsl"), Some("shader.wgsl"), None]);
}

/// A project as v0.1 writes it (format 1: the demo's four scenes, mappings, macros, a variation,
/// launch settings, a binding, a Shader node's code inline). It must open, unchanged, forever:
/// a new format adds a migration and keeps this test passing. Never regenerate the file.
#[test]
fn a_format_1_project_still_opens() {
    let project: Project = ron::from_str(include_str!("../tests/fixtures/format-1.velo")).expect("format 1 opens");
    assert_eq!(project.format, 1);
    assert_eq!((project.canvas.width, project.canvas.height), (1280, 720));
    let names: Vec<&str> = project.scenes.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Tunnel", "Kaleido Bloom", "Pulse Rings", "Liquid"]);
    let tunnel = &project.scenes[0].nodes[0];
    assert_eq!((tunnel.kind.as_str(), tunnel.file.as_deref()), (SHADER, Some("tunnel.wgsl")));
    assert!(tunnel.code.as_deref().is_some_and(|c| c.contains("fn main_image")));
    let bloom = &project.scenes[1];
    assert_eq!(bloom.launch.quantize, Quantize::Bars4);
    assert_eq!(bloom.variations.len(), 1);
    assert_eq!(bloom.macros[0].name, "Zoom");
    assert_eq!(bloom.nodes.iter().map(|n| n.mappings.len()).sum::<usize>(), 2);
    assert_eq!(project.bindings.len(), 1);
    // 1 → 2: mappings of shared sources come from an Audio In or a Beat, one per scene
    let mut project = project;
    project.migrate();
    assert_eq!(project.format, FORMAT);
    for scene in &project.scenes {
        let maps: Vec<&Mapping> = scene.nodes.iter().flat_map(|n| &n.mappings).collect();
        let audio: Vec<&Node> = scene.nodes.iter().filter(|n| n.kind == AUDIO).collect();
        assert_eq!(audio.len(), usize::from(!maps.is_empty()), "{}", scene.name);
        assert!(maps.iter().all(|m| m.source.starts_with(&format!("{}/", audio[0].id))), "{}: {maps:?}", scene.name);
    }
    let bloom = &project.scenes[1];
    let kaleido = bloom.nodes.iter().find(|n| n.mappings.len() == 1 && n.mappings[0].param == "zoom");
    assert!(kaleido.is_some_and(|n| n.mappings[0].source.ends_with("/4")), "Bloom's zoom on the kick");
}

#[test]
fn an_lfo_is_a_source_for_its_scene() {
    let mut s = session();
    let lfo = add(&mut s, LFO, 0.0);
    let kaleido = add(&mut s, "velo.kaleido", 200.0);
    ok(&mut s, "mapping.add", json!({ "node": kaleido, "param": "zoom", "source": lfo }));
    assert_eq!(s.active().unwrap().node(&kaleido).unwrap().mappings[0].source, lfo);
    // not a node of this scene, not an LFO: refused
    assert!(s.run("mapping.add", json!({ "node": kaleido, "param": "spin", "source": kaleido })).is_err());
    assert!(s.run("mapping.add", json!({ "node": kaleido, "param": "spin", "source": "n_nope" })).is_err());
}

#[test]
fn an_image_node_takes_a_file_or_a_folder() {
    let mut s = session();
    let image = add(&mut s, IMAGE, 0.0);
    let blur = add(&mut s, "velo.blur", 200.0);
    let media = |s: &Session| s.active().unwrap().node(&image).unwrap().media.clone();
    ok(&mut s, "node.set_media", json!({ "node": image, "media": " /loops/tunnel.gif " }));
    assert_eq!(media(&s).as_deref(), Some("/loops/tunnel.gif"));
    ok(&mut s, "node.set_media", json!({ "node": image, "media": "" }));
    assert_eq!(media(&s), None, "empty clears it");
    ok(&mut s, "edit.undo", json!({}));
    assert_eq!(media(&s).as_deref(), Some("/loops/tunnel.gif"));
    assert!(s.run("node.set_media", json!({ "node": blur, "media": "/a.png" })).is_err(), "only Image nodes");
    // a Text node takes a font file
    let caption = add(&mut s, TEXT, 400.0);
    ok(&mut s, "node.set_media", json!({ "node": caption, "media": "/fonts/Display.otf" }));
    assert_eq!(s.active().unwrap().node(&caption).unwrap().media.as_deref(), Some("/fonts/Display.otf"));
    // saved and back
    let text = ron::to_string(&s.project).unwrap();
    assert!(text.contains("tunnel.gif"));
    assert_eq!(ron::from_str::<Project>(&text).unwrap(), s.project);
}

#[test]
fn nodes_toggle_undo_and_save_only_when_off() {
    let mut s = session();
    let noise = add(&mut s, "velo.noise", 0.0);
    let blur = add(&mut s, "velo.blur", 200.0);
    let output = add(&mut s, "velo.output", 400.0);
    let states = |s: &Session| s.active().unwrap().nodes.iter().map(|n| n.off).collect::<Vec<_>>();
    let text = ron::to_string(&s.project).unwrap();
    assert!(!text.contains("off:"));
    assert_eq!(ron::from_str::<Project>(&text).unwrap(), s.project, "old files load with nodes on");
    for props in [json!({}), json!({ "nodes": [] }), json!({ "nodes": ["n_missing"] })] {
        assert!(s.run("node.toggle", props).is_err());
    }
    ok(&mut s, "node.toggle", json!({ "nodes": [noise], "on": false }));
    assert_eq!(states(&s), [true, false, false]);
    ok(&mut s, "node.toggle", json!({ "nodes": [noise, blur, output] }));
    assert_eq!(states(&s), [false, true, false], "each flips, except outputs");
    assert_eq!(s.undo_label(), Some("Turn Node On or Off"));
    ok(&mut s, "edit.undo", json!({}));
    assert_eq!(states(&s), [true, false, false]);
    let text = ron::to_string(&s.project).unwrap();
    assert_eq!(text.matches("off:true").count(), 1);
    assert_eq!(ron::from_str::<Project>(&text).unwrap(), s.project);
    ok(&mut s, "node.toggle", json!({ "nodes": [noise, blur, output], "on": false }));
    assert_eq!(states(&s), [true, true, false]);
    ok(&mut s, "node.toggle", json!({ "nodes": [noise, blur], "on": true }));
    assert!(!ron::to_string(&s.project).unwrap().contains("off:"));
}

#[test]
fn nodes_and_scenes_take_an_emoji() {
    let mut s = session();
    let noise = add(&mut s, "velo.noise", 0.0);
    ok(&mut s, "node.set_icon", json!({ "node": noise, "icon": " ❄️ " }));
    assert_eq!(s.active().unwrap().node(&noise).unwrap().icon, "❄", "one character, the variation selector dropped");
    ok(&mut s, "scene.set_icon", json!({ "icon": "🔥" }));
    assert_eq!(s.active().unwrap().icon, "🔥");
    ok(&mut s, "edit.undo", json!({}));
    assert_eq!(s.active().unwrap().icon, "");
    ok(&mut s, "node.set_icon", json!({ "node": noise, "icon": "" }));
    assert_eq!(s.active().unwrap().node(&noise).unwrap().icon, "");
    assert!(!ron::ser::to_string(&s.project).unwrap().contains("icon"), "none is written when there's none");
}

#[test]
fn a_node_turns_into_another_kind_in_place() {
    let mut s = session();
    let noise = add(&mut s, "velo.noise", 0.0);
    let blur = add(&mut s, "velo.blur", 200.0);
    let out = add(&mut s, "velo.output", 400.0);
    ok(&mut s, "edge.connect", json!({ "from": [noise, 0], "to": [blur, 0] }));
    ok(&mut s, "edge.connect", json!({ "from": [blur, 0], "to": [out, 0] }));
    ok(&mut s, "node.replace", json!({ "node": blur, "kind": "velo.kaleido", "params": { "zoom": [2.0, 0.0, 0.0, 0.0] } }));
    let scene = s.active().unwrap();
    let k = scene.node(&blur).unwrap();
    assert_eq!((k.kind.as_str(), k.name.as_str(), k.params["zoom"][0]), ("velo.kaleido", "Kaleido", 2.0), "the name said the old kind: it says the new one");
    assert_eq!(scene.edges.len(), 2, "both wires still fit");
    // a picture node into a signal node: its picture wires go, and so does what it fed
    ok(&mut s, "node.replace", json!({ "node": blur, "kind": LFO }));
    assert!(s.active().unwrap().edges.is_empty());
    ok(&mut s, "edit.undo", json!({}));
    assert_eq!(s.active().unwrap().edges.len(), 2, "one undo step");
    assert!(s.run("node.replace", json!({ "node": blur, "kind": "velo.nope" })).is_err());
}

#[test]
fn a_grid_that_went_out_feeds_the_output_once_it_opens() {
    // format 2: a Grid was an output. One wired and going out, alone in its scene; one beside an
    // Output that went out instead; one wired after an Output, so it went out
    let old = r#"(format: 2, scenes: [
        (id: "s_1", name: "Wall", nodes: [(id: "n_a", kind: "velo.noise", name: "Noise", pos: (0.0, 0.0)), (id: "n_g", kind: "velo.grid", name: "Grid", pos: (300.0, 0.0))], edges: [(from: ("n_a", 0), to: ("n_g", 0))]),
        (id: "s_2", name: "Plain", nodes: [(id: "n_a", kind: "velo.noise", name: "Noise", pos: (0.0, 0.0)), (id: "n_o", kind: "velo.output", name: "Output", pos: (300.0, 0.0)), (id: "n_g", kind: "velo.grid", name: "Grid", pos: (300.0, 200.0))], edges: [(from: ("n_a", 0), to: ("n_o", 0))]),
        (id: "s_3", name: "Both", nodes: [(id: "n_a", kind: "velo.noise", name: "Noise", pos: (0.0, 0.0)), (id: "n_o", kind: "velo.output", name: "Output", pos: (300.0, 0.0)), (id: "n_g", kind: "velo.grid", name: "Grid", pos: (300.0, 200.0))], edges: [(from: ("n_a", 0), to: ("n_o", 0)), (from: ("n_a", 0), to: ("n_g", 1))]),
    ])"#;
    let mut project: Project = ron::from_str(old).unwrap();
    project.migrate();
    assert_eq!(project.format, FORMAT);
    let into_output = |s: &Scene| {
        let out = s.nodes.iter().find(|n| n.kind == OUTPUT).map(|n| n.id.clone()).expect("an Output");
        s.wire_into(&out, 0).map(|e| e.from.0.clone())
    };
    let [wall, plain, both] = [&project.scenes[0], &project.scenes[1], &project.scenes[2]];
    assert_eq!((wall.nodes.len(), into_output(wall).as_deref()), (3, Some("n_g")), "an Output added, fed by the grid");
    assert_eq!(wall.nodes[2].pos, [560.0, 0.0]);
    assert_eq!((plain.nodes.len(), into_output(plain).as_deref()), (3, Some("n_a")), "the Output went out: as it was");
    assert_eq!((both.nodes.len(), into_output(both).as_deref()), (3, Some("n_g")), "the grid went out: it feeds the Output");
    // once is enough: a file of this format opens as it is
    let again = project.clone();
    project.migrate();
    assert_eq!(project, again);
}

#[test]
fn a_shader_that_read_an_input_opens_as_a_shader_fx() {
    // format 3: one Shader wired in, one whose header asks for a texture, one picture
    let old = r#"(format: 3, scenes: [(id: "s_1", name: "Fx", nodes: [
        (id: "n_a", kind: "velo.noise", name: "Noise", pos: (0.0, 0.0)),
        (id: "n_w", kind: "velo.shader", name: "Wired", pos: (300.0, 0.0), code: Some("/*{ \"name\": \"W\" }*/")),
        (id: "n_t", kind: "velo.shader", name: "Texture", pos: (300.0, 200.0), code: Some("/*{ \"inputs\": [{ \"id\": \"src\", \"type\": \"texture\" }] }*/")),
        (id: "n_p", kind: "velo.shader", name: "Picture", pos: (300.0, 400.0), code: Some("/*{ \"name\": \"P\" }*/ fn main_image() {} // \"texture\"")),
    ], edges: [(from: ("n_a", 0), to: ("n_w", 0))])])"#;
    let mut project: Project = ron::from_str(old).unwrap();
    project.migrate();
    let kinds: Vec<&str> = project.scenes[0].nodes.iter().map(|n| n.kind.as_str()).collect();
    assert_eq!(kinds, ["velo.noise", SHADER_FX, SHADER_FX, SHADER], "only the header counts, not the code below it");
    assert_eq!(project.scenes[0].edges.len(), 1, "its wire stays");
}

#[test]
fn a_duplicated_scene_is_its_own_copy_right_after_it() {
    let mut s = session();
    let noise = add(&mut s, "velo.noise", 0.0);
    let kaleido = add(&mut s, "velo.kaleido", 200.0);
    let output = add(&mut s, "velo.output", 400.0);
    let lfo = add(&mut s, LFO, 0.0);
    let shader = add(&mut s, SHADER, 600.0);
    connect(&mut s, &noise, &kaleido, 0).unwrap();
    connect(&mut s, &kaleido, &output, 0).unwrap();
    ok(&mut s, "mapping.add", json!({ "node": kaleido, "param": "zoom", "source": format!("{lfo}/0") }));
    ok(&mut s, "macro.assign", json!({ "index": 0, "node": kaleido, "param": "spin", "min": 0.0, "max": 1.0 }));
    let original = s.active().unwrap().clone();
    ok(&mut s, "scene.duplicate", json!({}));
    let p = &s.project;
    assert_eq!(p.scenes.len(), 2);
    let copy = &p.scenes[1];
    assert_eq!((s.active_scene.as_deref(), copy.name.clone()), (Some(copy.id.as_str()), format!("{} 2", original.name)), "right after it, selected, named apart");
    assert_ne!(copy.id, original.id);
    assert_eq!(copy.nodes.len(), original.nodes.len());
    assert!(copy.nodes.iter().all(|n| original.node(&n.id).is_none()), "every node is new");
    // the copy's wires, mapping and macro are among its own nodes, as they were in the original
    let id_of = |name: &str| copy.nodes.iter().find(|n| n.name == name).map(|n| n.id.clone()).unwrap();
    let (k, l) = (id_of("Kaleido"), id_of("LFO"));
    assert_eq!(copy.wire_into(&k, 0).map(|e| e.from.0.clone()), Some(id_of("Noise")));
    assert_eq!(copy.node(&k).unwrap().mappings[0].source, format!("{l}/0"));
    assert_eq!(copy.macros[0].targets[0].node, k);
    // a Shader node's code gets a file of its own; the original is untouched
    let file = |scene: &Scene, id: &str| scene.node(id).and_then(|n| n.file.clone());
    assert_ne!(file(copy, &id_of(&original.node(&shader).unwrap().name)), file(&original, &shader));
    assert_eq!(p.scenes[0], original);
    ok(&mut s, "edit.undo", json!({}));
    assert_eq!(s.project.scenes.len(), 1);
}

#[test]
fn an_imported_scene_joins_with_ids_and_files_of_its_own() {
    let mut s = session();
    let noise = add(&mut s, "velo.noise", 0.0);
    let output = add(&mut s, "velo.output", 200.0);
    connect(&mut s, &noise, &output, 0).unwrap();
    let a = add(&mut s, SHADER, 400.0);
    let b = add(&mut s, SHADER, 600.0);
    ok(&mut s, "node.rename", json!({ "node": a, "name": "Glow" }));
    ok(&mut s, "node.rename", json!({ "node": b, "name": "Glow" }));
    // the same scene, as another project would hand it over: every id clashes
    let other = s.active().unwrap().clone();
    ok(&mut s, "scene.import", json!({ "scene": other }));
    let p = &s.project;
    assert_eq!(p.scenes.len(), 2);
    let copy = &p.scenes[1];
    assert_eq!(s.active_scene.as_deref(), Some(copy.id.as_str()));
    assert_ne!(copy.name, other.name, "a free name");
    assert!(copy.nodes.iter().all(|n| other.node(&n.id).is_none()), "every node is new");
    assert_eq!(copy.edges.len(), 1);
    assert!(copy.node(&copy.edges[0].from.0).is_some(), "its wire between its own nodes");
    let files: Vec<_> = p.scenes.iter().flat_map(|c| &c.nodes).filter_map(|n| n.file.clone()).collect();
    assert_eq!(files.len(), 4);
    assert!(files.iter().all(|f| files.iter().filter(|g| *g == f).count() == 1), "two nodes named alike never share a file: {files:?}");
    ok(&mut s, "edit.undo", json!({}));
    assert_eq!(s.project.scenes.len(), 1);
}

#[test]
fn a_scene_becomes_a_layer_and_leaves_the_autopilot() {
    let mut s = session();
    let id = s.active().unwrap().id.clone();
    assert_eq!((s.active().unwrap().layer, s.active().unwrap().blend, s.active().unwrap().manual), (Layer::Scene, LayerBlend::Screen, false), "a scene, at first");
    ok(&mut s, "scene.set_layer", json!({ "layer": "Top" }));
    ok(&mut s, "scene.set_layer", json!({ "scene": id, "blend": "Multiply" }));
    ok(&mut s, "scene.set_autopilot", json!({ "on": false }));
    let scene = s.active().unwrap();
    assert_eq!((scene.layer, scene.blend, scene.manual), (Layer::Top, LayerBlend::Multiply, true));
    assert!(s.run("scene.set_layer", json!({ "layer": "Sideways" })).is_err());
    // saved only when set: an older file loads as a plain scene
    let text = ron::to_string(&s.project).unwrap();
    assert!(text.contains("layer:Top") && text.contains("manual:true"), "{text}");
    let plain: Scene = ron::from_str(r#"(id: "s_a", name: "A")"#).unwrap();
    assert_eq!((plain.layer, plain.manual), (Layer::Scene, false));
    ok(&mut s, "edit.undo", json!({}));
    assert!(!s.active().unwrap().manual);
}

#[test]
fn a_canvas_past_4k_is_brought_down_its_shape_kept() {
    assert_eq!(crate::fit_canvas(3840, 2160), (3840, 2160));
    let (w, h) = crate::fit_canvas(8192, 8192);
    assert!(u64::from(w) * u64::from(h) <= crate::CANVAS_PIXELS && w == h);
    let (w, h) = crate::fit_canvas(8192, 1024);
    assert!(u64::from(w) * u64::from(h) <= crate::CANVAS_PIXELS && (w as f32 / h as f32 - 8.0).abs() < 0.05);
}
