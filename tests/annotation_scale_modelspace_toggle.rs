//! The "annotation scale in model space" toggle: with it off (the app
//! default), annotative model-space content renders at its stored size, as
//! drawn; with it on, the classic annotation scaling applies.
//!
//! The scaled specimen is an annotative MTEXT — its glyphs tessellate with
//! the annotation factor. Paper space is out of scope here — the policy never
//! touches non-model blocks.

use acadrust::entities::{EntityType, Line, MText};
use acadrust::objects::Scale;
use acadrust::types::Vector3;
use OpenCADStudio::app::{QSelectMode, QSelectOp, QSelectScope};
use OpenCADStudio::scene::{annotative, Scene};

/// A model-space scene at CANNOSCALE 1:100 holding:
/// - an annotative MTEXT ("TEST", height 2, per-object context at 1:100), and
/// - a plain 1-unit LINE (never annotative, as the QSELECT=No control).
fn anno_scene() -> Scene {
    let mut scene = Scene::new();
    let doc = &mut scene.document;

    let line = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0));
    doc.add_entity(EntityType::Line(line)).unwrap();

    let mut m = MText::new();
    m.is_annotative = true;
    m.height = 2.0;
    m.value = "TEST".to_string();
    m.insertion_point = Vector3::new(0.0, 0.0, 0.0);
    let ent = doc.add_entity(EntityType::MText(m)).unwrap();
    let scale = annotative::ensure_scale_object(doc, &Scale::new("1:100", 1.0, 100.0));
    assert!(annotative::create_annotation_context(doc, ent, scale));

    scene
        .set_annotation_scale_named("1:100")
        .expect("1:100 must resolve after ensure_scale_object");
    assert!((scene.annotation_scale - 100.0).abs() < 1.0e-3);
    scene
}

/// XY extent of every wire point and SDF text vertex (low residuals folded
/// in), or 0 when nothing is present.
fn wire_extent(wires: &[OpenCADStudio::scene::WireModel]) -> f64 {
    let mut max = 0.0f64;
    for wire in wires {
        for (index, point) in wire.points.iter().enumerate() {
            let low = wire.points_low.get(index).copied().unwrap_or([0.0; 3]);
            max = max.max((point[0] as f64 + low[0] as f64).abs());
            max = max.max((point[1] as f64 + low[1] as f64).abs());
        }
        for vertex in &wire.text_verts {
            max = max.max((vertex.pos[0] as f64 + vertex.pos_low[0] as f64).abs());
            max = max.max((vertex.pos[1] as f64 + vertex.pos_low[1] as f64).abs());
        }
    }
    max
}

#[test]
fn model_space_toggle_controls_annotation_scaling() {
    let mut scene = anno_scene();
    assert!(scene.annotation_scale_modelspace, "Scene::new keeps scaling on");

    // On: the annotative MTEXT draws at the 1:100 annotation size (×100).
    let on = wire_extent(&scene.entity_wires());
    assert!(on > 0.0, "annotative MTEXT glyphs must produce geometry");

    // Off: the same MTEXT draws at its stored size, as drawn.
    scene.set_annotation_scale_modelspace(false);
    let off = wire_extent(&scene.entity_wires());
    assert!(
        off > 0.0,
        "with the display off the MTEXT must still draw"
    );
    assert!(
        ((on / off) - 100.0).abs() < 2.0,
        "expected a 100× shrink, got on={on} off={off}"
    );

    // Back on: the scaled display returns.
    scene.set_annotation_scale_modelspace(true);
    let restored = wire_extent(&scene.entity_wires());
    assert!(
        ((restored - on) / on).abs() < 1.0e-3,
        "toggling back must restore the scaled display"
    );
}

#[test]
fn qselect_matches_annotative_property() {
    let mut scene = anno_scene();
    let yes = scene.qselect(
        QSelectScope::CurrentSpace,
        None,
        Some("annotative"),
        QSelectOp::Eq,
        "Yes",
        QSelectMode::Include,
        false,
    );
    assert_eq!(yes, 1, "the annotative MTEXT must match Annotative=Yes");

    let no = scene.qselect(
        QSelectScope::CurrentSpace,
        None,
        Some("annotative"),
        QSelectOp::Eq,
        "No",
        QSelectMode::Include,
        false,
    );
    assert_eq!(no, 1, "the plain line must match Annotative=No");
}
