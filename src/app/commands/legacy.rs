use super::*;

impl OpenCADStudio {
    /// PURGELEGACY — rebuild proxy/AEC entities as native geometry and strip
    /// vendor dictionaries, then offer to Save As a clean OpenCADStudio file.
    /// See [`crate::scene::legacy`] for the document-level work.
    pub(super) fn dispatch_legacy(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        match cmd {
            "PURGELEGACY" => {
                use crate::command::KeywordCommand;
                let c = KeywordCommand::new(
                    "PURGELEGACY",
                    "PURGELEGACY  [All / Report] <All>:",
                    vec![("All", "ALL", None), ("Report", "REPORT", None)],
                )
                .with_default("ALL");
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
                Some(Task::none())
            }
            cmd if cmd.starts_with("PURGELEGACY ") => {
                let sub = cmd
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("ALL")
                    .to_uppercase();
                let plan = crate::scene::legacy::plan(&self.tabs[i].scene.document);

                if sub == "REPORT" {
                    self.report_legacy_plan(&plan);
                    return Some(Task::none());
                }
                if sub != "ALL" {
                    self.command_line.push_error(
                        crate::tf!(
                            "PURGELEGACY: unknown option \"{sub}\" — use All or Report."
                        )
                        .as_ref(),
                    );
                    return Some(Task::none());
                }
                if plan.is_empty() {
                    self.command_line
                        .push_output(crate::t!("PURGELEGACY: no legacy objects found.").as_ref());
                    return Some(Task::none());
                }

                self.report_legacy_plan(&plan);
                self.push_undo_snapshot(i, "PURGELEGACY");
                let report = crate::scene::legacy::apply(&mut self.tabs[i].scene.document);
                self.tabs[i].dirty = true;
                self.tabs[i].scene.deselect_all();
                self.tabs[i].scene.rebuild_derived_caches();
                self.refresh_layer_panel();

                let converted = report.proxies_converted + report.unknowns_converted;
                self.command_line.push_output(
                    crate::tf!(
                        "PURGELEGACY: rebuilt {converted} legacy object(s) as {} native entit(y|ies).",
                        report.pieces_added
                    )
                    .as_ref(),
                );
                self.command_line.push_output(
                    crate::tf!(
                        "PURGELEGACY: removed {} object(s) without usable graphics, {} unreachable object(s), {} vendor APPID(s).",
                        report.no_graphics_removed + report.undecodable_removed,
                        report.objects_gced(),
                        report.app_ids_removed.len()
                    )
                    .as_ref(),
                );
                if report.repair_actions > 0 {
                    self.command_line.push_output(
                        crate::tf!(
                            "PURGELEGACY: {} repair action(s) applied.",
                            report.repair_actions
                        )
                        .as_ref(),
                    );
                }

                // Offer the cleaned drawing as a fresh file, keeping the
                // original untouched — same flow as the recovery save-as.
                if !self.read_only {
                    let task = self.open_save_dialog_window(i);
                    let (ext, _) = crate::io::parse_save_format("DWG 2018");
                    let stem = self.tabs[i]
                        .current_path
                        .as_ref()
                        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                        .unwrap_or_else(|| self.tabs[i].tab_display_name());
                    self.save_dialog_format = "DWG 2018".to_string();
                    self.save_dialog_filename = format!("{stem}_OCS.{ext}");
                    self.command_line.push_info(
                        crate::t!("PURGELEGACY: choose a destination to save the clean drawing.").as_ref(),
                    );
                    Some(task)
                } else {
                    self.command_line.push_error(
                        crate::t!("PURGELEGACY: read-only session — saving is disabled.").as_ref(),
                    );
                    Some(Task::none())
                }
            }
            _ => None,
        }
    }

    /// AUDIT-style listing, capped like the audit report so a SENIC-sized
    /// drawing doesn't flood the command line.
    fn report_legacy_plan(&mut self, plan: &crate::scene::legacy::LegacyPlan) {
        self.command_line.push_output(
            crate::tf!(
                "PURGELEGACY: {} proxy/AEC entit(y|ies) ({} with usable graphics), {} unknown entit(y|ies).",
                plan.proxy_entities,
                plan.with_graphics,
                plan.unknown_entities
            )
            .as_ref(),
        );
        if plan.vendor_nod_keys.is_empty() {
            self.command_line
                .push_output(crate::t!("PURGELEGACY: no vendor dictionaries.").as_ref());
        } else {
            self.command_line.push_output(
                crate::tf!(
                    "PURGELEGACY: {} vendor dictionary ent(ry|ies):",
                    plan.vendor_nod_keys.len()
                )
                .as_ref(),
            );
            for name in plan.vendor_nod_keys.iter().take(50) {
                self.command_line.push_info(&format!("  {name}"));
            }
            if plan.vendor_nod_keys.len() > 50 {
                self.command_line.push_info(&format!(
                    "  … {} more",
                    plan.vendor_nod_keys.len() - 50
                ));
            }
        }
    }
}
