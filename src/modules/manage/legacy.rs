use crate::modules::{IconKind, ModuleEvent, ToolDef};
pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/purge_legacy.svg"));
pub fn tool() -> ToolDef {
    ToolDef {
        id: "PURGELEGACY",
        label: "Purge\nLegacy",
        icon: ICON,
        event: ModuleEvent::Command("PURGELEGACY".to_string()),
    }
}
