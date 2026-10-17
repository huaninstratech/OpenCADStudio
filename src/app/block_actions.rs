use super::{Message, OpenCADStudio};
use crate::command::CadCommand;
use crate::scene::block_actions::{self as actions, Kind};
use codec::{EntityType, Handle};
use iced::Task;

// These grips start an ordinary point-input command, keeping snapping, typed
// coordinates and Escape on the command driver's existing path.
const TEST_GRIP_START: usize = usize::MAX / 2;
pub(super) fn is_test_grip(id: usize) -> bool {
    (TEST_GRIP_START..usize::MAX).contains(&id)
}
impl OpenCADStudio {
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
        if actions::parameters(&tab.scene.document, s.br_handle, kind.unwrap_or(Kind::Move))
            .is_empty()
        {
            return Some("Place a compatible parameter before adding this action.");
        }
        None
    }
    pub(in crate::app) fn start_block_action(&mut self, args: &str) -> Task<Message> {
        let kind = if args.is_empty() {
            None
        } else {
            Kind::parse(args)
        };
        if !args.is_empty() && kind.is_none() {
            self.command_line.push_error(
                crate::t!("Supported actions: Move, Scale. Other action types are not wired yet.")
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
            actions::parameters(&tab.scene.document, definition, kind.unwrap_or(Kind::Move)),
        );
        self.command_line.push_info(&command.prompt());
        self.command_line.set_step_options(command.options());
        self.tabs[self.active_tab].active_cmd = Some(Box::new(command));
        Task::none()
    }
    pub(in crate::app) fn handle_block_action_result(
        &mut self,
        result: crate::command::CmdResult,
    ) -> Task<Message> {
        let i = self.active_tab;
        match result {
            crate::command::CmdResult::PlaceBlockAction {
                document,
                definition,
                kind,
                parameter,
                targets,
                location,
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
                match actions::create(&mut staged, definition, kind, parameter, &targets, location)
                {
                    Ok(_) => {
                        self.push_undo_snapshot(i, &format!("{} action", kind.name()));
                        self.tabs[i].scene.document = staged;
                        self.tabs[i].scene.bump_geometry();
                        self.tabs[i].dirty = true;
                        self.tabs[i].active_cmd = None;
                        self.tabs[i].scene.clear_preview_wire();
                        self.finish_pending_history(i);
                        self.refresh_properties();
                        self.command_line.push_output(crate::tf!("{} action attached. Open Test Block and click the parameter grip to test it.",crate::t!(kind.name())).as_ref());
                    }
                    Err(e) => self.command_line.push_error(crate::t!(e).as_ref()),
                }
            }
            crate::command::CmdResult::EvaluateBlockParameter {
                document,
                definition,
                parameter,
                position,
            } => {
                if self.tabs[i].id != document {
                    return Task::none();
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
                let local = actions::v(inverse.apply(actions::native(position)));
                let representation = insert.block_name.clone();
                let mut staged = self.tabs[i].scene.document.clone();
                match actions::evaluate(&mut staged, &test.baseline, definition, parameter, local) {
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
        for (index, p) in actions::parameters(&test.baseline, test.definition, Kind::Move)
            .into_iter()
            .enumerate()
        {
            if !all.iter().any(|(_, _, _, id)| *id == p.id) {
                continue;
            }
            let world = actions::v(
                transform.apply(actions::native(actions::current(&tab.scene.document, &p))),
            ) - glam::DVec3::from_array(wo);
            grips.push(crate::scene::model::object::GripDef {
                id: TEST_GRIP_START + index,
                world,
                is_midpoint: false,
                shape: crate::scene::model::object::GripShape::Triangle,
                dir: None,
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
            return false;
        };
        if handle != test.insert {
            return false;
        }
        if tab.scene.is_layer_locked(handle) {
            return true;
        }
        let Some(p) = actions::parameters(&test.baseline, test.definition, Kind::Move)
            .get(id - TEST_GRIP_START)
            .cloned()
        else {
            return false;
        };
        let command = crate::modules::block_editor::action::TestParameterCommand {
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
