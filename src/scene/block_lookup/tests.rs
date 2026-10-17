use super::*;
use crate::scene::{block_basepoint, block_distance_parameter as distance, render_graph, Scene};
use codec::{
    entities::{Insert, Line},
    EntityType,
};

fn fixture() -> (CadDocument, Handle) {
    let mut scene = Scene::new();
    scene.define_block_raw(
        "LOOKUP",
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::UNIT_X,
        ))],
    );
    let definition = scene.document.block_records.get("LOOKUP").unwrap().handle;
    (scene.document, definition)
}
fn add_distance(document: &mut CadDocument, definition: Handle) {
    distance::create(
        document,
        definition,
        distance::Kind::Linear,
        Vector3::ZERO,
        Vector3::new(2.0, 0.0, 0.0),
        Vector3::UNIT_Y,
    )
    .unwrap();
}
fn draft(document: &CadDocument, definition: Handle, parameter: Handle) -> BlockLookupAction {
    let p = parameters(document, definition)
        .into_iter()
        .find(|(h, _)| *h == parameter)
        .unwrap()
        .1;
    let mut table = empty_table(p);
    table
        .columns
        .insert(0, properties(document, definition)[0].column.clone());
    table.column_count = 2;
    table.row_count = 2;
    table.expressions = vec!["2".into(), "Small".into(), "4".into(), "Large".into()];
    table
}
fn graph_data(document: &CadDocument, definition: Handle) -> codec::objects::BlockEvaluationGraph {
    let ObjectType::DynamicBlock(o) =
        &document.objects[&graph_handle(document, definition).unwrap()]
    else {
        panic!()
    };
    let DynamicBlockData::EvaluationGraph(g) = &o.data else {
        panic!()
    };
    g.clone()
}
fn assert_links(document: &CadDocument, definition: Handle) {
    let g = graph_data(document, definition);
    for n in &g.nodes {
        assert!(graph::valid_lists(&g, n.id), "invalid node {}", n.id);
    }
    for edge in g.edges.iter().filter(|e| e.next_id == 4) {
        let inverse = g
            .edges
            .iter()
            .find(|e| e.id == edge.outgoing_edges[4])
            .unwrap();
        assert_eq!(
            (
                inverse.source_node,
                inverse.destination_node,
                inverse.outgoing_edges[4]
            ),
            (edge.destination_node, edge.source_node, edge.id)
        );
    }
}
fn roundtrip(document: &CadDocument, definition: Handle) {
    let graph = graph_handle(document, definition).unwrap();
    for extension in ["dwg", "dxf"] {
        let bytes = crate::io::save_to_bytes(document, extension, document.version).unwrap();
        let read = crate::io::load_bytes(&format!("lookup.{extension}"), bytes).unwrap();
        for (h, o) in &document.objects {
            if let ObjectType::DynamicBlock(o) = o {
                if o.owner == graph || o.handle == graph {
                    let ObjectType::DynamicBlock(actual) = &read.objects[h] else {
                        panic!()
                    };
                    assert_eq!(actual.data, o.data, "{extension}: {h}");
                    assert_eq!(actual.owner, o.owner);
                }
            }
        }
        assert_links(&read, definition);
        assert_eq!(
            block_basepoint::parameter(&read, definition),
            block_basepoint::parameter(document, definition)
        );
    }
}
#[test]
fn lookup_basepoint_native_roundtrip_rebinding_and_preservation() {
    let (mut document, definition) = fixture();
    add_distance(&mut document, definition);
    let original = document.clone();
    let base =
        block_basepoint::create(&mut document, definition, Vector3::new(3.0, 4.0, 0.0)).unwrap();
    let parameter = create(&mut document, definition, Vector3::UNIT_Y).unwrap();
    let edited = draft(&document, definition, parameter);
    let action = apply_table(&mut document, definition, parameter, edited).unwrap();
    assert_eq!(document.entities().count(), original.entities().count());
    assert_eq!(
        document.block_records.get("LOOKUP"),
        original.block_records.get("LOOKUP")
    );
    let gh = graph_handle(&document, definition).unwrap();
    for (h, o) in &original.objects {
        if *h != gh {
            assert_eq!(document.objects.get(h), Some(o), "preserved {h}");
        }
    }
    let first = graph_data(&document, definition);
    assert_eq!(first.nodes.len(), 10); // distance four, base one, lookup four, action one
    assert_eq!(first.edges.iter().filter(|e| e.next_id == 4).count(), 4);
    assert_links(&document, definition);
    let mut edited = table(&document, definition, parameter).unwrap().unwrap().1;
    edited.expressions[1] = "Compact".into();
    assert_eq!(
        apply_table(&mut document, definition, parameter, edited.clone()).unwrap(),
        action
    );
    assert_eq!(
        graph_data(&document, definition),
        first,
        "row edits preserve graph IDs and links"
    );
    add_distance(&mut document, definition);
    edited
        .columns
        .insert(1, properties(&document, definition)[1].column.clone());
    edited.column_count = 3;
    edited.expressions = vec![
        "2".into(),
        "2".into(),
        "Compact".into(),
        "4".into(),
        "2".into(),
        "Large".into(),
    ];
    apply_table(&mut document, definition, parameter, edited.clone()).unwrap();
    assert_links(&document, definition);
    assert_eq!(
        graph_data(&document, definition)
            .edges
            .iter()
            .filter(|e| e.next_id == 4)
            .count(),
        6
    );
    edited.columns.remove(0);
    edited.column_count = 2;
    edited.expressions = vec!["2".into(), "Compact".into(), "4".into(), "Large".into()];
    apply_table(&mut document, definition, parameter, edited).unwrap();
    assert_links(&document, definition);
    assert_eq!(
        graph_data(&document, definition)
            .edges
            .iter()
            .filter(|e| e.next_id == 4)
            .count(),
        4
    );
    assert_eq!(
        block_basepoint::create(&mut document, definition, Vector3::UNIT_X).unwrap(),
        base
    );
    roundtrip(&document, definition);
}
#[test]
fn lookup_audit_and_failed_binding_are_atomic() {
    let (mut document, definition) = fixture();
    add_distance(&mut document, definition);
    let parameter = create(&mut document, definition, Vector3::ZERO).unwrap();
    let good = draft(&document, definition, parameter);
    for (cell, value) in [(0, "NaN"), (0, "inf"), (0, ""), (2, "2.0"), (3, "small")] {
        let mut bad = good.clone();
        bad.expressions[cell] = value.into();
        let before = document.clone();
        assert!(apply_table(&mut document, definition, parameter, bad).is_err());
        assert_eq!(document, before);
    }
    let mut bad = good.clone();
    bad.columns[0].node_id = i32::MAX;
    let before = document.clone();
    assert!(apply_table(&mut document, definition, parameter, bad).is_err());
    assert_eq!(document, before);
    let mut forward_only = good;
    forward_only.columns[1].writable = false;
    forward_only.expressions[3] = "Small".into();
    assert!(validate(&forward_only, None).is_ok());
}

#[test]
fn lookup_malformed_dependencies_are_rejected_without_mutation() {
    let (mut document, definition) = fixture();
    add_distance(&mut document, definition);
    let parameter = create(&mut document, definition, Vector3::ZERO).unwrap();
    let edited = draft(&document, definition, parameter);
    apply_table(&mut document, definition, parameter, edited).unwrap();
    let mut edited = table(&document, definition, parameter).unwrap().unwrap().1;
    edited.expressions[1] = "Changed".into();
    let handle = graph_handle(&document, definition).unwrap();
    let ObjectType::DynamicBlock(object) = document.objects.get_mut(&handle).unwrap() else {
        panic!()
    };
    let DynamicBlockData::EvaluationGraph(g) = &mut object.data else {
        panic!()
    };
    g.edges
        .iter_mut()
        .find(|e| e.next_id == 4)
        .unwrap()
        .outgoing_edges[4] = -1;
    let before = document.clone();
    assert!(apply_table(&mut document, definition, parameter, edited).is_err());
    assert_eq!(document, before);
}

#[test]
fn lookup_existing_expressions_survive_row_reordering_and_output_edits() {
    let (mut document, definition) = fixture();
    add_distance(&mut document, definition);
    let parameter = create(&mut document, definition, Vector3::ZERO).unwrap();
    let mut original = draft(&document, definition, parameter);
    original.expressions[0] = "[1,3]".into();
    original.expressions[2] = "[4,6]".into();
    let mut edited = original.clone();
    edited.expressions.rotate_left(2);
    edited.expressions[1] = "Renamed".into();
    assert!(validate(&edited, Some(&original)).is_ok());
    edited.expressions[0] = "[7,9]".into();
    assert!(validate(&edited, Some(&original)).is_err());
}
#[test]
fn lookup_basepoint_anchor_transform_and_reframe() {
    let (mut document, definition) = fixture();
    let original = document.clone();
    let point = Vector3::new(3.0, 4.0, 2.0);
    let base = block_basepoint::create(&mut document, definition, point).unwrap();
    let objects = document.objects.len();
    assert_eq!(
        block_basepoint::create(&mut document, definition, point).unwrap(),
        base
    );
    assert_eq!(document.objects.len(), objects);
    let mut insert = Insert::new("LOOKUP", Vector3::new(11.0, 13.0, 17.0));
    insert.rotation = 1.2;
    insert.set_x_scale(2.0);
    insert.set_y_scale(3.0);
    insert.set_z_scale(4.0);
    assert!(
        (render_graph::insert_transform(&document, &insert).apply(point) - insert.insert_point)
            .length()
            < 1e-9
    );
    assert_eq!(document.entities().count(), original.entities().count());
    for original in original.entities() {
        let EntityType::Line(before) = original else {
            panic!()
        };
        let EntityType::Line(after) = document.get_entity(before.common.handle).unwrap() else {
            panic!()
        };
        assert_eq!(
            (after.start, after.end, after.normal, after.thickness),
            (before.start, before.end, before.normal, before.thickness),
            "native definition tags may be added, geometry stays intact"
        );
    }
    add_distance(&mut document, definition);
    let parameter = create(&mut document, definition, Vector3::UNIT_Y).unwrap();
    let edited = draft(&document, definition, parameter);
    apply_table(&mut document, definition, parameter, edited).unwrap();
    let old = table(&document, definition, parameter).unwrap().unwrap().1;
    let transform = Transform::from_translation(Vector3::new(10.0, 20.0, 0.0));
    assert_eq!(
        block_basepoint::reframe(&mut document, definition, &transform),
        1
    );
    assert_eq!(reframe(&mut document, definition, &transform), 3);
    let new = table(&document, definition, parameter).unwrap().unwrap().1;
    assert_eq!(old.columns, new.columns);
    assert_eq!(old.expressions, new.expressions);
    assert_eq!(
        new.action.display_location,
        transform.apply(old.action.display_location)
    );
    assert_eq!(
        block_basepoint::parameter(&document, definition)
            .unwrap()
            .1
            .point,
        transform.apply(point)
    );
    let before = document.clone();
    assert!(
        block_basepoint::create(&mut document, definition, Vector3::new(f64::NAN, 0.0, 0.0))
            .is_err()
    );
    assert!(create(
        &mut document,
        definition,
        Vector3::new(0.0, f64::INFINITY, 0.0)
    )
    .is_err());
    assert_eq!(document, before);
}

#[test]
fn lookup_basepoint_existing_attributes_follow_anchor_and_editor_frame() {
    use codec::entities::{AttributeDefinition, AttributeEntity};
    let (document, definition) = fixture();
    let mut scene = Scene::new();
    scene.document = document;
    let local = Vector3::new(7.0, 9.0, 0.0);
    let mut attdef = AttributeDefinition::new("TAG".into(), "Prompt".into(), "Value".into());
    attdef.insertion_point = local;
    attdef.common.owner_handle = definition;
    scene
        .document
        .add_entity(EntityType::AttributeDefinition(attdef))
        .unwrap();
    let mut insert = Insert::new("LOOKUP", Vector3::new(10.0, 20.0, 0.0)).with_scale(2.0, 3.0, 1.0);
    insert.rotation = 0.5;
    let mut attribute = AttributeEntity::simple("TAG", "Value");
    attribute.insertion_point =
        render_graph::insert_transform(&scene.document, &insert).apply(local);
    insert.attributes.push(attribute);
    let handle = scene
        .document
        .add_entity(EntityType::Insert(insert))
        .unwrap();
    block_basepoint::create(&mut scene.document, definition, Vector3::new(3.0, 4.0, 0.0)).unwrap();
    let Some(EntityType::Insert(i)) = scene.document.get_entity(handle) else {
        panic!()
    };
    assert!(
        (i.attributes[0].insertion_point
            - render_graph::insert_transform(&scene.document, i).apply(local))
        .length()
            < 1e-9
    );
    let transform = Transform::from_translation(Vector3::new(-1.0, -2.0, 0.0));
    scene.reframe_block_definition(definition, &transform);
    let Some(EntityType::Insert(i)) = scene.document.get_entity(handle) else {
        panic!()
    };
    assert!(
        (i.attributes[0].insertion_point
            - render_graph::insert_transform(&scene.document, i).apply(transform.apply(local)))
        .length()
            < 1e-9
    );
}
