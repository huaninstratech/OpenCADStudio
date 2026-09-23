use crate::modules::{IconKind, ModuleEvent, ToolDef};
pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/data_extract.svg"));
pub fn tool() -> ToolDef {
    ToolDef {
        id: "IFCDATA",
        label: "IFC\nData",
        icon: ICON,
        event: ModuleEvent::Command("IFCDATA".to_string()),
    }
}
