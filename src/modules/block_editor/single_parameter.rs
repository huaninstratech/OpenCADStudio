use crate::command::{CadCommand, CmdResult};
use codec::Handle;
use glam::DVec3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Basepoint,
    Lookup,
}
impl Kind {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_uppercase().as_str() {
            "BASE" | "BASEPOINT" | "B" => Some(Self::Basepoint),
            "LOOKUP" | "K" => Some(Self::Lookup),
            _ => None,
        }
    }
    pub(crate) fn name(self) -> &'static str {
        if self == Self::Basepoint {
            "Basepoint"
        } else {
            "Lookup"
        }
    }
}
inventory::submit!(crate::command::CommandRegistration {
    names: &["BLOOKUPTABLE"]
});

pub struct SingleParameterCommand {
    document: u64,
    definition: Handle,
    kind: Kind,
}
impl SingleParameterCommand {
    pub(crate) fn new(document: u64, definition: Handle, kind: Kind) -> Self {
        Self {
            document,
            definition,
            kind,
        }
    }
}
impl CadCommand for SingleParameterCommand {
    fn name(&self) -> &'static str {
        "BPARAMETER"
    }
    fn prompt(&self) -> String {
        crate::tf!(
            "BPARAMETER {}  Specify parameter location:",
            crate::t!(self.kind.name())
        )
        .into_owned()
    }
    fn on_point(&mut self, location: DVec3) -> CmdResult {
        if !location.is_finite() {
            return CmdResult::ReportError(
                crate::t!("Specify a finite parameter location.").into_owned(),
            );
        }
        CmdResult::PlaceBlockSingleParameter {
            document_id: self.document,
            definition: self.definition,
            kind: self.kind,
            location,
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}
