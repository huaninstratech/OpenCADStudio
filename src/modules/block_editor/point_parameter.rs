//! Two-point authoring gesture: parameter position followed by label position.
use crate::command::{CadCommand, CmdResult};
use crate::scene::WireModel;
use codec::Handle;
use glam::DVec3;

pub struct PointParameterCommand {
    document_id: u64,
    definition: Handle,
    location: Option<DVec3>,
}

impl PointParameterCommand {
    pub fn new(document_id: u64, definition: Handle) -> Self {
        Self {
            document_id,
            definition,
            location: None,
        }
    }
}

impl CadCommand for PointParameterCommand {
    fn name(&self) -> &'static str {
        "BPARAMETER"
    }

    fn prompt(&self) -> String {
        crate::t!(if self.location.is_some() {
            "BPARAMETER  Specify point parameter label location:"
        } else {
            "BPARAMETER  Specify point parameter location:"
        })
        .into_owned()
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if !point.is_finite() {
            return CmdResult::ReportError(
                crate::t!("Specify a finite parameter location.").into_owned(),
            );
        }
        if let Some(location) = self.location {
            CmdResult::PlaceBlockPointParameter {
                document_id: self.document_id,
                definition: self.definition,
                location,
                label: point,
            }
        } else {
            self.location = Some(point);
            CmdResult::NeedPoint
        }
    }

    fn on_preview_wires(&mut self, point: DVec3) -> Vec<WireModel> {
        self.location
            .filter(|_| point.is_finite())
            .map(|location| {
                vec![WireModel::solid_f64(
                    "preview_point_parameter".into(),
                    vec![location.to_array(), point.to_array()],
                    WireModel::CYAN,
                    false,
                )]
            })
            .unwrap_or_default()
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}
