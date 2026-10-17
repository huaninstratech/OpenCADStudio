use super::{Message, OpenCADStudio};
use crate::command::{CadCommand, CmdResult};
use crate::scene::block_authoring::edit::{self, Part};
use codec::Handle;
use glam::DVec3;
use iced::{Point, Rectangle, Task};

const EDIT_GRIP_START: usize = usize::MAX / 4;
pub(super) fn is_editor_grip(id: usize) -> bool {
    (EDIT_GRIP_START..EDIT_GRIP_START + 16).contains(&id)
}
struct EditCommand {
    document: u64,
    definition: Handle,
    handle: Handle,
    part: Part,
}
impl CadCommand for EditCommand {
    fn name(&self) -> &'static str {
        "BEDITGRIP"
    }
    fn prompt(&self) -> String {
        crate::t!("Block Editor  Specify the new authoring grip location (Escape cancels):")
            .into_owned()
    }
    fn on_point(&mut self, position: DVec3) -> CmdResult {
        CmdResult::EditBlockAuthoring {
            document: self.document,
            definition: self.definition,
            handle: self.handle,
            part: self.part,
            position,
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}
impl OpenCADStudio {
    pub(super) fn pick_authoring_object(&mut self, p: Point, bounds: Rectangle) -> bool {
        let i = self.active_tab;
        let tab = &self.tabs[i];
        if tab.active_cmd.is_some() || tab.active_grip.is_some() {
            return false;
        }
        let Some(session) = tab.active_block_edit_session() else {
            return false;
        };
        let items = edit::items(&tab.scene.document, session.br_handle);
        let camera = tab.scene.camera.borrow();
        let project = |v: codec::types::Vector3| {
            camera
                .project(DVec3::new(v.x, v.y, v.z), bounds)
                .map(|v| Point::new(v.x, v.y))
        };
        let segment = |a: Point, b: Point| {
            let ab = glam::Vec2::new(b.x - a.x, b.y - a.y);
            let ap = glam::Vec2::new(p.x - a.x, p.y - a.y);
            let t = (ap.dot(ab) / ab.length_squared().max(1e-12)).clamp(0., 1.);
            (ap - ab * t).length()
        };
        let hit = items
            .iter()
            .filter_map(|item| {
                let mut d = f32::INFINITY;
                for (_, v) in &item.grips {
                    if let Some(v) = project(*v) {
                        d = d.min(segment(v, v));
                    }
                }
                for [a, b] in &item.graphic.lines {
                    if let (Some(a), Some(b)) = (project(*a), project(*b)) {
                        d = d.min(segment(a, b));
                    }
                }
                for (v, label) in &item.graphic.labels {
                    if let Some(v) = project(*v) {
                        // Labels are drawn starting beside their anchor, in screen pixels.
                        let width = (label.chars().count() as f32 * 7.).max(20.);
                        if p.x >= v.x - 8. && p.x <= v.x + width + 12. && (p.y - v.y).abs() < 16. {
                            d = d.min(0.);
                        }
                    }
                }
                (d <= 10.).then_some((d, item.handle, item.name.clone()))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        drop(camera);
        if let Some((_, handle, name)) = hit {
            self.tabs[i].scene.deselect_all();
            self.tabs[i].selected_authoring = (!self.shift_down).then_some(handle);
            self.tabs[i]
                .scene
                .selection
                .borrow_mut()
                .clear_left_selection_gesture();
            self.refresh_properties();
            self.command_line.push_info(&format!(
                "{} — {}",
                name,
                crate::t!("Select a grip to edit its location.")
            ));
            true
        } else {
            if self.tabs[i].selected_authoring.take().is_some() {
                self.refresh_properties();
            }
            false
        }
    }
    pub(super) fn refresh_authoring_grips(&mut self, wo: [f64; 3]) {
        let i = self.active_tab;
        let Some(handle) = self.tabs[i].selected_authoring else {
            return;
        };
        let item = self.tabs[i].active_block_edit_session().and_then(|s| {
            edit::items(&self.tabs[i].scene.document, s.br_handle)
                .into_iter()
                .find(|p| p.handle == handle)
        });
        let Some(item) = item.filter(|_| self.tabs[i].scene.selected.is_empty()) else {
            self.tabs[i].selected_authoring = None;
            return;
        };
        for (index, (_, point)) in item.grips.into_iter().enumerate() {
            self.tabs[i]
                .selected_grips
                .push(crate::scene::model::object::GripDef {
                    id: EDIT_GRIP_START + index,
                    world: DVec3::new(point.x - wo[0], point.y - wo[1], point.z - wo[2]),
                    is_midpoint: false,
                    shape: crate::scene::model::object::GripShape::Square,
                    dir: None,
                    axis: None,
                });
            self.tabs[i].selected_grip_handles.push(handle);
        }
    }
    pub(super) fn start_authoring_grip(&mut self, handle: Handle, id: usize) -> bool {
        if !is_editor_grip(id) {
            return false;
        }
        let i = self.active_tab;
        let Some(session) = self.tabs[i].active_block_edit_session() else {
            return false;
        };
        let Some(part) = edit::items(&self.tabs[i].scene.document, session.br_handle)
            .into_iter()
            .find(|p| p.handle == handle)
            .and_then(|p| p.grips.get(id - EDIT_GRIP_START).map(|(part, _)| *part))
        else {
            return false;
        };
        let command = EditCommand {
            document: self.tabs[i].id,
            definition: session.br_handle,
            handle,
            part,
        };
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        self.grip_hover = None;
        self.grip_popup = None;
        true
    }
    pub(super) fn handle_authoring_edit(&mut self, result: CmdResult) -> Task<Message> {
        let CmdResult::EditBlockAuthoring {
            document,
            definition,
            handle,
            part,
            position,
        } = result
        else {
            unreachable!()
        };
        let i = self.active_tab;
        if self.tabs[i].id != document
            || self.tabs[i]
                .active_block_edit_session()
                .map(|s| s.br_handle)
                != Some(definition)
        {
            self.command_line
                .push_error(crate::t!("The Block Editor session is no longer active.").as_ref());
            return Task::none();
        }
        let mut staged = self.tabs[i].scene.document.clone();
        match edit::move_part(
            &mut staged,
            definition,
            handle,
            part,
            codec::types::Vector3::new(position.x, position.y, position.z),
        ) {
            Ok(changed) => {
                if changed {
                    self.push_undo_snapshot(i, "Block authoring grip");
                    // Keep the live entity-change recorder attached. Basepoint edits
                    // can also relocate INSERT attributes outside the edited block.
                    let doc = &mut self.tabs[i].scene.document;
                    let changed_entities = staged
                        .entities()
                        .filter(|e| doc.get_entity(e.common().handle) != Some(*e))
                        .cloned()
                        .collect::<Vec<_>>();
                    for entity in changed_entities {
                        let handle = entity.common().handle;
                        *doc.get_entity_mut(handle)
                            .expect("authoring edits retain entity identities") = entity;
                    }
                    for (h, o) in staged.objects.iter() {
                        if doc.objects.get(h) != Some(o) {
                            doc.objects.insert(*h, o.clone());
                        }
                    }
                    doc.block_visibility_params = staged.block_visibility_params.clone();
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
