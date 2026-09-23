use crate::modules::{IconKind, ModuleEvent, ToolDef};
pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/step.svg"));
pub fn tool() -> ToolDef {
    ToolDef {
        id: "IMPORTSTEP",
        label: "STEP",
        icon: ICON,
        event: ModuleEvent::Command("IMPORTSTEP".to_string()),
    }
}
