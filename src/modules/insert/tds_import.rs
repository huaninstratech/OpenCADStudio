// 3D Studio import: when the import is set to ask what a mesh with several
// materials becomes, this asks for each such mesh in turn.
//
//   <name> has multiple materials assigned. [Split/First/Material] <Split>:
//   Select a material or [None] <None>:

use std::collections::HashMap;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::io::tds_import::{LayerMode, MultiMaterial};

/// What one 3D Studio import reads and how.
#[derive(Clone, Debug, PartialEq)]
pub struct TdsImportRequest {
    pub path: String,
    /// The objects to import, by name ("Global Settings" included).
    pub selected: Vec<String>,
    pub layers: LayerMode,
    /// The choice for each mesh with several materials, by name.
    pub multi: HashMap<String, MultiMaterial>,
    /// Meshes keep only the vertices their faces use (the import splits or
    /// asks), else the file's vertex list.
    pub compact: bool,
}

pub struct TdsMaterialCommand {
    request: TdsImportRequest,
    /// The meshes still to ask about, with their materials.
    queue: Vec<(String, Vec<String>)>,
    choosing: bool,
}

impl TdsMaterialCommand {
    pub fn new(request: TdsImportRequest, queue: Vec<(String, Vec<String>)>) -> Self {
        Self {
            request,
            queue,
            choosing: false,
        }
    }

    fn answer(&mut self, choice: MultiMaterial) -> CmdResult {
        if self.queue.is_empty() {
            return CmdResult::Cancel;
        }
        let (name, _) = self.queue.remove(0);
        self.request.multi.insert(name, choice);
        self.choosing = false;
        if self.queue.is_empty() {
            CmdResult::TdsImport(self.request.clone())
        } else {
            CmdResult::NeedPoint
        }
    }
}

impl CadCommand for TdsMaterialCommand {
    fn name(&self) -> &'static str {
        "IMPORT"
    }

    fn prompt(&self) -> String {
        match (self.choosing, self.queue.first()) {
            (true, _) => "Select a material or [None] <None>:".to_string(),
            (false, Some((name, _))) => {
                format!("{name} has multiple materials assigned. [Split/First/Material] <Split>:")
            }
            (false, None) => String::new(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.choosing {
            let mut options = vec![CmdOption::new("None", "N")];
            if let Some((_, materials)) = self.queue.first() {
                options.extend(materials.iter().map(|m| CmdOption::new(m, m)));
            }
            options
        } else {
            vec![
                CmdOption::new("Split", "S"),
                CmdOption::new("First", "F"),
                CmdOption::new("Material", "M"),
            ]
        }
    }

    fn input_kind(&self) -> InputKind {
        if self.choosing {
            InputKind::FreeText
        } else {
            InputKind::SingleToken
        }
    }

    fn on_point(&mut self, _pt: glam::DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim();
        if self.choosing {
            if text.is_empty() || text.eq_ignore_ascii_case("None") || text.eq_ignore_ascii_case("N") {
                return Some(self.answer(MultiMaterial::Material(None)));
            }
            let found = self
                .queue
                .first()
                .and_then(|(_, materials)| materials.iter().find(|m| m.eq_ignore_ascii_case(text)))
                .cloned();
            return Some(match found {
                Some(material) => self.answer(MultiMaterial::Material(Some(material))),
                None => CmdResult::ReportError("Invalid option keyword.".to_string()),
            });
        }
        let t = text.to_ascii_uppercase();
        Some(match t.as_str() {
            "" => self.answer(MultiMaterial::Split),
            _ if "SPLIT".starts_with(&t) => self.answer(MultiMaterial::Split),
            _ if "FIRST".starts_with(&t) => self.answer(MultiMaterial::First),
            _ if "MATERIAL".starts_with(&t) => {
                self.choosing = true;
                CmdResult::NeedPoint
            }
            _ => CmdResult::ReportError("Invalid option keyword.".to_string()),
        })
    }

    fn on_enter(&mut self) -> CmdResult {
        self.on_text_input("").unwrap_or(CmdResult::NeedPoint)
    }
}
