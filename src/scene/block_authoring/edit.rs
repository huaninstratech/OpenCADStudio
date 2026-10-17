//! Editor picking and native definition-point edits. No proxy drawing entities.
use super::*;
use crate::scene::block_distance_parameter::{self as distance, Graphic};
use codec::objects::{BlockGrip, BlockTwoPointParameter};
use codec::types::Vector3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Point,
    Base,
    End,
    Label(usize),
    Reference,
}
pub struct Item {
    pub handle: Handle,
    pub name: String,
    pub graphic: Graphic,
    pub grips: Vec<(Part, Vector3)>,
}
pub fn items(doc: &CadDocument, definition: Handle) -> Vec<Item> {
    let mut result = vec![];
    for (handle, p) in crate::scene::block_point_parameter::parameters(doc, definition) {
        let mut graphic = Graphic::default();
        graphic
            .lines
            .push([p.parameter.definition_point, p.definition_label_point]);
        graphic
            .labels
            .push((p.definition_label_point, p.position_name.clone()));
        result.push(Item {
            handle,
            name: p.position_name.clone(),
            graphic,
            grips: vec![
                (Part::Point, p.parameter.definition_point),
                (Part::Label(0), p.definition_label_point),
            ],
        });
    }
    for p in distance::parameters(doc, definition) {
        let graphic = distance::graphic(&p);
        let mut grips = vec![
            (Part::Base, p.native.definition_base_point),
            (Part::End, p.native.definition_end_point),
        ];
        // Polar's second label is derived from its angle arc, not an editable offset.
        grips.extend(
            graphic
                .labels
                .iter()
                .take(if p.kind == distance::Kind::XY { 2 } else { 1 })
                .enumerate()
                .map(|(i, (p, _))| (Part::Label(i), *p)),
        );
        result.push(Item {
            handle: p.handle,
            name: p.labels.join(" / "),
            graphic,
            grips,
        });
    }
    for p in crate::scene::block_orientation_parameter::parameters(doc, definition) {
        use crate::scene::block_orientation_parameter::Kind;
        let graphic = crate::scene::block_orientation_parameter::graphic(&p);
        let mut grips = vec![
            (Part::Base, p.native.definition_base_point),
            (Part::End, p.native.definition_end_point),
        ];
        if p.kind == Kind::Rotation {
            grips.push((Part::Reference, p.reference));
        }
        if p.kind != Kind::Alignment {
            grips.extend(
                graphic
                    .labels
                    .iter()
                    .take(1)
                    .map(|(p, _)| (Part::Label(0), *p)),
            );
        }
        result.push(Item {
            handle: p.handle,
            name: p.label.clone(),
            graphic,
            grips,
        });
    }
    for (handle, p) in crate::scene::block_lookup::parameters(doc, definition) {
        result.push(single(
            handle,
            p.lookup_name.clone(),
            p.parameter.definition_point,
        ));
    }
    if let Some((handle, p)) = crate::scene::block_basepoint::parameter(doc, definition) {
        result.push(single(
            handle,
            "Basepoint".into(),
            p.parameter.definition_point,
        ));
    }
    if let Some(p) = crate::scene::block_visibility::parameter(doc, definition) {
        result.push(single(p.handle, p.name.clone(), p.def_point));
    }
    for (handle, p, _, _) in crate::scene::block_actions::actions(doc, definition) {
        result.push(single(handle, p.element.name.clone(), p.display_location));
    }
    result
}
fn single(handle: Handle, name: String, point: Vector3) -> Item {
    let mut graphic = Graphic::default();
    graphic.labels.push((point, name.clone()));
    Item {
        handle,
        name,
        graphic,
        grips: vec![(Part::Point, point)],
    }
}
fn two(data: &mut DynamicBlockData) -> Option<&mut BlockTwoPointParameter> {
    Some(match data {
        DynamicBlockData::LinearParameter(p) => &mut p.parameter,
        DynamicBlockData::PolarParameter(p) => &mut p.parameter,
        DynamicBlockData::XYParameter(p) => &mut p.parameter,
        DynamicBlockData::RotationParameter(p) => &mut p.parameter,
        DynamicBlockData::AlignmentParameter(p) => &mut p.parameter,
        DynamicBlockData::FlipParameter(p) => &mut p.parameter,
        _ => return None,
    })
}
fn grip(data: &mut DynamicBlockData) -> Option<&mut BlockGrip> {
    Some(match data {
        DynamicBlockData::LinearGrip(p) | DynamicBlockData::AlignmentGrip(p) => &mut p.grip,
        DynamicBlockData::XYGrip(p)
        | DynamicBlockData::PolarGrip(p)
        | DynamicBlockData::RotationGrip(p)
        | DynamicBlockData::LookupGrip(p)
        | DynamicBlockData::VisibilityGrip(p) => p,
        DynamicBlockData::FlipGrip(p) => &mut p.grip,
        _ => return None,
    })
}
pub fn move_part(
    doc: &mut CadDocument,
    definition: Handle,
    handle: Handle,
    part: Part,
    at: Vector3,
) -> Result<bool, &'static str> {
    if ![at.x, at.y, at.z].iter().all(|v| v.is_finite()) {
        return Err("Specify a finite authoring location.");
    }
    let item = items(doc, definition)
        .into_iter()
        .find(|i| i.handle == handle)
        .ok_or("The authoring object is no longer available.")?;
    let old = item
        .grips
        .iter()
        .find(|(p, _)| *p == part)
        .map(|(_, p)| *p)
        .ok_or("This authoring grip is unavailable.")?;
    if (at.z - old.z).abs() > 1e-8 {
        return Err("Keep the authoring grip in its block XY plane.");
    }
    if (at - old).length() < 1e-10 {
        return Ok(false);
    }
    let graph = graph(doc, definition)?.ok_or("The authoring graph is unavailable.")?;
    let mut staged = doc.clone();
    if crate::scene::block_basepoint::parameter(doc, definition).is_some_and(|(h, _)| h == handle) {
        crate::scene::block_basepoint::create(&mut staged, definition, at)?;
        *doc = staged;
        return Ok(true);
    }
    let mut moved_grips = vec![];
    match staged
        .objects
        .get_mut(&handle)
        .ok_or("The authoring object is unavailable.")?
    {
        ObjectType::BlockVisibilityParameter(p) => {
            p.def_point = at;
            moved_grips.extend(
                p.property_info
                    .iter()
                    .flat_map(|p| p.connections.iter().map(|c| (c.code, at))),
            );
        }
        ObjectType::DynamicBlock(object) => {
            if let Some(p) = two(&mut object.data) {
                match part {
                    Part::Base => {
                        p.definition_base_point = at;
                        p.base_point = p.base_point.map(|_| at);
                        p.updated_base_point = p.updated_base_point.map(|_| at);
                        moved_grips.push((p.property_states[0], at));
                    }
                    Part::End => {
                        p.definition_end_point = at;
                        p.end_point = p.end_point.map(|_| at);
                        p.updated_end_point = p.updated_end_point.map(|_| at);
                        moved_grips.push((p.property_states[1], at));
                    }
                    _ => {}
                }
                let length = (p.definition_end_point - p.definition_base_point).length();
                if !length.is_finite() || length < 1e-10 {
                    return Err("Parameter endpoints must remain distinct and finite.");
                }
            }
            match &mut object.data {
                DynamicBlockData::PointParameter(p) => match part {
                    Part::Point => {
                        p.parameter.definition_point = at;
                        moved_grips.push((p.parameter.property_count, at))
                    }
                    Part::Label(_) => p.definition_label_point = at,
                    _ => {}
                },
                DynamicBlockData::LookupParameter(p) => {
                    p.parameter.definition_point = at;
                    moved_grips.push((p.parameter.property_count, at))
                }
                DynamicBlockData::BasePointParameter(p) => {
                    p.parameter.definition_point = at;
                    p.point = at;
                    p.base_point = at;
                }
                DynamicBlockData::LinearParameter(p) => {
                    if let Part::Label(_) = part {
                        p.distance = label_offset(&p.parameter, at)
                    }
                }
                DynamicBlockData::PolarParameter(p) => {
                    if let Part::Label(_) = part {
                        p.offset = label_offset(&p.parameter, at)
                    }
                }
                DynamicBlockData::XYParameter(p) => {
                    if let Part::Label(index) = part {
                        if index == 0 {
                            p.x_value = at.y - p.parameter.definition_base_point.y
                        } else {
                            p.y_value = at.x - p.parameter.definition_base_point.x
                        }
                    }
                    if (p.parameter.definition_end_point.x - p.parameter.definition_base_point.x)
                        .abs()
                        < 1e-10
                        || (p.parameter.definition_end_point.y
                            - p.parameter.definition_base_point.y)
                            .abs()
                            < 1e-10
                    {
                        return Err("XY parameter endpoints need both X and Y distances.");
                    }
                }
                DynamicBlockData::RotationParameter(p) => match part {
                    Part::Reference => {
                        if (at - p.parameter.definition_base_point).length() < 1e-10 {
                            return Err("The rotation reference must differ from its base.");
                        }
                        p.definition_base_angle_point = at
                    }
                    Part::Label(_) => {
                        let b = p.parameter.definition_base_point;
                        p.angle =
                            (at - b).length() - (p.parameter.definition_end_point - b).length()
                    }
                    _ => {}
                },
                DynamicBlockData::FlipParameter(p) => {
                    if let Part::Label(_) = part {
                        p.definition_label_point = at
                    }
                }
                DynamicBlockData::MoveAction(p) => p.action.display_location = at,
                DynamicBlockData::ScaleAction(p) => p.action.action.display_location = at,
                DynamicBlockData::AlignmentParameter(_) => {}
                _ => return Err("This authoring object cannot be edited yet."),
            }
            if let DynamicBlockData::RotationParameter(p) = &mut object.data {
                let base = p.parameter.definition_base_point;
                if part == Part::Reference {
                    let radius = (p.definition_base_angle_point - base).length();
                    let delta = p.parameter.definition_end_point - base;
                    let end = base + delta * (radius / delta.length());
                    p.parameter.definition_end_point = end;
                    p.parameter.end_point = p.parameter.end_point.map(|_| end);
                    p.parameter.updated_end_point = p.parameter.updated_end_point.map(|_| end);
                    moved_grips.push((p.parameter.property_states[1], end));
                } else if matches!(part, Part::Base | Part::End) {
                    let old_base = item.grips.iter().find(|(p, _)| *p == Part::Base).unwrap().1;
                    let direction = p.definition_base_angle_point - old_base;
                    let radius = (p.parameter.definition_end_point - base).length();
                    p.definition_base_angle_point =
                        base + direction * (radius / direction.length());
                }
                if let Some(e) = crate::scene::block_orientation_parameter::points_error(
                    crate::scene::block_orientation_parameter::Kind::Rotation,
                    base,
                    p.parameter.definition_end_point,
                    p.definition_base_angle_point,
                ) {
                    return Err(e);
                }
            }
        }
        _ => return Err("This is not an authoring object."),
    }
    for object in staged.objects.values_mut() {
        let ObjectType::DynamicBlock(object) = object else {
            continue;
        };
        if object.owner != graph {
            continue;
        }
        let Some(g) = grip(&mut object.data) else {
            continue;
        };
        if let Some((_, location)) = moved_grips
            .iter()
            .find(|(id, _)| *id > 0 && *id == g.element.eval.node_id)
        {
            g.location = *location;
        }
    }
    // Keep the Linear grip's native direction consistent after endpoint edits.
    if matches!(part, Part::Base | Part::End) {
        if let Some(p) = distance::parameters(&staged, definition)
            .into_iter()
            .find(|p| p.handle == handle)
        {
            let delta = p.native.definition_end_point - p.native.definition_base_point;
            let orientation = delta * (1.0 / delta.length());
            let ids = p.native.property_states;
            for o in staged.objects.values_mut() {
                if let ObjectType::DynamicBlock(o) = o {
                    if o.owner == graph {
                        if let DynamicBlockData::LinearGrip(g) = &mut o.data {
                            if ids.contains(&g.grip.element.eval.node_id) {
                                g.orientation = orientation;
                            }
                        }
                    }
                }
            }
        }
        if let Some(p) = crate::scene::block_orientation_parameter::parameters(&staged, definition)
            .into_iter()
            .find(|p| p.handle == handle)
        {
            let orientation = p.native.definition_end_point - p.native.definition_base_point;
            let ids = p.native.property_states;
            for o in staged.objects.values_mut() {
                if let ObjectType::DynamicBlock(o) = o {
                    if o.owner == graph {
                        match &mut o.data {
                            DynamicBlockData::AlignmentGrip(g)
                                if ids.contains(&g.grip.element.eval.node_id) =>
                            {
                                g.orientation = orientation
                            }
                            DynamicBlockData::FlipGrip(g)
                                if ids.contains(&g.grip.element.eval.node_id) =>
                            {
                                g.orientation = orientation
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    if let Some(ObjectType::BlockVisibilityParameter(p)) = staged.objects.get(&handle) {
        staged.block_visibility_params.insert(handle, p.clone());
    }
    if let Some(item) = items(&staged, definition)
        .into_iter()
        .find(|i| i.handle == handle)
    {
        let finite = |p: Vector3| [p.x, p.y, p.z].iter().all(|v| v.is_finite());
        if !item.grips.iter().all(|(_, p)| finite(*p))
            || !item.graphic.lines.iter().flatten().all(|p| finite(*p))
        {
            return Err("The authoring location produces non-finite geometry.");
        }
    }
    *doc = staged;
    Ok(true)
}
fn label_offset(p: &BlockTwoPointParameter, at: Vector3) -> f64 {
    let d = p.definition_end_point - p.definition_base_point;
    let n = Vector3::new(-d.y, d.x, 0.) * (1.0 / d.x.hypot(d.y));
    let delta = at - (p.definition_base_point + p.definition_end_point) * 0.5;
    delta.x * n.x + delta.y * n.y
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{
        block_orientation_parameter as orientation, block_point_parameter as point, Scene,
    };
    #[test]
    fn authoring_edit_all_parameter_kinds_keep_native_grips_and_graph_identity() {
        let mut scene = Scene::new();
        scene.define_block_raw("EDIT", Vector3::ZERO, vec![]);
        let doc = &mut scene.document;
        let def = doc.block_records.get("EDIT").unwrap().handle;
        let b = Vector3::new(2., 3., 0.);
        let e = Vector3::new(6., 7., 0.);
        let l = Vector3::new(1., 8., 0.);
        point::create(doc, def, b, l).unwrap();
        for kind in [
            distance::Kind::Linear,
            distance::Kind::Polar,
            distance::Kind::XY,
        ] {
            distance::create(doc, def, kind, b, e, l).unwrap();
        }
        for kind in [
            orientation::Kind::Rotation,
            orientation::Kind::Alignment,
            orientation::Kind::Flip,
        ] {
            orientation::create(
                doc,
                def,
                kind,
                b,
                e,
                Vector3::new(b.x + (e - b).length(), b.y, 0.),
            )
            .unwrap();
        }
        crate::scene::block_lookup::create(doc, def, b).unwrap();
        crate::scene::block_basepoint::create(doc, def, b).unwrap();
        let visibility = crate::scene::block_visibility::create(doc, def, b).unwrap();
        let graph_handle = graph(doc, def).unwrap().unwrap();
        let graph_before = doc.objects[&graph_handle].clone();
        let inventory = items(doc, def);
        assert_eq!(inventory.len(), 10);
        for item in inventory {
            for (part, _) in item.grips {
                let old = items(doc, def)
                    .into_iter()
                    .find(|i| i.handle == item.handle)
                    .unwrap()
                    .grips
                    .into_iter()
                    .find(|(p, _)| *p == part)
                    .unwrap()
                    .1;
                let next = old + Vector3::new(0.3, 0.5, 0.);
                assert!(
                    move_part(doc, def, item.handle, part, next).unwrap(),
                    "{} {part:?}",
                    item.name
                );
                let after = items(doc, def)
                    .into_iter()
                    .find(|i| i.handle == item.handle)
                    .unwrap();
                let actual = after.grips.iter().find(|(p, _)| *p == part).unwrap().1;
                // Dimension labels project onto their normal/radius; definition points are exact.
                if !matches!(part, Part::Label(_)) {
                    assert!((actual - next).length() < 1e-10);
                }
                let snapshot = doc.clone();
                assert!(!move_part(doc, def, item.handle, part, actual).unwrap());
                assert!(
                    move_part(doc, def, item.handle, part, Vector3::new(f64::NAN, 0., 0.)).is_err()
                );
                assert_eq!(*doc, snapshot);
            }
        }
        assert_eq!(
            doc.objects[&graph_handle], graph_before,
            "editing positions must preserve graph lists and node identities"
        );
        assert_eq!(
            crate::scene::block_visibility::parameter(doc, def)
                .unwrap()
                .def_point,
            items(doc, def)
                .into_iter()
                .find(|i| i.handle == visibility)
                .unwrap()
                .grips[0]
                .1
        );
        for p in distance::parameters(doc, def) {
            let ids = p.native.property_states;
            let expected = p.native.definition_end_point;
            for o in doc.objects.values() {
                if let ObjectType::DynamicBlock(o) = o {
                    let mut data = o.data.clone();
                    if let Some(g) = grip(&mut data) {
                        if g.element.eval.node_id == ids[1] {
                            assert_eq!(g.location, expected);
                        }
                    }
                }
            }
        }
        let bytes = codec::DxfWriter::new(doc).write_to_vec().unwrap();
        let saved = codec::DxfReader::from_reader(std::io::Cursor::new(bytes))
            .unwrap()
            .read()
            .unwrap();
        let a = items(doc, def);
        let b = items(&saved, def);
        assert_eq!(a.len(), b.len());
        for (a, b) in a.iter().zip(b) {
            assert_eq!(a.handle, b.handle);
            assert_eq!(a.grips, b.grips);
        }
    }
}
