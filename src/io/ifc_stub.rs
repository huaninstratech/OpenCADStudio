//! The `mobile` build's stand-in for [`super::ifc`].
//!
//! IFC is a whole schema (2×3, 4, 4×3), a STEP physical-file reader and a mesh
//! tessellator; together they are the second largest thing in the wasm after
//! the geometry kernel. A phone opens 2D drawings, so the parser is left out
//! and this module keeps the types the rest of the tree names, so nothing else
//! needs a `cfg`. Every entry point that would parse says what is missing
//! instead of returning an empty model, which would look like a corrupt file.

use crate::scene::model::mesh_model::MeshModel;

/// One element of an imported model.
#[derive(Clone, Debug)]
pub struct IfcElement {
    pub guid: String,
    pub name: String,
    pub type_name: String,
    pub storey: String,
    /// (property set, property name, value)
    pub props: Vec<(String, String, String)>,
}

/// One imported mesh plus the GUID that links it to the element list.
#[derive(Clone, Debug)]
pub struct IfcMeshOut {
    pub mesh: MeshModel,
    pub guid: String,
    pub class: String,
    pub element_id: u64,
    pub params: Option<IfcExtrusionParams>,
    /// Feature edges computed on the worker thread.
    pub edges: (Vec<[f32; 3]>, Vec<[f32; 3]>),
}

/// Semantic shape parameters of an extrusion-based element, in millimetres.
#[derive(Clone, Debug)]
pub struct IfcExtrusionParams {
    pub world: glam::DMat4,
    pub dir_world: [f64; 3],
    pub depth: f64,
    pub profile: IfcProfile,
    pub outline: Option<Vec<Vec<[f64; 2]>>>,
}

/// Profile families whose dimensions are captured (and editable). The
/// variants mirror the real module so the shape editor still compiles; none
/// of them can be rebuilt without the extrusion pipeline.
#[derive(Clone, Debug)]
pub enum IfcProfile {
    Rectangle {
        x: f64,
        y: f64,
    },
    Circle {
        radius: f64,
    },
    CircleHollow {
        radius: f64,
        thickness: f64,
    },
    RectangleHollow {
        x: f64,
        y: f64,
        thickness: f64,
    },
    Trapezium {
        bottom: f64,
        top: f64,
        height: f64,
        offset: f64,
    },
    IShape {
        width: f64,
        depth: f64,
        web: f64,
        flange: f64,
    },
    LShape {
        depth: f64,
        width: f64,
        thickness: f64,
    },
    Points,
}

impl IfcProfile {
    /// Editable dimension rows (key, label, current mm value). Same table as
    /// the full build, so the properties panel looks the same; the values are
    /// simply not attached to anything that can rebuild.
    pub fn editable_rows(&self, rows: &mut Vec<(&'static str, &'static str, f64)>) {
        match self {
            Self::Rectangle { x, y } => {
                rows.push(("xdim", "Width", *x));
                rows.push(("ydim", "Height", *y));
            }
            Self::Circle { radius } => rows.push(("radius", "Radius", *radius)),
            Self::CircleHollow { radius, thickness } => {
                rows.push(("radius", "Radius", *radius));
                rows.push(("thickness", "Thickness", *thickness));
            }
            Self::RectangleHollow { x, y, thickness } => {
                rows.push(("xdim", "Width", *x));
                rows.push(("ydim", "Height", *y));
                rows.push(("thickness", "Thickness", *thickness));
            }
            Self::Trapezium {
                bottom,
                top,
                height,
                offset,
            } => {
                rows.push(("bottom", "Bottom", *bottom));
                rows.push(("top", "Top", *top));
                rows.push(("pheight", "Height", *height));
                rows.push(("poffset", "Offset", *offset));
            }
            Self::IShape {
                width,
                depth,
                web,
                flange,
            } => {
                rows.push(("iwidth", "Width", *width));
                rows.push(("idepth", "Depth", *depth));
                rows.push(("web", "Web", *web));
                rows.push(("flange", "Flange", *flange));
            }
            Self::LShape {
                depth,
                width,
                thickness,
            } => {
                rows.push(("ldepth", "Depth", *depth));
                rows.push(("lwidth", "Width", *width));
                rows.push(("thickness", "Thickness", *thickness));
            }
            Self::Points => {}
        }
    }
}

impl IfcExtrusionParams {
    /// All editable rows: extrusion depth plus the profile dimensions.
    pub fn editable_rows(&self) -> Vec<(&'static str, &'static str, f64)> {
        let mut rows = vec![("depth", "Extrusion depth", self.depth)];
        self.profile.editable_rows(&mut rows);
        rows
    }

    /// Apply one edit; unknown keys or invalid values return false. The value
    /// is stored so the panel behaves, but nothing can rebuild the mesh from
    /// it in this build.
    pub fn set(&mut self, key: &str, value: f64) -> bool {
        if key == "depth" {
            if value <= 0.0 {
                return false;
            }
            self.depth = value;
            return true;
        }
        self.profile.set(key, value)
    }
}

impl IfcProfile {
    /// Apply one edited dimension; returns false for unknown keys.
    pub fn set(&mut self, key: &str, value: f64) -> bool {
        let positive = value > 0.0;
        match (key, self) {
            ("xdim", IfcProfile::Rectangle { x, .. } | IfcProfile::RectangleHollow { x, .. })
                if positive =>
            {
                *x = value
            }
            ("ydim", IfcProfile::Rectangle { y, .. } | IfcProfile::RectangleHollow { y, .. })
                if positive =>
            {
                *y = value
            }
            ("radius", IfcProfile::Circle { radius } | IfcProfile::CircleHollow { radius, .. })
                if positive =>
            {
                *radius = value
            }
            ("thickness", IfcProfile::CircleHollow { radius, thickness })
                if positive && value < *radius =>
            {
                *thickness = value
            }
            ("thickness", IfcProfile::RectangleHollow { x, y, thickness })
                if positive && value < x.min(*y) =>
            {
                *thickness = value
            }
            ("bottom", IfcProfile::Trapezium { bottom, .. }) if positive => *bottom = value,
            ("top", IfcProfile::Trapezium { top, .. }) if positive => *top = value,
            ("pheight", IfcProfile::Trapezium { height, .. }) if positive => *height = value,
            ("poffset", IfcProfile::Trapezium { offset, .. }) => *offset = value,
            ("iwidth", IfcProfile::IShape { width, .. }) if positive => *width = value,
            ("idepth", IfcProfile::IShape { depth, .. }) if positive => *depth = value,
            ("web", IfcProfile::IShape { web, .. }) if positive => *web = value,
            ("flange", IfcProfile::IShape { flange, .. }) if positive => *flange = value,
            ("ldepth", IfcProfile::LShape { depth, .. }) if positive => *depth = value,
            ("lwidth", IfcProfile::LShape { width, .. }) if positive => *width = value,
            ("thickness", IfcProfile::LShape { thickness, .. }) if positive => *thickness = value,
            _ => return false,
        }
        true
    }
}

/// Per-element semantic record.
#[derive(Clone, Debug)]
pub struct IfcElementRecord {
    pub guid: String,
    pub class: String,
    pub name: String,
    pub storey: String,
    pub parent_guid: Option<String>,
    pub params: Option<IfcExtrusionParams>,
}

/// Node of the spatial model tree.
#[derive(Clone, Debug)]
pub struct IfcTreeNode {
    pub label: String,
    pub children: Vec<IfcTreeNode>,
    pub leaf_guid: Option<String>,
}

/// Result of parsing an IFC file.
#[derive(Clone, Debug)]
pub struct IfcImportResult {
    pub schema: String,
    pub meshes: Vec<IfcMeshOut>,
    pub elements: Vec<IfcElement>,
    pub warnings: Vec<String>,
    pub records: Vec<IfcElementRecord>,
    pub tree: Vec<IfcTreeNode>,
}

/// The message every entry point gives.
const UNAVAILABLE: &str =
    "IFC import is not available in this build. Open the model in the desktop app.";

impl IfcImportResult {
    /// Kept with the real signature: the import path calls it, and on a phone
    /// it fails with the explanation above.
    pub fn parse_ifc_with_progress(
        _bytes: &[u8],
        _progress: Option<&crate::io::OpenProgressState>,
    ) -> Result<Self, String> {
        Err(UNAVAILABLE.to_string())
    }
}

pub fn parse_ifc(_bytes: &[u8]) -> Result<IfcImportResult, String> {
    Err(UNAVAILABLE.to_string())
}

pub fn properties_csv_string(_bytes: &[u8]) -> Result<String, String> {
    Err(UNAVAILABLE.to_string())
}

/// Parametric rebuild needs the extrusion pipeline, so it cannot run here.
pub fn rebuild_extrusion(
    _params: &IfcExtrusionParams,
    _name: &str,
    _color: [f32; 4],
    _scale: f64,
) -> Option<MeshModel> {
    None
}
