use super::{Message, OpenCADStudio};
use crate::command::{CadCommand, CmdResult};
use iced::Task;

impl OpenCADStudio {
    pub(in crate::app) fn start_block_grip_set(&mut self) -> Task<Message> {
        let tab = &self.tabs[self.active_tab];
        let Some(session) = tab.active_block_edit_session() else {
            self.command_line
                .push_error(crate::t!("BGRIPSET is only available in the Block Editor.").as_ref());
            return Task::none();
        };
        match crate::modules::block_editor::grip_set::GripSetCommand::new(
            tab.id,
            session.br_handle,
            &tab.scene.document,
            tab.selected_authoring,
        ) {
            Ok(command) => {
                self.command_line.push_info(&command.prompt());
                self.command_line.set_step_options(command.options());
                self.tabs[self.active_tab].active_cmd = Some(Box::new(command));
            }
            Err(error) => self.command_line.push_error(crate::t!(error).as_ref()),
        }
        Task::none()
    }
    pub(super) fn commit_block_grip_count(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let tab = &self.tabs[i];
        let Some(session) = tab.active_block_edit_session() else {
            return Task::none();
        };
        let Some(parameter) = tab.selected_authoring else {
            return Task::none();
        };
        let definition = session.br_handle;
        let key = crate::ui::properties::FieldKey::Geom("authoring_grip_count");
        let Some(input) = tab.properties.edit_buf.get(&key) else {
            return Task::none();
        };
        let Ok(count) = input.trim().parse::<usize>() else {
            self.command_line
                .push_error(crate::t!("Enter an available whole-number grip count.").as_ref());
            return Task::none();
        };
        let Ok(expected) =
            crate::scene::block_grips::config::snapshot(&tab.scene.document, definition)
        else {
            return Task::none();
        };
        self.set_block_parameter_grips(CmdResult::SetBlockParameterGrips {
            document: tab.id,
            definition,
            parameter,
            expected,
            count: Some(count),
        })
    }
    pub(super) fn set_block_parameter_grips(&mut self, result: CmdResult) -> Task<Message> {
        let CmdResult::SetBlockParameterGrips {
            document,
            definition,
            parameter,
            expected,
            count,
        } = result
        else {
            unreachable!()
        };
        let i = self.active_tab;
        let tab = &self.tabs[i];
        if tab.id != document
            || tab.active_block_edit_session().map(|s| s.br_handle) != Some(definition)
            || expected
                .iter()
                .any(|(h, object)| tab.scene.document.objects.get(h) != Some(object))
        {
            self.command_line.push_error(
                crate::t!("The parameter or editor session changed; restart BGRIPSET.").as_ref(),
            );
            return Task::none();
        }
        let mut staged = tab.scene.document.clone();
        match crate::scene::block_grips::config::set(&mut staged, definition, parameter, count) {
            Ok(changed) => {
                if changed {
                    self.push_undo_snapshot(i, "Configure block parameter grips");
                    self.apply_action_document(i, staged);
                    self.tabs[i].dirty = true;
                    self.tabs[i].scene.bump_geometry();
                    self.finish_pending_history(i);
                }
                if self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|command| command.name() == "BGRIPSET")
                {
                    self.tabs[i].active_cmd = None;
                }
                self.tabs[i]
                    .properties
                    .edit_buf
                    .remove(&crate::ui::properties::FieldKey::Geom(
                        "authoring_grip_count",
                    ));
                self.refresh_properties();
            }
            Err(error) => self.command_line.push_error(crate::t!(error).as_ref()),
        }
        Task::none()
    }
}
