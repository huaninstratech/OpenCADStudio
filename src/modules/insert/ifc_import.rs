use crate::modules::{IconKind, ModuleEvent, ToolDef};
pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/ifc.svg"));
pub fn tool() -> ToolDef {
    ToolDef {
        id: "IMPORTIFC",
        label: "IFC",
        icon: ICON,
        event: ModuleEvent::Command("IMPORTIFC".to_string()),
    }
}
