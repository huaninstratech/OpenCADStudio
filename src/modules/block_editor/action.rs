//! Reuse the command host's point picking and object-set selection.
use crate::command::{CadCommand, CmdOption, CmdResult, PointPickContext};
use crate::scene::block_actions::{Kind, Parameter};
use codec::Handle;
use glam::DVec3;

inventory::submit!(crate::command::CommandRegistration {
    names: &["BACTION", "BACTIONTOOL"]
});

pub(crate) struct ActionCommand {
    document: u64,
    definition: Handle,
    kind: Option<Kind>,
    candidates: Vec<Parameter>,
    parameter: Option<Handle>,
    targets: Vec<Handle>,
    selecting: bool,
    context: Option<PointPickContext>,
}
impl ActionCommand {
    pub(crate) fn new(
        document: u64,
        definition: Handle,
        kind: Option<Kind>,
        candidates: Vec<Parameter>,
    ) -> Self {
        Self {
            document,
            definition,
            kind,
            candidates,
            parameter: None,
            targets: vec![],
            selecting: false,
            context: None,
        }
    }
    fn choose(&mut self, p: Parameter) -> CmdResult {
        if self.kind == Some(Kind::Scale) && (p.point || p.xy) {
            return CmdResult::ReportError(
                crate::t!("Scale requires a Linear or Polar parameter.").into_owned(),
            );
        }
        self.parameter = Some(p.handle);
        self.selecting = true;
        CmdResult::DeselectAndContinue
    }
}
impl CadCommand for ActionCommand {
    fn name(&self) -> &'static str {
        "BACTION"
    }
    fn wants_text_input(&self) -> bool {
        self.parameter.is_none()
    }
    fn prompt(&self) -> String {
        if self.kind.is_none() {
            return crate::t!("BACTION  Enter action type [Move/Scale]:").into_owned();
        }
        if self.parameter.is_none() {
            return crate::tf!(
                "BACTION {}  Select a parameter endpoint or enter its name:",
                crate::t!(self.kind.unwrap().name())
            )
            .into_owned();
        }
        crate::t!(if self.selecting {
            "BACTION  Select objects for the action, then press Enter:"
        } else {
            "BACTION  Specify action marker location:"
        })
        .into_owned()
    }
    fn options(&self) -> Vec<CmdOption> {
        if self.kind.is_none() {
            vec![
                CmdOption::new("Move", "Move"),
                CmdOption::new("Scale", "Scale"),
            ]
        } else {
            vec![]
        }
    }
    fn on_text_input(&mut self, s: &str) -> Option<CmdResult> {
        if self.kind.is_none() {
            return Some(if let Some(k) = Kind::parse(s) {
                self.kind = Some(k);
                CmdResult::NeedPoint
            } else {
                CmdResult::ReportError(crate::t!("Choose Move or Scale.").into_owned())
            });
        }
        if self.parameter.is_none() {
            let matches = self
                .candidates
                .iter()
                .filter(|p| p.name.eq_ignore_ascii_case(s))
                .cloned()
                .collect::<Vec<_>>();
            return Some(if matches.len() == 1 {
                self.choose(matches[0].clone())
            } else {
                CmdResult::ReportError(
                    crate::t!("Enter a unique compatible parameter name or pick its endpoint.")
                        .into_owned(),
                )
            });
        }
        None
    }
    fn wants_point_pick_context(&self) -> bool {
        self.kind.is_some() && self.parameter.is_none()
    }
    fn set_point_pick_context(&mut self, c: Option<PointPickContext>) {
        self.context = c;
    }
    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if self.parameter.is_none() {
            let Some(c) = self.context else {
                return CmdResult::ReportError(crate::t!("Enter the parameter name.").into_owned());
            };
            let screen =
                |p| crate::scene::pick::hit_test::world_to_screen(p, c.view, c.eye, c.bounds);
            let click = screen(point);
            let candidate = self
                .candidates
                .iter()
                .filter(|p| self.kind != Some(Kind::Scale) || (!p.point && !p.xy))
                .map(|p| (screen(p.end).distance(click), p))
                .filter(|(d, _)| d.is_finite() && *d <= c.aperture_px.max(10.))
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, p)| p.clone());
            return candidate.map(|p| self.choose(p)).unwrap_or_else(|| {
                CmdResult::ReportError(
                    crate::t!("Pick a compatible parameter endpoint.").into_owned(),
                )
            });
        }
        if self.selecting {
            return CmdResult::NeedPoint;
        }
        if !point.is_finite() {
            return CmdResult::ReportError(
                crate::t!("Specify a finite action location.").into_owned(),
            );
        }
        CmdResult::PlaceBlockAction {
            document: self.document,
            definition: self.definition,
            kind: self.kind.unwrap(),
            parameter: self.parameter.unwrap(),
            targets: self.targets.clone(),
            location: point,
        }
    }
    fn is_selection_gathering(&self) -> bool {
        self.selecting
    }
    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        // The host calls this after every pick/window/removal, not just Enter.
        // Retain the current set while the user gathers additional objects.
        self.targets = handles;
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        if self.selecting {
            if self.targets.is_empty() {
                return CmdResult::ReportError(
                    crate::t!("Select at least one object for the action.").into_owned(),
                );
            }
            self.selecting = false;
            return CmdResult::NeedPoint;
        }
        CmdResult::Cancel
    }
}

pub(crate) struct TestParameterCommand {
    pub document: u64,
    pub definition: Handle,
    pub parameter: Handle,
}
impl CadCommand for TestParameterCommand {
    fn name(&self) -> &'static str {
        "BTESTBLOCK"
    }
    fn prompt(&self) -> String {
        crate::t!("Test Block  Specify the new parameter endpoint (Escape cancels):").into_owned()
    }
    fn on_point(&mut self, position: DVec3) -> CmdResult {
        CmdResult::EvaluateBlockParameter {
            document: self.document,
            definition: self.definition,
            parameter: self.parameter,
            position,
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}
