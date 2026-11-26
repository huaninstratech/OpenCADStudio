//! The `mobile` build's stand-in for [`super::ifc_write`].
//!
//! Exporting IFC builds the whole STEP entity graph, which is the same code
//! the importer shares. Keeping the two structures here means the model-import
//! screen compiles unchanged, and asking to export gives a clear answer rather
//! than an empty file.

#[derive(Clone, Debug)]
pub struct IfcExportElement {
    /// IFC GlobalId.
    pub guid: String,
    /// e.g. "IFCWALL", without the leading '#'.
    pub ifc_class: String,
    pub name: String,
    /// Building storey display name.
    pub storey: String,
    /// Flat triangle vertices in millimetres, 3 per triangle.
    pub verts: Vec<[f64; 3]>,
    /// (property set, property name, value) rows.
    pub props: Vec<(String, String, String)>,
}

#[derive(Clone, Debug)]
pub struct IfcExportModel {
    pub elements: Vec<IfcExportElement>,
}

pub fn write_ifc(_model: &IfcExportModel) -> Result<String, String> {
    Err("IFC export is not available in this build. Use the desktop app.".to_string())
}
