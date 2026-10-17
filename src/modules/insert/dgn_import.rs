// DGNIMPORT / -DGNIMPORT — a DGN model (V7 or V8) as drawing objects.
//
//   Enter filename for DGN import:
//   Enter name of model or [?] <Default>:
//   Specify conversion units [Master/Sub] <Master>:
//   Specify mapping setup or [?] <Standard>:
//
// The Standard mapping setup: each level becomes the layer of its name (its
// number when unnamed), colours become the colour index of the same RGB
// (else a true colour), weights the lineweights below, V7 line styles
// Continuous. Lines, line strings and shapes, arcs, circles and ellipses,
// complex chains and shapes (polylines with arcs), text, text nodes and
// shared cells (blocks) are converted.

use codec::entities::{
    Arc, Circle, Ellipse, Insert, Line, LwPolyline, LwVertex, MText, Text,
};
use codec::types::{Color, LineWeight, Vector2, Vector3};
use codec::EntityType;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::scene::model::underlay_vector::{DgnElement, DgnGeom, DgnImport};

/// What one import reads and where it goes.
#[derive(Clone, Debug, PartialEq)]
pub struct DgnImportRequest {
    pub path: String,
    pub model: String,
    /// Sub units (else master units) become drawing units.
    pub sub_units: bool,
    /// Text nodes become one TEXT per line (else one MTEXT).
    pub explode_text_nodes: bool,
    /// Into the current drawing (else a new one).
    pub current_drawing: bool,
    /// In the current drawing: layer, block and style names that already
    /// exist get the file name as prefix (else the existing ones are used).
    pub prefix_duplicates: bool,
}

/// The objects of an import: layers (name, colour), text styles (name, font
/// file), blocks (name, objects) and the model's objects, in file order.
#[derive(Default)]
pub struct Converted {
    pub layers: Vec<(String, Color)>,
    pub styles: Vec<(String, String)>,
    pub blocks: Vec<(String, Vec<EntityType>)>,
    pub entities: Vec<EntityType>,
}

/// A DGN weight (0-31) as a lineweight (1/100 mm), as the Standard mapping
/// converts it.
pub fn lineweight_of(weight: u32) -> i16 {
    const TABLE: [i16; 15] = [0, 13, 30, 40, 53, 70, 80, 100, 106, 120, 140, 158, 158, 158, 200];
    TABLE.get(weight as usize).copied().unwrap_or(211)
}

/// An RGB colour as the colour index of the same RGB, else a true colour.
pub fn color_of(rgb: [u8; 3]) -> Color {
    let index = codec::types::aci_table::nearest_aci(rgb[0], rgb[1], rgb[2]);
    if codec::types::aci_table::aci_to_rgb(index) == Some((rgb[0], rgb[1], rgb[2])) {
        Color::Index(index)
    } else {
        Color::from_rgb(rgb[0], rgb[1], rgb[2])
    }
}

/// A unit's short name as words ("m" → Meters).
pub fn unit_display(short: &str) -> String {
    match short.trim().to_ascii_lowercase().as_str() {
        "m" => "Meters",
        "mm" => "Millimeters",
        "cm" => "Centimeters",
        "km" => "Kilometers",
        "in" | "\"" => "Inches",
        "ft" | "'" => "Feet",
        "yd" => "Yards",
        "mi" => "Miles",
        other => return other.to_string(),
    }
    .to_string()
}

fn layer_name(import: &DgnImport, level: u32) -> String {
    import
        .levels
        .get(&level)
        .map(|l| l.0.clone())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| level.to_string())
}

/// True when the axes `u` and `v` are of one length and at right angles.
fn circular(u: [f64; 2], v: [f64; 2]) -> bool {
    let (lu, lv) = (u[0].hypot(u[1]), v[0].hypot(v[1]));
    (lu - lv).abs() <= 1e-9 * lu.max(1.0) && (u[0] * v[0] + u[1] * v[1]).abs() <= 1e-9 * lu * lv
}

/// The arc's points at `n` steps.
fn arc_points(center: [f64; 2], u: [f64; 2], v: [f64; 2], start: f64, sweep: f64) -> Vec<[f64; 2]> {
    let n = ((sweep.abs() / (std::f64::consts::PI / 32.0)).ceil() as usize).clamp(2, 4096);
    (0..=n)
        .map(|k| {
            let t = start + sweep * k as f64 / n as f64;
            [
                center[0] + t.cos() * u[0] + t.sin() * v[0],
                center[1] + t.cos() * u[1] + t.sin() * v[1],
            ]
        })
        .collect()
}

/// A circular arc as centre, radius, start angle and signed sweep in the
/// drawing's angles.
fn circle_angles(u: [f64; 2], v: [f64; 2], start: f64, sweep: f64) -> (f64, f64, f64) {
    let side = (u[0] * v[1] - u[1] * v[0]).signum();
    let a0 = u[1].atan2(u[0]);
    (u[0].hypot(u[1]), a0 + side * start, side * sweep)
}

fn polyline(points: &[[f64; 2]], bulges: &[f64]) -> EntityType {
    let mut pl = LwPolyline::new();
    pl.plinegen = true;
    pl.vertices = points
        .iter()
        .enumerate()
        .map(|(k, p)| {
            let mut v = LwVertex::new(Vector2::new(p[0], p[1]));
            v.bulge = bulges.get(k).copied().unwrap_or(0.0);
            v
        })
        .collect();
    EntityType::LwPolyline(pl)
}

/// A complex chain's vertices and bulges: lines and line strings add their
/// points, circular arcs a bulge, other arcs their points.
fn chain(parts: &[DgnGeom]) -> (Vec<[f64; 2]>, Vec<f64>) {
    let mut points: Vec<[f64; 2]> = Vec::new();
    let mut bulges: Vec<f64> = Vec::new();
    let mut add = |p: [f64; 2], bulge_before: Option<f64>| {
        let same = points
            .last()
            .is_some_and(|q| (q[0] - p[0]).abs() < 1e-9 && (q[1] - p[1]).abs() < 1e-9);
        if let (Some(b), Some(last)) = (bulge_before, bulges.last_mut()) {
            *last = b;
        }
        if !same {
            points.push(p);
            bulges.push(0.0);
        }
    };
    for part in parts {
        match part {
            DgnGeom::Line(a, b) => {
                add(*a, None);
                add(*b, None);
            }
            DgnGeom::Points(pts) => pts.iter().for_each(|p| add(*p, None)),
            DgnGeom::Arc {
                center,
                u,
                v,
                start,
                sweep,
            } if circular(*u, *v) => {
                let at = |t: f64| {
                    [
                        center[0] + t.cos() * u[0] + t.sin() * v[0],
                        center[1] + t.cos() * u[1] + t.sin() * v[1],
                    ]
                };
                let (_, _, signed) = circle_angles(*u, *v, *start, *sweep);
                add(at(*start), None);
                add(at(start + sweep), Some((signed / 4.0).tan()));
            }
            DgnGeom::Arc {
                center,
                u,
                v,
                start,
                sweep,
            } => arc_points(*center, *u, *v, *start, *sweep)
                .into_iter()
                .for_each(|p| add(p, None)),
            _ => {}
        }
    }
    (points, bulges)
}

fn text_entity(geom: &DgnGeom, style: &str) -> Option<EntityType> {
    let DgnGeom::Text {
        text,
        origin,
        height,
        width_factor,
        rotation,
        ..
    } = geom
    else {
        return None;
    };
    let mut t = Text::with_value(text.clone(), Vector3::new(origin[0], origin[1], 0.0))
        .with_height(*height)
        .with_rotation(*rotation);
    t.width_factor = if *width_factor > 0.0 { *width_factor } else { 1.0 };
    t.style = style.to_string();
    Some(EntityType::Text(t))
}

/// The text style of a line of text (V7: one per font number, "dgnNNN"),
/// added to `styles` with its font file the first time.
fn text_style(geom: &DgnGeom, styles: &mut Vec<(String, String)>) -> String {
    match geom {
        DgnGeom::Text {
            font: Some(font), ..
        } => {
            let name = format!("dgn{font:03}");
            if !styles.iter().any(|s| s.0 == name) {
                styles.push((name.clone(), format!("{name}.shx")));
            }
            name
        }
        _ => "Standard".to_string(),
    }
}

/// Lines of text as one MTEXT, a paragraph a line, as the reference
/// converts them: a text node attached at its top left (a height above the
/// first baseline), a V8 text at its bottom left a third of the height
/// below its baseline.
fn mtext(lines: &[DgnGeom], style: String, node: bool) -> Option<EntityType> {
    let Some(DgnGeom::Text {
        origin,
        height,
        rotation,
        ..
    }) = lines.first()
    else {
        return None;
    };
    let mut m = MText::new();
    m.value = lines
        .iter()
        .filter_map(|g| match g {
            DgnGeom::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\\P");
    let up = if node { *height } else { -0.33 * height };
    m.insertion_point = Vector3::new(
        origin[0] - rotation.sin() * up,
        origin[1] + rotation.cos() * up,
        0.0,
    );
    if !node {
        m.attachment_point = codec::entities::AttachmentPoint::BottomLeft;
        m.line_spacing_factor = 0.8;
    }
    m.height = *height;
    m.rotation = *rotation;
    m.rectangle_width = 0.0;
    m.style = style;
    Some(EntityType::MText(m))
}

/// The object(s) one element converts to.
fn convert_geom(
    geom: &DgnGeom,
    explode_text_nodes: bool,
    styles: &mut Vec<(String, String)>,
) -> Vec<EntityType> {
    match geom {
        DgnGeom::Line(a, b) => vec![EntityType::Line(Line::from_points(
            Vector3::new(a[0], a[1], 0.0),
            Vector3::new(b[0], b[1], 0.0),
        ))],
        DgnGeom::Points(pts) if pts.len() >= 2 => vec![polyline(pts, &[])],
        DgnGeom::Points(_) => Vec::new(),
        DgnGeom::Arc {
            center,
            u,
            v,
            start,
            sweep,
        } => {
            let c = Vector3::new(center[0], center[1], 0.0);
            let full = sweep.abs() >= std::f64::consts::TAU - 1e-9;
            if circular(*u, *v) {
                let (radius, a0, signed) = circle_angles(*u, *v, *start, *sweep);
                if full {
                    return vec![EntityType::Circle(Circle::from_center_radius(c, radius))];
                }
                let (from, to) = if signed >= 0.0 {
                    (a0, a0 + signed)
                } else {
                    (a0 + signed, a0)
                };
                return vec![EntityType::Arc(Arc::from_center_radius_angles(
                    c,
                    radius,
                    from.rem_euclid(std::f64::consts::TAU),
                    to.rem_euclid(std::f64::consts::TAU),
                ))];
            }
            let (lu, lv) = (u[0].hypot(u[1]), v[0].hypot(v[1]));
            if (u[0] * v[0] + u[1] * v[1]).abs() > 1e-9 * lu * lv || lu < 1e-12 || lv < 1e-12 {
                // ponytail: a skewed plan projection (a 3D ellipse seen
                // obliquely) becomes points; find its principal axes if
                // such files matter.
                return vec![polyline(&arc_points(*center, *u, *v, *start, *sweep), &[])];
            }
            // P(t) = c + cos t·u + sin t·v as an ellipse on its major axis
            // with the minor axis turned counter-clockwise from it.
            let ccw = u[0] * v[1] - u[1] * v[0] > 0.0;
            let (major, ratio, map): (_, f64, Box<dyn Fn(f64) -> f64>) = if lu >= lv {
                if ccw {
                    (*u, lv / lu, Box::new(|t| t))
                } else {
                    (*u, lv / lu, Box::new(|t: f64| -t))
                }
            } else if ccw {
                (*v, lu / lv, Box::new(|t: f64| t - std::f64::consts::FRAC_PI_2))
            } else {
                (*v, lu / lv, Box::new(|t: f64| std::f64::consts::FRAC_PI_2 - t))
            };
            let mut e = Ellipse::from_center_axes(c, Vector3::new(major[0], major[1], 0.0), ratio);
            if !full {
                let (p0, p1) = (map(*start), map(start + sweep));
                let (from, to) = if p1 >= p0 { (p0, p1) } else { (p1, p0) };
                e.start_parameter = from.rem_euclid(std::f64::consts::TAU);
                e.end_parameter = e.start_parameter + (to - from);
            }
            vec![EntityType::Ellipse(e)]
        }
        DgnGeom::Chain(parts) => {
            let (points, bulges) = chain(parts);
            if points.len() >= 2 {
                vec![polyline(&points, &bulges)]
            } else {
                Vec::new()
            }
        }
        // V7 text is TEXT, V8 text an MTEXT.
        DgnGeom::Text { font: Some(_), .. } => {
            let style = text_style(geom, styles);
            text_entity(geom, &style).into_iter().collect()
        }
        DgnGeom::Text { .. } => {
            let style = text_style(geom, styles);
            mtext(std::slice::from_ref(geom), style, false).into_iter().collect()
        }
        // Exploded, each line converts as a text element of its own.
        DgnGeom::TextNode(lines) if explode_text_nodes => lines
            .iter()
            .flat_map(|g| convert_geom(g, false, styles))
            .collect(),
        DgnGeom::TextNode(lines) => {
            let Some(first) = lines.first() else {
                return Vec::new();
            };
            let style = text_style(first, styles);
            mtext(lines, style, true).into_iter().collect()
        }
        DgnGeom::Cell {
            name,
            m,
            origin,
            z_scale,
        } => {
            let sx = m[0].hypot(m[2]);
            let det = m[0] * m[3] - m[1] * m[2];
            let sy = if sx > 0.0 { det / sx } else { 1.0 };
            let insert = Insert::new(name.clone(), Vector3::new(origin[0], origin[1], 0.0))
                .with_scale(sx, sy, *z_scale)
                .with_rotation(m[2].atan2(m[0]));
            vec![EntityType::Insert(insert)]
        }
    }
}

fn convert_elements(
    import: &DgnImport,
    elements: &[DgnElement],
    explode_text_nodes: bool,
    in_cell: bool,
    out: &mut Converted,
) -> Vec<EntityType> {
    let mut entities = Vec::new();
    for element in elements {
        // A cell's objects are on layer 0 (V7: by block), as the reference
        // defines the block.
        let layer = if in_cell {
            "0".to_string()
        } else {
            layer_name(import, element.level)
        };
        if !out.layers.iter().any(|l| l.0 == layer) {
            let color = import
                .levels
                .get(&element.level)
                .and_then(|l| l.1)
                .map(color_of)
                .unwrap_or(Color::Index(7));
            out.layers.push((layer.clone(), color));
        }
        for mut entity in convert_geom(&element.geom, explode_text_nodes, &mut out.styles) {
            let common = entity.common_mut();
            common.layer = layer.clone();
            common.color = element.color.map(color_of).unwrap_or(Color::ByLayer);
            common.line_weight = element
                .weight
                .map(|w| LineWeight::Value(lineweight_of(w)))
                .unwrap_or(LineWeight::ByLayer);
            // The Standard mapping draws every line style Continuous.
            if element.style.is_some() {
                common.linetype = "Continuous".to_string();
            }
            if import.v7 && in_cell {
                common.color = Color::ByBlock;
                common.line_weight = LineWeight::ByBlock;
            }
            entities.push(entity);
        }
    }
    entities
}

/// The import's objects, in sub units when `sub_units`.
pub fn convert(import: &DgnImport, sub_units: bool, explode_text_nodes: bool) -> Converted {
    let k = if sub_units { import.sub_per_master } else { 1.0 };
    let scale = |elements: &[DgnElement]| {
        crate::scene::model::underlay_vector::scaled_elements(elements, k, [0.0, 0.0])
    };
    let mut out = Converted::default();
    for (name, elements) in &import.cells {
        let entities = convert_elements(import, &scale(elements), explode_text_nodes, true, &mut out);
        if !name.is_empty() && !entities.is_empty() {
            out.blocks.push((name.clone(), entities));
        }
    }
    out.entities = convert_elements(import, &scale(&import.elements), explode_text_nodes, false, &mut out);
    out
}

/// The Standard mapping of what a file uses: level → layer, line style →
/// linetype, weight → lineweight, colour → colour, one row each.
pub fn mapping_rows(import: &DgnImport, tab: usize) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut add = |dgn: String, dwg: String| {
        if !rows.iter().any(|r| r.0 == dgn) {
            rows.push((dgn, dwg));
        }
    };
    let all = import
        .elements
        .iter()
        .chain(import.cells.iter().flat_map(|c| c.1.iter()));
    for e in all {
        match tab {
            0 => {
                let name = layer_name(import, e.level);
                add(name.clone(), name);
            }
            1 => match e.style {
                Some(s) => add(s.to_string(), "Continuous".to_string()),
                None => add("ByLevel".to_string(), "ByLayer".to_string()),
            },
            2 => match e.weight {
                Some(w) => add(
                    w.to_string(),
                    format!("{:.2} mm", lineweight_of(w) as f64 / 100.0),
                ),
                None => add("ByLevel".to_string(), "ByLayer".to_string()),
            },
            _ => match e.color {
                Some(rgb) => add(
                    format!("RGB {},{},{}", rgb[0], rgb[1], rgb[2]),
                    match color_of(rgb) {
                        Color::Index(i) => i.to_string(),
                        _ => format!("{},{},{}", rgb[0], rgb[1], rgb[2]),
                    },
                ),
                None => add("ByLevel".to_string(), "ByLayer".to_string()),
            },
        }
    }
    rows
}

// ── Command line ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Path,
    Model,
    ListModels,
    Conversion,
    Mapping,
    ListMappings,
}

pub struct DgnImportCommand {
    step: Step,
    path: String,
    models: Vec<String>,
    model: String,
    sub_units: bool,
}

impl Default for DgnImportCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl DgnImportCommand {
    pub fn new() -> Self {
        Self {
            step: Step::Path,
            path: String::new(),
            models: Vec::new(),
            model: String::new(),
            sub_units: false,
        }
    }

    fn accept_path(&mut self, text: &str) -> CmdResult {
        let path = text.trim().trim_matches('"').to_string();
        if path.is_empty() {
            return CmdResult::NeedPoint;
        }
        let models = crate::scene::model::underlay_vector::item_names(
            codec::entities::UnderlayType::Dgn,
            &path,
        )
        .filter(|m| !m.is_empty() && crate::scene::model::underlay_vector::dgn_import(&path, &m[0]).is_some());
        let Some(models) = models else {
            return CmdResult::ReportError("Invalid or unsupported DGN file.".to_string());
        };
        self.path = path;
        self.model = models[0].clone();
        self.models = models;
        self.step = Step::Model;
        CmdResult::NeedPoint
    }

    fn accept_model(&mut self, text: &str) -> CmdResult {
        let text = text.trim();
        if text == "?" {
            self.step = Step::ListModels;
            return CmdResult::NeedPoint;
        }
        let name = if text.is_empty() { self.models[0].as_str() } else { text };
        match self.models.iter().find(|m| m.eq_ignore_ascii_case(name)) {
            Some(found) => {
                self.model = found.clone();
                self.step = Step::Conversion;
                CmdResult::NeedPoint
            }
            None => CmdResult::ReportError(format!("There is no {name} in {}.", self.path)),
        }
    }

    fn list(&mut self, pattern: &str, names: &[&str], back: Step) -> CmdResult {
        let pattern = match pattern.trim() {
            "" => "*",
            p => p,
        };
        let lines: Vec<&str> = names
            .iter()
            .copied()
            .filter(|n| crate::io::xref_model::wildcard_match(n, pattern))
            .collect();
        self.step = back;
        CmdResult::ReportMeasurement(lines.join("\n"))
    }

    fn accept_conversion(&mut self, text: &str) -> CmdResult {
        let t = text.trim().to_ascii_uppercase();
        self.sub_units = match t.as_str() {
            "" => false,
            _ if "MASTER".starts_with(&t) => false,
            _ if "SUB".starts_with(&t) => true,
            _ => return CmdResult::ReportError("Invalid option keyword.".to_string()),
        };
        self.step = Step::Mapping;
        CmdResult::NeedPoint
    }

    fn accept_mapping(&mut self, text: &str) -> CmdResult {
        let text = text.trim();
        if text == "?" {
            self.step = Step::ListMappings;
            return CmdResult::NeedPoint;
        }
        if !text.is_empty() && !text.eq_ignore_ascii_case("Standard") {
            return CmdResult::ReportError(format!("{text} not found."));
        }
        CmdResult::DgnImport(DgnImportRequest {
            path: self.path.clone(),
            model: self.model.clone(),
            sub_units: self.sub_units,
            explode_text_nodes: false,
            current_drawing: false,
            prefix_duplicates: false,
        })
    }
}

impl CadCommand for DgnImportCommand {
    fn name(&self) -> &'static str {
        "DGNIMPORT"
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::Path => "Enter filename for DGN import:".to_string(),
            Step::Model => format!("Enter name of model or [?] <{}>:", self.models[0]),
            Step::ListModels => "Enter model(s) to list <*>:".to_string(),
            Step::Conversion => "Specify conversion units [Master/Sub] <Master>:".to_string(),
            Step::Mapping => "Specify mapping setup or [?] <Standard>:".to_string(),
            Step::ListMappings => "Enter mapping setup(s) to list <*>:".to_string(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Model | Step::Mapping => vec![CmdOption::new("?", "?")],
            Step::Conversion => vec![CmdOption::new("Master", "M"), CmdOption::new("Sub", "S")],
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Path | Step::Model => InputKind::FreeText,
            _ => InputKind::SingleToken,
        }
    }

    fn on_point(&mut self, _pt: glam::DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        Some(match self.step {
            Step::Path => self.accept_path(text),
            Step::Model => self.accept_model(text),
            Step::ListModels => {
                let models = self.models.clone();
                let names: Vec<&str> = models.iter().map(String::as_str).collect();
                self.list(text, &names, Step::Model)
            }
            Step::Conversion => self.accept_conversion(text),
            Step::Mapping => self.accept_mapping(text),
            Step::ListMappings => {
                match self.list(text, &["Standard"], Step::Mapping) {
                    CmdResult::ReportMeasurement(lines) => {
                        CmdResult::ReportMeasurement(format!("Import mapping setups:\n{lines}"))
                    }
                    other => other,
                }
            }
        })
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Path => CmdResult::Cancel,
            _ => self.on_text_input("").unwrap_or(CmdResult::NeedPoint),
        }
    }
}
