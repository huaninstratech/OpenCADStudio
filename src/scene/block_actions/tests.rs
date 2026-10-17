use super::*;
use crate::scene::{block_distance_parameter as distance, block_point_parameter as point, Scene};
use codec::{DwgReader, DwgWriter, DxfReader, DxfWriter};
use std::io::Cursor;

fn fixture() -> (CadDocument, Handle, Vec<Handle>) {
    let mut scene = Scene::new();
    scene.define_block_raw(
        "ACTIONS",
        Vector3::ZERO,
        vec![
            EntityType::Line(codec::entities::Line::from_points(
                Vector3::UNIT_X,
                Vector3::new(2., 0., 0.),
            )),
            EntityType::Circle(codec::entities::Circle::from_center_radius(
                Vector3::new(4., 3., 0.),
                2.,
            )),
        ],
    );
    let block = scene.document.block_records.get("ACTIONS").unwrap();
    let (definition, handles) = (block.handle, block.entity_handles.clone());
    (scene.document, definition, handles)
}
fn linear(doc: &mut CadDocument, def: Handle) -> Handle {
    distance::create(
        doc,
        def,
        distance::Kind::Linear,
        Vector3::ZERO,
        Vector3::new(10., 0., 0.),
        Vector3::new(5., 2., 0.),
    )
    .unwrap()
}
fn start(doc: &CadDocument, h: Handle) -> Vector3 {
    let EntityType::Line(l) = doc.get_entity(h).unwrap() else {
        panic!()
    };
    l.start
}
#[test]
fn block_actions_native_bindings_roundtrip_and_append_preserves_neighbors() {
    let (mut doc, def, handles) = fixture();
    let p = linear(&mut doc, def);
    for kind in [Kind::Move, Kind::Scale] {
        create(
            &mut doc,
            def,
            kind,
            p,
            &[handles[0]],
            DVec3::new(3., 4., 0.),
        )
        .unwrap();
    }
    let graph = authoring::graph(&doc, def).unwrap().unwrap();
    let ObjectType::DynamicBlock(o) = &doc.objects[&graph] else {
        panic!()
    };
    let DynamicBlockData::EvaluationGraph(g) = &o.data else {
        panic!()
    };
    assert!(g
        .nodes
        .iter()
        .all(|n| authoring::graph::valid_lists(g, n.id)));
    let dwg = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
        .read()
        .unwrap();
    let dxf = DxfReader::from_reader(Cursor::new(DxfWriter::new(&doc).write_to_vec().unwrap()))
        .unwrap()
        .read()
        .unwrap();
    for saved in [dwg, dxf] {
        assert_eq!(actions(&saved, def).len(), 2);
        for (h, _, _, _) in actions(&doc, def) {
            assert_eq!(saved.objects[&h], doc.objects[&h]);
        }
        let mut test = saved.clone();
        assert!(evaluate(&mut test, &saved, def, p, DVec3::new(20., 5., 0.)).unwrap());
        assert_eq!(start(&test, handles[0]), Vector3::new(22., 0., 0.));
        assert_eq!(test.get_entity(handles[1]), saved.get_entity(handles[1]));
    }
}
#[test]
fn block_actions_point_move_is_absolute_reversible_and_leaves_unselected_objects() {
    let (mut doc, def, handles) = fixture();
    let p = point::create(
        &mut doc,
        def,
        Vector3::new(2., 3., 0.),
        Vector3::new(5., 6., 0.),
    )
    .unwrap();
    create(&mut doc, def, Kind::Move, p, &[handles[0]], DVec3::ZERO).unwrap();
    let baseline = doc.clone();
    for _ in 0..8 {
        assert!(evaluate(&mut doc, &baseline, def, p, DVec3::new(12., -7., 0.)).unwrap());
        assert_eq!(start(&doc, handles[0]), Vector3::new(11., -10., 0.));
        assert!(!evaluate(&mut doc, &baseline, def, p, DVec3::new(12., -7., 0.)).unwrap());
        assert!(evaluate(&mut doc, &baseline, def, p, DVec3::new(2., 3., 0.)).unwrap());
        assert_eq!(start(&doc, handles[0]), start(&baseline, handles[0]));
    }
    assert_eq!(doc.get_entity(handles[1]), baseline.get_entity(handles[1]));
}
#[test]
fn block_actions_scale_uses_distance_ratio_and_parameter_base() {
    let (mut doc, def, handles) = fixture();
    let p = distance::create(
        &mut doc,
        def,
        distance::Kind::Polar,
        Vector3::new(4., 3., 0.),
        Vector3::new(14., 3., 0.),
        Vector3::new(9., 5., 0.),
    )
    .unwrap();
    create(&mut doc, def, Kind::Scale, p, &[handles[1]], DVec3::ZERO).unwrap();
    let baseline = doc.clone();
    evaluate(&mut doc, &baseline, def, p, DVec3::new(4., 23., 0.)).unwrap();
    let EntityType::Circle(c) = doc.get_entity(handles[1]).unwrap() else {
        panic!()
    };
    assert_eq!(c.center, Vector3::new(4., 3., 0.));
    assert_eq!(c.radius, 4.);
    let before = doc.clone();
    assert!(evaluate(&mut doc, &baseline, def, p, DVec3::new(4., 3., 0.)).is_err());
    assert_eq!(doc, before);
}
#[test]
fn block_actions_reject_invalid_selection_binding_and_nonfinite_without_mutation() {
    let (mut doc, def, handles) = fixture();
    let p = linear(&mut doc, def);
    let before = doc.clone();
    for (parameter, targets, location) in [
        (Handle::NULL, vec![handles[0]], DVec3::ZERO),
        (p, vec![], DVec3::ZERO),
        (p, vec![Handle::new(0xff_ff_ff)], DVec3::ZERO),
        (p, vec![handles[0]], DVec3::splat(f64::NAN)),
    ] {
        assert!(create(&mut doc, def, Kind::Move, parameter, &targets, location).is_err());
        assert_eq!(doc, before);
    }
    let action = create(&mut doc, def, Kind::Move, p, &[handles[0]], DVec3::ZERO).unwrap();
    let baseline = doc.clone();
    assert!(evaluate(&mut doc, &baseline, def, p, DVec3::splat(f64::NAN)).is_err());
    assert_eq!(doc, baseline);
    let mut restricted = doc.clone();
    let ObjectType::DynamicBlock(object) = restricted.objects.get_mut(&p).unwrap() else {
        panic!()
    };
    let DynamicBlockData::LinearParameter(parameter) = &mut object.data else {
        panic!()
    };
    parameter.value_set.maximum = 15.;
    parameter.value_set.flags |= 2;
    let restricted_baseline = restricted.clone();
    assert!(evaluate(
        &mut restricted,
        &restricted_baseline,
        def,
        p,
        DVec3::new(20., 0., 0.)
    )
    .is_err());
    assert_eq!(
        restricted, restricted_baseline,
        "unsupported value constraints are not silently ignored"
    );
    let ObjectType::DynamicBlock(o) = doc.objects.get_mut(&action).unwrap() else {
        panic!()
    };
    let DynamicBlockData::MoveAction(m) = &mut o.data else {
        panic!()
    };
    m.offsets.distance_multiplier = 0.5;
    let baseline = doc.clone();
    assert!(evaluate(&mut doc, &baseline, def, p, DVec3::new(15., 0., 0.)).is_err());
    assert_eq!(doc, baseline);
}
#[test]
fn block_actions_reframe_moves_markers_and_retains_targets_and_connections() {
    let (mut doc, def, handles) = fixture();
    let p = linear(&mut doc, def);
    let h = create(
        &mut doc,
        def,
        Kind::Scale,
        p,
        &[handles[0]],
        DVec3::new(3., 4., 0.),
    )
    .unwrap();
    let before = doc.objects[&h].clone();
    let t = Transform::from_translation(Vector3::new(-2., -3., 0.));
    assert_eq!(reframe(&mut doc, def, &t), 1);
    let ObjectType::DynamicBlock(o) = &doc.objects[&h] else {
        panic!()
    };
    let DynamicBlockData::ScaleAction(s) = &o.data else {
        panic!()
    };
    assert_eq!(s.action.action.display_location, Vector3::new(1., 1., 0.));
    let ObjectType::DynamicBlock(o) = before else {
        panic!()
    };
    let DynamicBlockData::ScaleAction(b) = o.data else {
        panic!()
    };
    assert_eq!(s.action.action.dependencies, b.action.action.dependencies);
    assert_eq!(s.connections, b.connections);
}
