//! Native Move/Scale authoring and deterministic evaluation in Test Block.
use super::block_authoring::{self as authoring, *};
use codec::objects::{
    BlockAction, BlockActionOffsets, BlockActionWithBasePoint, BlockBasePointAction,
    BlockConnection, BlockElement, BlockEvalValue, BlockMoveAction, DynamicBlockData, ObjectType,
};
use codec::types::{Transform, Vector3};
use codec::{CadDocument, EntityType, Handle};
use glam::DVec3;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Move,
    Scale,
}
impl Kind {
    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "MOVE" | "M" => Some(Self::Move),
            "SCALE" | "S" => Some(Self::Scale),
            _ => None,
        }
    }
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Move => "Move",
            Self::Scale => "Scale",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Parameter {
    pub handle: Handle,
    pub id: i32,
    pub grip: Handle,
    pub name: String,
    pub base: DVec3,
    pub end: DVec3,
    pub linear: bool,
    pub point: bool,
    pub xy: bool,
}
pub(crate) fn v(p: Vector3) -> DVec3 {
    DVec3::new(p.x, p.y, p.z)
}
pub(crate) fn native(p: DVec3) -> Vector3 {
    Vector3::new(p.x, p.y, p.z)
}

pub(crate) fn parameters(doc: &CadDocument, definition: Handle, kind: Kind) -> Vec<Parameter> {
    let Ok(Some(graph)) = authoring::graph(doc, definition) else {
        return vec![];
    };
    let mut result = vec![];
    for (handle, p) in super::block_point_parameter::parameters(doc, definition) {
        if kind == Kind::Move {
            result.push(Parameter {
                handle,
                id: p.parameter.parameter.element.eval.node_id,
                grip: Handle::NULL,
                name: p.position_name.clone(),
                base: v(p.parameter.definition_point),
                end: v(p.parameter.definition_point),
                linear: false,
                point: true,
                xy: false,
            });
        }
    }
    for p in super::block_distance_parameter::parameters(doc, definition) {
        let xy = p.kind == super::block_distance_parameter::Kind::XY;
        if kind == Kind::Scale && xy {
            continue;
        }
        result.push(Parameter {
            handle: p.handle,
            id: p.native.parameter.element.eval.node_id,
            grip: Handle::NULL,
            name: p.labels.first().cloned().unwrap_or_default(),
            base: v(p.native.definition_base_point),
            end: v(p.native.definition_end_point),
            linear: p.kind == super::block_distance_parameter::Kind::Linear,
            point: false,
            xy,
        });
    }
    // Resolve the actual linked grip, rather than assuming expression IDs are contiguous.
    result.retain_mut(|p| {
        let grip_id = if p.point {
            super::block_point_parameter::parameters(doc, definition)
                .into_iter()
                .find(|(h, _)| *h == p.handle)
                .unwrap()
                .1
                .parameter
                .property_count
        } else {
            super::block_distance_parameter::parameters(doc, definition)
                .into_iter()
                .find(|q| q.handle == p.handle)
                .unwrap()
                .native
                .properties[2]
                .connections
                .first()
                .map(|c| c.code)
                .unwrap_or(-1)
        };
        let grips = doc
            .objects
            .iter()
            .filter_map(|(h, o)| {
                let ObjectType::DynamicBlock(o) = o else {
                    return None;
                };
                if o.owner != graph {
                    return None;
                }
                let grip = match &o.data {
                    DynamicBlockData::XYGrip(g) | DynamicBlockData::PolarGrip(g) => g,
                    DynamicBlockData::LinearGrip(g) => &g.grip,
                    _ => return None,
                };
                (grip.element.eval.node_id == grip_id).then_some(*h)
            })
            .collect::<Vec<_>>();
        if grips.len() != 1
            || !p.base.is_finite()
            || !p.end.is_finite()
            || (!p.point && (p.end.distance(p.base) < 1e-9 || !p.end.distance(p.base).is_finite()))
        {
            return false;
        }
        p.grip = grips[0];
        true
    });
    result.sort_by_key(|p| p.id);
    result
}

pub(crate) fn supported(e: &EntityType) -> bool {
    matches!(
        e,
        EntityType::Line(_)
            | EntityType::Circle(_)
            | EntityType::Arc(_)
            | EntityType::LwPolyline(_)
            | EntityType::Ellipse(_)
            | EntityType::Point(_)
            | EntityType::Text(_)
            | EntityType::MText(_)
    )
}
pub(crate) fn create(
    doc: &mut CadDocument,
    definition: Handle,
    kind: Kind,
    parameter: Handle,
    targets: &[Handle],
    location: DVec3,
) -> Result<Handle, &'static str> {
    if !location.is_finite() {
        return Err("Specify a finite action location.");
    }
    let p = parameters(doc, definition, kind)
        .into_iter()
        .find(|p| p.handle == parameter)
        .ok_or("Select a compatible parameter belonging to this block.")?;
    if targets.is_empty() {
        return Err("Select at least one object for the action.");
    }
    let members = &doc
        .block_records
        .iter()
        .find(|b| b.handle == definition)
        .ok_or("The block definition is unavailable.")?
        .entity_handles;
    if targets.iter().any(|h| {
        !members.contains(h)
            || doc
                .get_entity(*h)
                .is_none_or(|e| !supported(e) || e.common().owner_handle != definition)
    }) {
        return Err("This milestone supports lines, polylines, arcs, circles, ellipses, points and text belonging to this block.");
    }
    let mut staged = doc.clone();
    let (graph, slots) = authoring::prepare(&mut staged, definition)?;
    let (dxf, cpp) = match kind {
        Kind::Move => ("BLOCKMOVEACTION", "AcDbBlockMoveAction"),
        Kind::Scale => ("BLOCKSCALEACTION", "AcDbBlockScaleAction"),
    };
    register_classes(&mut staged, &[(dxf, cpp, 55)]);
    let handle = staged.allocate_handle();
    let mut dependencies = targets.to_vec();
    dependencies.sort();
    dependencies.dedup();
    let action = BlockAction {
        element: BlockElement {
            eval: expression(slots.eval, NO_VALUE, BlockEvalValue::None),
            name: format!("{}{}", kind.name(), slots.eval),
            major: MAJOR,
            minor: MINOR,
            eed_1071: 0,
        },
        display_location: native(location),
        dependencies,
        action_ids: vec![],
    };
    let connection = |name: &str| BlockConnection {
        code: p.id,
        name: name.into(),
    };
    let data = match kind {
        Kind::Move => DynamicBlockData::MoveAction(BlockMoveAction {
            action,
            connections: if p.point {
                [connection("XDelta"), connection("YDelta")]
            } else {
                [connection("EndXDelta"), connection("EndYDelta")]
            },
            offsets: BlockActionOffsets {
                distance_multiplier: 1.,
                angle_offset: 0.,
                flags: 0,
            },
        }),
        Kind::Scale => DynamicBlockData::ScaleAction(BlockBasePointAction {
            action: BlockActionWithBasePoint {
                action,
                offset: Vector3::ZERO,
                connections: vec![connection("UpdatedBaseX"), connection("UpdatedBaseY")],
                dependent: true,
                base_point: Vector3::ZERO,
            },
            connections: vec![
                connection("Scale"),
                connection("XScale"),
                connection("YScale"),
            ],
            dwg_tail_byte: 0,
        }),
    };
    staged.objects.insert(
        handle,
        ObjectType::DynamicBlock(dynamic(handle, graph, dxf, cpp, data)),
    );
    authoring::append_single(&mut staged, graph, slots, handle);
    authoring::graph::bind_ids(&mut staged, graph, slots.eval, &[p.id])?;
    *doc = staged;
    Ok(handle)
}

pub(crate) fn actions(
    doc: &CadDocument,
    definition: Handle,
) -> Vec<(Handle, &BlockAction, Kind, i32)> {
    let Ok(Some(graph)) = authoring::graph(doc, definition) else {
        return vec![];
    };
    let ObjectType::DynamicBlock(o) = &doc.objects[&graph] else {
        return vec![];
    };
    let DynamicBlockData::EvaluationGraph(g) = &o.data else {
        return vec![];
    };
    g.nodes
        .iter()
        .filter_map(|n| {
            let ObjectType::DynamicBlock(o) = doc.objects.get(&n.expression)? else {
                return None;
            };
            if o.owner != graph {
                return None;
            }
            match &o.data {
                DynamicBlockData::MoveAction(a) => {
                    Some((o.handle, &a.action, Kind::Move, a.connections[0].code))
                }
                DynamicBlockData::ScaleAction(a) => a
                    .connections
                    .first()
                    .map(|c| (o.handle, &a.action.action, Kind::Scale, c.code)),
                _ => None,
            }
        })
        .collect()
}
pub(crate) fn reframe(doc: &mut CadDocument, definition: Handle, t: &Transform) -> usize {
    let handles = actions(doc, definition)
        .iter()
        .map(|(h, _, _, _)| *h)
        .collect::<Vec<_>>();
    for h in &handles {
        let ObjectType::DynamicBlock(o) = doc.objects.get_mut(h).unwrap() else {
            unreachable!()
        };
        match &mut o.data {
            DynamicBlockData::MoveAction(a) => {
                a.action.display_location = t.apply(a.action.display_location)
            }
            DynamicBlockData::ScaleAction(a) => {
                a.action.action.display_location = t.apply(a.action.action.display_location);
                a.action.base_point = t.apply(a.action.base_point);
                a.action.offset = t.apply_rotation(a.action.offset);
            }
            _ => {}
        }
    }
    handles.len()
}

pub(crate) fn current(doc: &CadDocument, p: &Parameter) -> DVec3 {
    let Some(ObjectType::DynamicBlock(o)) = doc.objects.get(&p.grip) else {
        return p.end;
    };
    match &o.data {
        DynamicBlockData::XYGrip(g) | DynamicBlockData::PolarGrip(g) => v(g.location),
        DynamicBlockData::LinearGrip(g) => v(g.grip.location),
        _ => p.end,
    }
}

/// Visibility can give the test INSERT an anonymous representation. Keep its
/// parallel members in sync, retaining their own handles and visibility flags.
pub(crate) fn sync_representation(
    doc: &mut CadDocument,
    definition: Handle,
    name: &str,
) -> Result<(), &'static str> {
    let original = doc
        .block_records
        .iter()
        .find(|b| b.handle == definition)
        .ok_or("The test definition is unavailable.")?;
    if original.name == name {
        return Ok(());
    }
    let source = original.entity_handles.clone();
    let target = doc
        .block_records
        .get(name)
        .ok_or("The test representation is unavailable.")?
        .entity_handles
        .clone();
    if source.len() != target.len() {
        return Err("The test representation no longer matches its definition.");
    }
    let affected = actions(doc, definition)
        .iter()
        .flat_map(|(_, a, _, _)| a.dependencies.iter().copied())
        .collect::<std::collections::BTreeSet<_>>();
    for (from, to) in source.into_iter().zip(target) {
        if !affected.contains(&from) {
            continue;
        }
        let mut entity = doc
            .get_entity(from)
            .cloned()
            .ok_or("An action target is unavailable.")?;
        let current = doc
            .get_entity_mut(to)
            .ok_or("An action representation target is unavailable.")?;
        *entity.common_mut() = current.common().clone();
        *current = entity;
    }
    Ok(())
}

/// Re-evaluate from the scratch document's immutable starting geometry. This
/// avoids incremental drift and makes repeated changes/order independent.
pub(crate) fn evaluate(
    doc: &mut CadDocument,
    baseline: &CadDocument,
    definition: Handle,
    parameter: Handle,
    position: DVec3,
) -> Result<bool, &'static str> {
    let graph = authoring::graph(baseline, definition)?
        .ok_or("The test authoring graph is unavailable.")?;
    if baseline.objects.values().any(|o|matches!(o,ObjectType::DynamicBlock(o) if o.owner==graph && matches!(&o.data,
        DynamicBlockData::StretchAction(_)|DynamicBlockData::PolarStretchAction(_)|DynamicBlockData::RotateAction(_)|DynamicBlockData::FlipAction(_)|DynamicBlockData::ArrayAction(_)))) {
        return Err("This block contains action types not supported in Test Block yet.");
    }
    let p = parameters(baseline, definition, Kind::Move)
        .into_iter()
        .find(|p| p.handle == parameter)
        .ok_or("The test parameter is unavailable.")?;
    if !position.is_finite() {
        return Err("Specify a finite parameter position.");
    }
    let mut position = position;
    position.z = p.end.z;
    if p.linear {
        let axis = (p.end - p.base).normalize();
        let length = (position - p.base).dot(axis);
        if length < 1e-9 {
            return Err("Specify a positive parameter length.");
        }
        position = p.base + axis * length;
    }
    if current(doc, &p).distance(position) < 1e-10 {
        return Ok(false);
    }
    let mut staged = doc.clone();
    let ObjectType::DynamicBlock(o) = staged
        .objects
        .get_mut(&p.grip)
        .ok_or("The test grip is unavailable.")?
    else {
        return Err("The test grip is unavailable.");
    };
    match &mut o.data {
        DynamicBlockData::XYGrip(g) | DynamicBlockData::PolarGrip(g) => {
            g.location = native(position)
        }
        DynamicBlockData::LinearGrip(g) => g.grip.location = native(position),
        _ => return Err("The test grip is unavailable."),
    }
    let all = actions(baseline, definition);
    let mut affected = std::collections::BTreeSet::new();
    for (_, a, _, _) in &all {
        affected.extend(a.dependencies.iter().copied());
    }
    for h in affected {
        let mut entity = baseline
            .get_entity(h)
            .cloned()
            .ok_or("An action target is unavailable.")?;
        if !supported(&entity) {
            return Err("This action contains an unsupported object type.");
        }
        // State flags may have changed through the existing visibility grip.
        if let Some(e) = staged.get_entity(h) {
            entity.common_mut().invisible = e.common().invisible;
        }
        for (action_handle, a, kind, id) in &all {
            if !a.dependencies.contains(&h) {
                continue;
            }
            let q = parameters(baseline, definition, *kind)
                .into_iter()
                .find(|p| p.id == *id)
                .ok_or("An action has an unsupported parameter binding.")?;
            if !test_parameter_supported(baseline, q.handle) {
                return Err("This parameter uses value constraints or chaining not supported in Test Block yet.");
            }
            let end = current(&staged, &q);
            let transform = match kind {
                Kind::Move => crate::command::EntityTransform::Translate(end - q.end),
                Kind::Scale => {
                    let factor = end.distance(q.base) / q.end.distance(q.base);
                    if !factor.is_finite() || factor < 1e-9 {
                        return Err("Specify a positive scale distance.");
                    }
                    crate::command::EntityTransform::Scale {
                        center: q.base,
                        factor,
                    }
                }
            };
            // Imported actions with custom multipliers/offsets/chaining must
            // not silently behave like the simple actions authored here.
            if !a.action_ids.is_empty() {
                return Err("Chained actions are not supported in Test Block yet.");
            }
            let ObjectType::DynamicBlock(o) = &baseline.objects[action_handle] else {
                unreachable!()
            };
            match &o.data {
                DynamicBlockData::MoveAction(m)
                    if m.offsets.distance_multiplier != 1.
                        || m.offsets.angle_offset != 0.
                        || m.offsets.flags != 0
                        || m.connections
                            != if q.point {
                                [
                                    BlockConnection {
                                        code: q.id,
                                        name: "XDelta".into(),
                                    },
                                    BlockConnection {
                                        code: q.id,
                                        name: "YDelta".into(),
                                    },
                                ]
                            } else {
                                [
                                    BlockConnection {
                                        code: q.id,
                                        name: "EndXDelta".into(),
                                    },
                                    BlockConnection {
                                        code: q.id,
                                        name: "EndYDelta".into(),
                                    },
                                ]
                            } =>
                {
                    return Err("This Move action uses options not supported in Test Block yet.")
                }
                DynamicBlockData::ScaleAction(s)
                    if !s.action.dependent
                        || s.action.offset != Vector3::ZERO
                        || s.connections
                            != ["Scale", "XScale", "YScale"].map(|name| BlockConnection {
                                code: q.id,
                                name: name.into(),
                            })
                        || s.action.connections
                            != ["UpdatedBaseX", "UpdatedBaseY"].map(|name| BlockConnection {
                                code: q.id,
                                name: name.into(),
                            }) =>
                {
                    return Err("This Scale action uses options not supported in Test Block yet.")
                }
                _ => {}
            }
            super::view::dispatch::apply_transform(&mut entity, &transform);
            let bounds = entity.as_entity().bounding_box();
            if !v(bounds.min).is_finite() || !v(bounds.max).is_finite() {
                return Err("The parameter value produces non-finite geometry.");
            }
        }
        *staged
            .get_entity_mut(h)
            .ok_or("An action target was removed.")? = entity;
    }
    *doc = staged;
    Ok(true)
}

fn test_parameter_supported(doc: &CadDocument, handle: Handle) -> bool {
    let Some(ObjectType::DynamicBlock(o)) = doc.objects.get(&handle) else {
        return false;
    };
    let distance = |s: &codec::objects::BlockParameterValueSet| {
        s.flags == 1
            && s.minimum == 0.
            && s.maximum == 0.
            && s.increment == 0.
            && s.values.is_empty()
    };
    match &o.data {
        DynamicBlockData::PointParameter(p) => !p.parameter.parameter.chain_actions,
        DynamicBlockData::LinearParameter(p) => {
            !p.parameter.parameter.chain_actions && distance(&p.value_set)
        }
        DynamicBlockData::XYParameter(p) => {
            !p.parameter.parameter.chain_actions
                && distance(&p.x_value_set)
                && distance(&p.y_value_set)
        }
        DynamicBlockData::PolarParameter(p) => {
            !p.parameter.parameter.chain_actions
                && distance(&p.distance_value_set)
                && p.angle_value_set.flags == 3
                && p.angle_value_set.minimum == 0.
                && p.angle_value_set.maximum == std::f64::consts::TAU
                && p.angle_value_set.increment == 0.
                && p.angle_value_set.values.is_empty()
        }
        _ => false,
    }
}
