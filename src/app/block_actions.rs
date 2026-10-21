use super::{Message, OpenCADStudio};
use crate::command::CadCommand;
use crate::scene::block_actions::{self as actions, Kind};
use codec::{EntityType, Handle};
use iced::Task;

// These grips start an ordinary point-input command, keeping snapping, typed
// coordinates and Escape on the command driver's existing path.
pub(super) const TEST_GRIP_START: usize = usize::MAX / 2;
pub(super) fn is_test_grip(id: usize) -> bool {
    (TEST_GRIP_START..usize::MAX).contains(&id)
}
impl OpenCADStudio {
    pub(in crate::app) fn action_association_error(&self) -> Option<&'static str> {
        let tab = &self.tabs[self.active_tab];
        let Some(session) = tab.active_block_edit_session() else {
            return Some("BASSOCIATE is only available in the Block Editor.");
        };
        let orphaned = actions::association::orphaned(&tab.scene.document, session.br_handle);
        if orphaned.is_empty() {
            return Some("This definition has no unassociated actions.");
        }
        let selected = tab
            .selected_authoring
            .filter(|selected| orphaned.contains(selected));
        let compatible = actions::actions(&tab.scene.document, session.br_handle)
            .into_iter()
            .filter(|(handle, _, _, _)| {
                orphaned.contains(handle) && selected.is_none_or(|selected| selected == *handle)
            })
            .any(|(_, _, kind, _)| {
                !actions::parameters(&tab.scene.document, session.br_handle, kind).is_empty()
            });
        if !compatible {
            return Some("Add a compatible parameter before associating an action.");
        }
        None
    }

    pub(in crate::app) fn start_block_action_association(&mut self) -> Task<Message> {
        if let Some(error) = self.action_association_error() {
            self.command_line.push_error(crate::t!(error).as_ref());
            return Task::none();
        }
        let tab = &self.tabs[self.active_tab];
        let command = crate::modules::block_editor::associate::AssociateCommand::new(
            tab.id,
            tab.active_block_edit_session().unwrap().br_handle,
            &tab.scene.document,
            tab.selected_authoring,
        );
        self.command_line.push_info(&command.prompt());
        self.command_line.set_step_options(command.options());
        self.tabs[self.active_tab].active_cmd = Some(Box::new(command));
        Task::none()
    }
    pub(in crate::app) fn reset_selected_block_references(&mut self, apply: bool) -> Task<Message> {
        let i = self.active_tab;
        let handles = self.tabs[i]
            .scene
            .selected_entities()
            .iter()
            .map(|(handle, _)| *handle)
            .collect::<Vec<_>>();
        if handles.is_empty() && !apply {
            let command = crate::modules::block_editor::reset::ResetCommand::new();
            self.command_line.push_info(&command.prompt());
            self.tabs[i].active_cmd = Some(Box::new(command));
            return Task::none();
        }
        let mut staged = self.tabs[i].scene.document.clone();
        let mut changed = 0;
        for handle in handles {
            let Some(definition) = staged.dynamic_definition_for_insert(handle) else {
                continue;
            };
            if self.tabs[i].scene.is_layer_locked(handle) {
                self.command_line.push_error(
                    crate::t!("RESETBLOCK: a selected reference is on a locked layer.").as_ref(),
                );
                return Task::none();
            }
            let result = if let Some(test) = &self.tabs[i].block_test {
                if handle != test.insert || definition != test.definition
                    || staged.entities().any(|entity| matches!(entity, EntityType::Insert(other) if other.common.handle != handle && staged.dynamic_definition_for_insert(other.common.handle) == Some(definition))) {
                    self.command_line.push_error(crate::t!("Reset the single test reference or reopen Test Block.").as_ref());
                    return Task::none();
                }
                actions::instance::reset_test(&staged, &test.baseline, definition, handle)
            } else {
                actions::instance::reset(&staged, handle)
            };
            match result {
                Ok(Some(next)) => {
                    staged = next;
                    changed += 1;
                }
                Ok(None) => {}
                Err(error) => {
                    self.command_line.push_error(crate::t!(error).as_ref());
                    return Task::none();
                }
            }
        }
        if changed != 0 {
            self.push_undo_snapshot(i, "Reset block references");
            self.apply_action_document(i, staged);
            self.tabs[i].scene.bump_geometry();
            self.tabs[i].dirty = true;
            self.finish_pending_history(i);
            self.refresh_properties();
        }
        self.command_line
            .push_output(crate::tf!("Reset {} dynamic block references.", changed).as_ref());
        Task::none()
    }

    pub(in crate::app) fn action_error(&self, kind: Option<Kind>) -> Option<&'static str> {
        let tab = &self.tabs[self.active_tab];
        let Some(s) = tab.active_block_edit_session() else {
            return Some("Open a block in the Block Editor before adding actions.");
        };
        if let Some(e) =
            crate::scene::block_point_parameter::placement_error(&tab.scene.document, s.br_handle)
        {
            return Some(e);
        }
        let candidates = kind.map_or_else(
            || actions::all_parameters(&tab.scene.document, s.br_handle),
            |kind| actions::parameters(&tab.scene.document, s.br_handle, kind),
        );
        if candidates.is_empty() {
            return Some("Place a compatible parameter before adding this action.");
        }
        None
    }
    pub(in crate::app) fn action_set_error(&self) -> Option<&'static str> {
        let tab = &self.tabs[self.active_tab];
        let Some(session) = tab.active_block_edit_session() else {
            return Some("BACTIONSET is only available in the Block Editor.");
        };
        if let Some(e) = crate::scene::block_point_parameter::placement_error(
            &tab.scene.document,
            session.br_handle,
        ) {
            return Some(e);
        }
        actions::actions(&tab.scene.document, session.br_handle)
            .is_empty()
            .then_some("This block has no supported actions to edit.")
    }
    pub(in crate::app) fn start_block_action_set(&mut self) -> Task<Message> {
        if let Some(e) = self.action_set_error() {
            self.command_line.push_error(crate::t!(e).as_ref());
            return Task::none();
        }
        let tab = &self.tabs[self.active_tab];
        let command = crate::modules::block_editor::action_set::ActionSetCommand::new(
            tab.id,
            tab.active_block_edit_session().unwrap().br_handle,
            &tab.scene.document,
            tab.selected_authoring,
        );
        self.command_line.push_info(&command.prompt());
        self.command_line.set_step_options(command.options());
        self.tabs[self.active_tab].active_cmd = Some(Box::new(command));
        Task::none()
    }
    pub(super) fn apply_action_document(&mut self, i: usize, mut staged: codec::CadDocument) {
        let doc = &mut self.tabs[i].scene.document;
        let changes = staged
            .entities()
            .filter(|e| doc.get_entity(e.common().handle) != Some(*e))
            .cloned()
            .collect::<Vec<_>>();
        let removed = doc
            .entities()
            .map(|e| e.common().handle)
            .filter(|h| staged.get_entity(*h).is_none())
            .collect::<Vec<_>>();
        doc.swap_structure(staged.snapshot_structure());
        for handle in removed {
            doc.remove_entity_arc(handle);
        }
        for entity in changes {
            let handle = entity.common().handle;
            if let Some(current) = doc.get_entity_mut(handle) {
                *current = entity;
            } else {
                doc.restore_entity_arc(std::sync::Arc::new(entity));
            }
        }
    }
    pub(in crate::app) fn start_block_action(&mut self, args: &str) -> Task<Message> {
        let kind = if args.is_empty() {
            None
        } else {
            Kind::parse(args)
        };
        if !args.is_empty() && kind.is_none() {
            self.command_line.push_error(
                crate::t!("Supported actions: Move, Scale, Stretch, Polar Stretch, Rotate, Flip, Array. Other action types are not wired yet.")
                    .as_ref(),
            );
            return Task::none();
        }
        if let Some(e) = self.action_error(kind) {
            self.command_line.push_error(crate::t!(e).as_ref());
            return Task::none();
        }
        let tab = &self.tabs[self.active_tab];
        let definition = tab.active_block_edit_session().unwrap().br_handle;
        let command = crate::modules::block_editor::action::ActionCommand::new(
            tab.id,
            definition,
            kind,
            kind.map_or_else(
                || actions::all_parameters(&tab.scene.document, definition),
                |kind| actions::parameters(&tab.scene.document, definition, kind),
            ),
        );
        self.command_line.push_info(&command.prompt());
        self.command_line.set_step_options(command.options());
        self.tabs[self.active_tab].active_cmd = Some(Box::new(command));
        Task::none()
    }
    pub(in crate::app) fn start_block_action_for_parameter(
        &mut self,
        args: &str,
        parameter: codec::Handle,
    ) -> Task<Message> {
        self.start_block_action_for_parameter_at(args, parameter, None)
    }
    fn start_block_action_for_parameter_at(
        &mut self,
        args: &str,
        parameter: codec::Handle,
        xy_key_point: Option<actions::XyKeyPoint>,
    ) -> Task<Message> {
        let Some(kind) = Kind::parse(args) else {
            self.command_line.push_error(
                crate::t!("The Parameter Set's action type is not supported.").as_ref(),
            );
            return Task::none();
        };
        if let Some(error) = self.action_error(Some(kind)) {
            self.command_line.push_error(crate::t!(error).as_ref());
            return Task::none();
        }
        let tab = &self.tabs[self.active_tab];
        let definition = tab.active_block_edit_session().unwrap().br_handle;
        let candidates = actions::parameters(&tab.scene.document, definition, kind);
        let Some(command) = crate::modules::block_editor::action::ActionCommand::for_parameter(
            tab.id,
            definition,
            kind,
            candidates,
            parameter,
            xy_key_point,
        ) else {
            self.command_line.push_error(
                crate::t!("The placed parameter is incompatible with this action.").as_ref(),
            );
            return Task::none();
        };
        self.command_line.push_info(&command.prompt());
        self.command_line.set_step_options(command.options());
        self.tabs[self.active_tab].active_cmd = Some(Box::new(command));
        Task::none()
    }
    pub(in crate::app) fn start_xy_box_set_action(
        &mut self,
        parameter: codec::Handle,
        kind: Kind,
        key_point: u8,
    ) -> Task<Message> {
        let key_point = match key_point {
            0 => actions::XyKeyPoint::Base,
            1 => actions::XyKeyPoint::X,
            2 => actions::XyKeyPoint::Y,
            3 => actions::XyKeyPoint::End,
            _ => return Task::none(),
        };
        self.start_block_action_for_parameter_at(kind.name(), parameter, Some(key_point))
    }
    pub(in crate::app) fn start_parameter_pair_action(
        &mut self,
        parameter: codec::Handle,
        kind: Kind,
        side: u8,
    ) -> Task<Message> {
        let key_point = match side {
            0 => actions::XyKeyPoint::Base,
            1 => actions::XyKeyPoint::End,
            _ => return Task::none(),
        };
        self.start_block_action_for_parameter_at(kind.name(), parameter, Some(key_point))
    }
    pub(in crate::app) fn handle_block_action_result(
        &mut self,
        result: crate::command::CmdResult,
    ) -> Task<Message> {
        let i = self.active_tab;
        match result {
            crate::command::CmdResult::AssociateBlockAction {
                document,
                definition,
                action,
                expected,
                parameter,
                key_point,
            } => {
                let tab = &self.tabs[i];
                if tab.id != document
                    || tab
                        .active_block_edit_session()
                        .map(|session| session.br_handle)
                        != Some(definition)
                    || tab.scene.document.objects.get(&action) != Some(expected.as_ref())
                {
                    self.command_line.push_error(
                        crate::t!(
                            "The action or its Block Editor session changed; restart BASSOCIATE."
                        )
                        .as_ref(),
                    );
                    return Task::none();
                }
                let mut staged = tab.scene.document.clone();
                let result = actions::association::associate(
                    &mut staged,
                    definition,
                    action,
                    parameter,
                    key_point,
                )
                .and_then(|changed| {
                    if changed && actions::association::orphaned(&staged, definition).is_empty() {
                        staged = actions::instance::refresh(&staged, definition)?;
                    }
                    Ok(changed)
                });
                match result {
                    Ok(changed) => {
                        if changed {
                            self.push_undo_snapshot(i, "Associate block action");
                            self.apply_action_document(i, staged);
                            self.tabs[i].scene.bump_geometry();
                            self.tabs[i].dirty = true;
                            self.finish_pending_history(i);
                        }
                        self.tabs[i].active_cmd = None;
                        self.tabs[i].scene.clear_preview_wire();
                        self.tabs[i].selected_authoring = Some(action);
                        self.refresh_properties();
                        self.command_line
                            .push_output(crate::t!("Action associated with parameter.").as_ref());
                    }
                    Err(error) => self.command_line.push_error(crate::t!(error).as_ref()),
                }
            }
            crate::command::CmdResult::EditBlockActionSet {
                document,
                definition,
                action,
                expected,
                targets,
                frame,
                rotate_only,
            } => {
                let tab = &self.tabs[i];
                if tab.id != document
                    || tab.active_block_edit_session().map(|s| s.br_handle) != Some(definition)
                    || tab.scene.document.objects.get(&action) != Some(expected.as_ref())
                {
                    self.command_line.push_error(
                        crate::t!(
                            "The action or its Block Editor session changed; restart BACTIONSET."
                        )
                        .as_ref(),
                    );
                    return Task::none();
                }
                let mut staged = tab.scene.document.clone();
                let result = actions::retarget_frame_points(
                    &mut staged,
                    definition,
                    action,
                    &targets,
                    frame.as_deref(),
                )
                .and_then(|changed| {
                    let rotation_changed = match rotate_only.as_deref() {
                        Some(targets) => actions::options::set_rotate_only(
                            &mut staged,
                            definition,
                            action,
                            targets,
                        )?,
                        None => false,
                    };
                    let changed = changed || rotation_changed;
                    if changed {
                        staged = actions::instance::refresh(&staged, definition)?;
                    }
                    Ok(changed)
                });
                match result {
                    Ok(changed) => {
                        if changed {
                            self.push_undo_snapshot(i, "Action selection set");
                            self.apply_action_document(i, staged);
                            self.tabs[i].scene.bump_geometry();
                            self.tabs[i].dirty = true;
                            self.finish_pending_history(i);
                        }
                        self.tabs[i].active_cmd = None;
                        self.tabs[i].scene.clear_preview_wire();
                        self.refresh_properties();
                        self.command_line
                            .push_output(crate::t!("Action selection updated.").as_ref());
                    }
                    Err(e) => self.command_line.push_error(crate::t!(e).as_ref()),
                }
            }
            crate::command::CmdResult::PlaceBlockAction {
                document,
                definition,
                kind,
                parameter,
                xy_key_point,
                targets,
                location,
                frame,
                array_offsets,
                displacement_overrides,
                rotate_only,
            } => {
                if self.tabs[i].id != document
                    || self.tabs[i]
                        .active_block_edit_session()
                        .map(|s| s.br_handle)
                        != Some(definition)
                {
                    self.command_line.push_error(
                        crate::t!("The action's Block Editor session is no longer active.")
                            .as_ref(),
                    );
                    return Task::none();
                }
                let mut staged = self.tabs[i].scene.document.clone();
                match actions::create_with_frame_points(
                    &mut staged,
                    definition,
                    kind,
                    parameter,
                    xy_key_point,
                    &targets,
                    location,
                    frame.as_deref(),
                    array_offsets,
                )
                .and_then(|handle| {
                    if let Some((multiplier, angle)) = displacement_overrides {
                        actions::options::set(&mut staged, definition, handle, multiplier, angle)?;
                    }
                    if kind == Kind::PolarStretch {
                        actions::options::set_rotate_only(
                            &mut staged,
                            definition,
                            handle,
                            &rotate_only,
                        )?;
                    } else if !rotate_only.is_empty() {
                        return Err("Rotate-only objects require a Polar Stretch action.");
                    }
                    Ok(handle)
                }) {
                    Ok(_) => {
                        self.push_undo_snapshot(i, &format!("{} action", kind.name()));
                        self.tabs[i].scene.document = staged;
                        self.tabs[i].scene.bump_geometry();
                        self.tabs[i].dirty = true;
                        self.tabs[i].active_cmd = None;
                        self.tabs[i].scene.clear_preview_wire();
                        self.finish_pending_history(i);
                        self.refresh_properties();
                        self.command_line.push_output(crate::tf!("{} action attached. Select the reference and use its parameter grip, or open Test Block.",crate::t!(kind.name())).as_ref());
                        if let Some((set_parameter, expected_kind, next_key_point)) =
                            self.tabs[i].pending_xy_box_set
                        {
                            let completed_key_point = match xy_key_point {
                                Some(actions::XyKeyPoint::Base) => 0,
                                Some(actions::XyKeyPoint::X) => 1,
                                Some(actions::XyKeyPoint::Y) => 2,
                                Some(actions::XyKeyPoint::End) => 3,
                                None => 3,
                            };
                            if set_parameter == parameter
                                && kind == expected_kind
                                && usize::from(next_key_point) == completed_key_point
                                && next_key_point < 3
                            {
                                let next = next_key_point + 1;
                                self.tabs[i].pending_xy_box_set = Some((parameter, kind, next));
                                return self.start_xy_box_set_action(parameter, kind, next);
                            }
                            self.tabs[i].pending_xy_box_set = None;
                        }
                        if let Some((set_parameter, expected_kind, expected_side)) =
                            self.tabs[i].pending_action_pair
                        {
                            let completed_side = match xy_key_point {
                                Some(actions::XyKeyPoint::Base) => Some(0),
                                Some(actions::XyKeyPoint::End) | None => Some(1),
                                _ => None,
                            };
                            if set_parameter == parameter
                                && kind == expected_kind
                                && completed_side == Some(expected_side)
                                && expected_side == 0
                            {
                                self.tabs[i].pending_action_pair = Some((parameter, kind, 1));
                                return self.start_parameter_pair_action(parameter, kind, 1);
                            }
                            self.tabs[i].pending_action_pair = None;
                        }
                    }
                    Err(e) => self.command_line.push_error(crate::t!(e).as_ref()),
                }
            }
            crate::command::CmdResult::EvaluateBlockParameter {
                insert,
                document,
                definition,
                parameter,
                value,
            } => {
                if self.tabs[i].id != document {
                    return Task::none();
                }
                if let Some(insert) = insert {
                    return self.evaluate_reference_parameter(insert, definition, parameter, value);
                }
                let Some(test) = self.tabs[i].block_test.as_ref() else {
                    return Task::none();
                };
                if test.definition != definition {
                    return Task::none();
                }
                let Some(EntityType::Insert(insert)) =
                    self.tabs[i].scene.document.get_entity(test.insert)
                else {
                    return Task::none();
                };
                let doc = &self.tabs[i].scene.document;
                if doc.entities().any(|e|matches!(e,EntityType::Insert(other) if other.common.handle!=test.insert && (other.block_name==insert.block_name||doc.dynamic_definition_for_insert(other.common.handle)==Some(definition)))) {
                    self.command_line.push_error(crate::t!("Test actions with one reference in Test Block. Remove the extra references or reopen Test Block.").as_ref());return Task::none();
                }
                let Some(inverse) =
                    crate::scene::inverse_affine(&crate::scene::render_graph::insert_transform(
                        &self.tabs[i].scene.document,
                        insert,
                    ))
                else {
                    self.command_line.push_error(
                        crate::t!("The test reference has a singular transform.").as_ref(),
                    );
                    return Task::none();
                };
                let local = match value {
                    actions::Value::Point(at) => {
                        actions::Value::Point(actions::v(inverse.apply(actions::native(at))))
                    }
                    state => state,
                };
                let representation = insert.block_name.clone();
                let mut staged = self.tabs[i].scene.document.clone();
                match actions::evaluate_value(
                    &mut staged,
                    &test.baseline,
                    definition,
                    parameter,
                    local,
                ) {
                    Ok(changed) => {
                        if changed {
                            if let Err(e) = actions::sync_representation(
                                &mut staged,
                                definition,
                                &representation,
                            ) {
                                self.command_line.push_error(crate::t!(e).as_ref());
                                return Task::none();
                            }
                            self.finish_pending_history(i);
                            let before = &self.tabs[i].scene.document;
                            let entities = before
                                .entities()
                                .filter_map(|e| {
                                    let h = e.common().handle;
                                    (staged.get_entity(h) != Some(e))
                                        .then(|| (h, before.get_entity_arc(h).unwrap()))
                                })
                                .collect();
                            let objects = before
                                .objects
                                .iter()
                                .filter(|(h, o)| staged.objects.get(h) != Some(*o))
                                .map(|(h, o)| (*h, o.clone()))
                                .collect();
                            let dirty_before = self.tabs[i].dirty;
                            self.tabs[i].scene.document = staged;
                            self.tabs[i].scene.bump_geometry();
                            self.tabs[i].dirty = true;
                            self.push_entity_group_history(
                                i,
                                "Test block parameter",
                                entities,
                                objects,
                                dirty_before,
                            );
                        }
                        self.tabs[i].active_cmd = None;
                        self.tabs[i].scene.clear_preview_wire();
                        self.refresh_properties();
                    }
                    Err(e) => self.command_line.push_error(crate::t!(e).as_ref()),
                }
            }
            _ => unreachable!(),
        }
        Task::none()
    }
    pub(super) fn refresh_test_action_grips(&mut self, wo: [f64; 3]) {
        let i = self.active_tab;
        let tab = &self.tabs[i];
        let Some(test) = tab.block_test.as_ref() else {
            self.refresh_reference_action_grips(wo);
            return;
        };
        if tab.selected_handle != Some(test.insert) {
            return;
        }
        let Some(EntityType::Insert(insert)) = tab.scene.document.get_entity(test.insert) else {
            return;
        };
        let transform = crate::scene::render_graph::insert_transform(&tab.scene.document, insert);
        let mut grips = vec![];
        let all = actions::actions(&test.baseline, test.definition);
        for (index, p) in actions::all_parameters(&test.baseline, test.definition)
            .into_iter()
            .enumerate()
        {
            if p.grip == Handle::NULL || !all.iter().any(|(_, _, _, id)| *id == p.id) {
                continue;
            }
            let Ok(value) = actions::value(&tab.scene.document, &p) else {
                continue;
            };
            let world =
                actions::v(transform.apply(actions::native(actions::grip_position(&p, value))))
                    - glam::DVec3::from_array(wo);
            grips.push(crate::scene::model::object::GripDef {
                id: TEST_GRIP_START + index,
                world,
                is_midpoint: false,
                shape: crate::scene::model::object::GripShape::Triangle,
                dir: actions::grip_direction(&p, value).and_then(|direction| {
                    (actions::v(transform.apply(actions::native(direction)))
                        - actions::v(transform.apply(codec::types::Vector3::ZERO)))
                    .try_normalize()
                }),
                axis: None,
            });
        }
        let handle = test.insert;
        let tab = &mut self.tabs[i];
        for grip in grips {
            tab.selected_grips.push(grip);
            tab.selected_grip_handles.push(handle);
        }
    }
    pub(super) fn start_test_action_grip(&mut self, handle: Handle, id: usize) -> bool {
        if !is_test_grip(id) {
            return false;
        }
        let i = self.active_tab;
        let tab = &self.tabs[i];
        let Some(test) = tab.block_test.as_ref() else {
            return self.start_reference_action_grip(handle, id);
        };
        if handle != test.insert {
            return false;
        }
        if tab.scene.is_layer_locked(handle) {
            return true;
        }
        let Some(p) = actions::all_parameters(&test.baseline, test.definition)
            .get(id - TEST_GRIP_START)
            .cloned()
        else {
            return false;
        };
        if p.grip == Handle::NULL {
            return false;
        }
        if p.flip {
            let state = actions::value(&tab.scene.document, &p);
            let document = tab.id;
            let definition = test.definition;
            return self.toggle_action_grip(document, definition, p.handle, None, state);
        }
        let command = crate::modules::block_editor::action::TestParameterCommand {
            insert: None,
            document: tab.id,
            definition: test.definition,
            parameter: p.handle,
        };
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        self.grip_hover = None;
        self.grip_popup = None;
        true
    }
}

impl OpenCADStudio {
    fn refresh_reference_action_grips(&mut self, wo: [f64; 3]) {
        let i = self.active_tab;
        let tab = &self.tabs[i];
        if tab.active_block_edit_session().is_some() || tab.scene.selected.len() != 1 {
            return;
        }
        let Some(handle) = tab.selected_handle else {
            return;
        };
        let Some(def) = actions::instance::definition(&tab.scene.document, handle) else {
            return;
        };
        let Some(EntityType::Insert(reference)) = tab.scene.document.get_entity(handle) else {
            return;
        };
        let transform =
            crate::scene::render_graph::insert_transform(&tab.scene.document, reference);
        let all = actions::actions(&tab.scene.document, def);
        let mut grips = vec![];
        for (index, p) in actions::all_parameters(&tab.scene.document, def)
            .into_iter()
            .enumerate()
        {
            if p.grip == Handle::NULL || !all.iter().any(|(_, _, _, id)| *id == p.id) {
                continue;
            }
            let Ok(value) = actions::instance::value(&tab.scene.document, handle, &p) else {
                continue;
            };
            let world =
                actions::v(transform.apply(actions::native(actions::grip_position(&p, value))))
                    - glam::DVec3::from_array(wo);
            grips.push(crate::scene::model::object::GripDef {
                id: TEST_GRIP_START + index,
                world,
                is_midpoint: false,
                shape: crate::scene::model::object::GripShape::Triangle,
                dir: actions::grip_direction(&p, value).and_then(|direction| {
                    (actions::v(transform.apply(actions::native(direction)))
                        - actions::v(transform.apply(codec::types::Vector3::ZERO)))
                    .try_normalize()
                }),
                axis: None,
            });
        }
        for grip in grips {
            self.tabs[i].selected_grips.push(grip);
            self.tabs[i].selected_grip_handles.push(handle);
        }
    }
    fn start_reference_action_grip(&mut self, handle: Handle, id: usize) -> bool {
        let i = self.active_tab;
        let tab = &self.tabs[i];
        if tab.active_block_edit_session().is_some() || tab.selected_handle != Some(handle) {
            return false;
        }
        let Some(def) = actions::instance::definition(&tab.scene.document, handle) else {
            return false;
        };
        if tab.scene.is_layer_locked(handle) {
            return true;
        }
        let Some(p) = actions::all_parameters(&tab.scene.document, def)
            .get(id - TEST_GRIP_START)
            .cloned()
        else {
            return false;
        };
        if p.grip == Handle::NULL {
            return false;
        }
        if p.flip {
            let state = actions::instance::value(&tab.scene.document, handle, &p);
            let document = tab.id;
            return self.toggle_action_grip(document, def, p.handle, Some(handle), state);
        }
        let command = crate::modules::block_editor::action::TestParameterCommand {
            insert: Some(handle),
            document: tab.id,
            definition: def,
            parameter: p.handle,
        };
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        self.grip_hover = None;
        self.grip_popup = None;
        true
    }
    fn evaluate_reference_parameter(
        &mut self,
        handle: Handle,
        definition: Handle,
        parameter: Handle,
        value: actions::Value,
    ) -> Task<Message> {
        let i = self.active_tab;
        let tab = &self.tabs[i];
        if tab.active_block_edit_session().is_some()
            || tab.block_test.is_some()
            || actions::instance::definition(&tab.scene.document, handle) != Some(definition)
        {
            return Task::none();
        }
        if tab.scene.is_layer_locked(handle) {
            return Task::none();
        }
        let Some(EntityType::Insert(reference)) = tab.scene.document.get_entity(handle) else {
            return Task::none();
        };
        let Some(inverse) = crate::scene::inverse_affine(
            &crate::scene::render_graph::insert_transform(&tab.scene.document, reference),
        ) else {
            return Task::none();
        };
        let local = match value {
            actions::Value::Point(at) => {
                actions::Value::Point(actions::v(inverse.apply(actions::native(at))))
            }
            state => state,
        };
        match actions::instance::evaluate_value(&tab.scene.document, handle, parameter, local) {
            Ok(staged) => {
                if let Some(staged) = staged {
                    self.push_undo_snapshot(i, "Block reference parameter");
                    self.apply_action_document(i, staged);
                    self.tabs[i].scene.bump_geometry();
                    self.tabs[i].dirty = true;
                    self.finish_pending_history(i);
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].scene.clear_preview_wire();
                self.refresh_properties();
            }
            Err(e) => self.command_line.push_error(crate::t!(e).as_ref()),
        }
        Task::none()
    }
}

impl OpenCADStudio {
    fn toggle_action_grip(
        &mut self,
        document: u64,
        definition: Handle,
        parameter: Handle,
        insert: Option<Handle>,
        state: Result<actions::Value, &'static str>,
    ) -> bool {
        match state {
            Ok(actions::Value::Flip(state)) => {
                let _ = self.handle_block_action_result(
                    crate::command::CmdResult::EvaluateBlockParameter {
                        document,
                        definition,
                        parameter,
                        insert,
                        value: actions::Value::Flip(!state),
                    },
                );
            }
            Err(e) => self.command_line.push_error(crate::t!(e).as_ref()),
            _ => return false,
        }
        self.grip_hover = None;
        self.grip_popup = None;
        true
    }
}
