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
// B-spline curves, complex chains and shapes (polylines with arcs), text,
// text nodes and shared cells (blocks) are converted.

use std::collections::{HashMap, HashSet};

use codec::entities::{
    Arc, AttachmentPoint, Circle, DrawingDirection, Ellipse, Insert, Line, LineSpacingStyle,
    LwPolyline, LwVertex, MText, Point, Polyline3D, Spline, Text, TextHorizontalAlignment,
    TextVerticalAlignment,
};
use codec::types::{Color, LineWeight, Vector2, Vector3};
use codec::EntityType;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::scene::model::underlay_vector::{By, DgnElement, DgnGeom, DgnImport, P3};

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
    /// In the current drawing: every imported layer, block and text style
    /// name gets "<file> - <model>$0$" as prefix (else names the drawing
    /// already has keep the existing definitions).
    pub prefix_duplicates: bool,
    /// What becomes of the model's reference attachments.
    pub references: ReferenceMode,
    /// What an earlier translation of a reference already on disk gets.
    pub existing_reference: ExistingReference,
}

/// The External DGN references choice: translated into drawings attached
/// as external references, left out, or attached as DGN underlays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ReferenceMode {
    Translate,
    #[default]
    Ignore,
    Underlay,
}

/// A translated reference drawing that already exists: asked about,
/// overwritten, or kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ExistingReference {
    #[default]
    Prompt,
    Overwrite,
    Keep,
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

/// A name read from a DGN file as a symbol (layer, block, text style) or
/// file name: the characters those cannot hold become `_`.
pub fn symbol_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_control() || "<>/\\\":;?*|=`".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect()
}

fn layer_name(import: &DgnImport, level: u32) -> String {
    import
        .levels
        .get(&level)
        .map(|l| symbol_name(&l.0))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| level.to_string())
}

/// A cell's name as a block name: a leading `*` (an anonymous block
/// written out) is dropped.
pub fn block_name(cell: &str) -> String {
    symbol_name(cell.trim_start_matches('*'))
}

fn v3(p: P3) -> Vector3 {
    Vector3::new(p[0], p[1], p[2])
}

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: P3, b: P3) -> P3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: P3) -> f64 {
    dot(a, a).sqrt()
}

/// True when the axes `u` and `v` are of one length and at right angles.
fn circular(u: P3, v: P3) -> bool {
    let (lu, lv) = (norm(u), norm(v));
    (lu - lv).abs() <= 1e-9 * lu.max(1.0) && dot(u, v).abs() <= 1e-9 * lu * lv
}

/// The arc's points at `n` steps.
fn arc_points(center: P3, u: P3, v: P3, start: f64, sweep: f64) -> Vec<P3> {
    let n = ((sweep.abs() / (std::f64::consts::PI / 32.0)).ceil() as usize).clamp(2, 4096);
    (0..=n)
        .map(|k| {
            let t = start + sweep * k as f64 / n as f64;
            std::array::from_fn(|c| center[c] + t.cos() * u[c] + t.sin() * v[c])
        })
        .collect()
}

/// The plane of an arc: its normal (+z for an arc in the drawing plane,
/// whichever way it turns) and whether its parameter turns clockwise about
/// that normal.
fn arc_plane(u: P3, v: P3) -> (P3, bool) {
    let n = cross(u, v);
    let len = norm(n);
    if len < 1e-12 {
        return ([0.0, 0.0, 1.0], false);
    }
    let n = n.map(|c| c / len);
    if n[2] < -1.0 + 1e-9 {
        ([0.0, 0.0, 1.0], true)
    } else {
        (n, false)
    }
}

/// The angle of `d` in the entity coordinate system of `normal`.
fn ocs_angle(normal: P3, d: P3) -> f64 {
    let m = codec::types::Matrix3::arbitrary_axis(v3(normal));
    let (x, y) = (
        m * Vector3::new(1.0, 0.0, 0.0),
        m * Vector3::new(0.0, 1.0, 0.0),
    );
    (d[0] * y.x + d[1] * y.y + d[2] * y.z).atan2(d[0] * x.x + d[1] * x.y + d[2] * x.z)
}

/// A point in the entity coordinate system of `normal`.
fn to_ocs(normal: P3, p: P3) -> Vector3 {
    codec::types::Matrix3::arbitrary_axis(v3(normal)).transpose() * v3(p)
}

/// True when every point is at the first one's z.
fn flat(points: &[P3]) -> bool {
    let z = points.first().map_or(0.0, |p| p[2]);
    points.iter().all(|p| (p[2] - z).abs() <= 1e-9 * z.abs().max(1.0))
}

/// The points as a polyline: at one z a 2D polyline at that elevation
/// (with the bulges), else a 3D polyline (no bulges).
fn polyline(points: &[P3], bulges: &[f64]) -> EntityType {
    if !flat(points) {
        let points: Vec<Vector3> = points.iter().map(|p| v3(*p)).collect();
        return EntityType::Polyline3D(Polyline3D::from_points(points));
    }
    let mut pl = LwPolyline::new();
    pl.plinegen = true;
    pl.elevation = points.first().map_or(0.0, |p| p[2]);
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

/// A solid hatch filling a closed outline (with its bulges) at the
/// outline's elevation; `None` when the outline is not at one z.
fn solid_hatch(points: &[P3], bulges: &[f64]) -> Option<EntityType> {
    use codec::entities::{BoundaryEdge, BoundaryPath, Hatch, PolylineEdge};
    if points.len() < 3 || !flat(points) {
        return None;
    }
    let mut n = points.len();
    if points[0][0] == points[n - 1][0] && points[0][1] == points[n - 1][1] {
        n -= 1;
    }
    let mut edge = PolylineEdge::new(Vec::new(), true);
    for (k, p) in points[..n].iter().enumerate() {
        let bulge = bulges.get(k).copied().unwrap_or(0.0);
        edge.add_vertex(Vector2::new(p[0], p[1]), bulge);
    }
    let mut hatch = Hatch::solid();
    let mut boundary = BoundaryPath::external();
    boundary.flags.set_polyline(true);
    boundary.edges.push(BoundaryEdge::Polyline(edge));
    hatch.add_path(boundary);
    hatch.elevation = points[0][2];
    Some(EntityType::Hatch(Box::new(hatch)))
}

/// A complex chain's vertices and bulges: lines and line strings add their
/// points, circular arcs in the drawing plane a bulge, other arcs their
/// points. A chain whose vertices are not at one z comes out as points
/// only (a 3D polyline has no bulges).
fn chain(parts: &[DgnGeom]) -> (Vec<P3>, Vec<f64>) {
    let (points, bulges) = chain_points(parts, true);
    if flat(&points) {
        (points, bulges)
    } else {
        chain_points(parts, false)
    }
}

fn chain_points(parts: &[DgnGeom], use_bulges: bool) -> (Vec<P3>, Vec<f64>) {
    let mut points: Vec<P3> = Vec::new();
    let mut bulges: Vec<f64> = Vec::new();
    let mut add = |p: P3, bulge_before: Option<f64>| {
        let same = points
            .last()
            .is_some_and(|q| (0..3).all(|c| (q[c] - p[c]).abs() < 1e-9));
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
            } if use_bulges && circular(*u, *v) && arc_plane(*u, *v).0[2] > 1.0 - 1e-9 => {
                let at = |t: f64| -> P3 {
                    std::array::from_fn(|c| center[c] + t.cos() * u[c] + t.sin() * v[c])
                };
                let side = if arc_plane(*u, *v).1 { -1.0 } else { 1.0 };
                add(at(*start), None);
                add(at(start + sweep), Some((side * sweep / 4.0).tan()));
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

/// Horizontal (0 left, 1 centre, 2 right) and vertical (0 top, 1 middle,
/// 2 baseline, 3 descender) parts of a DGN justification code.
fn just_parts(just: u8) -> (u8, u8) {
    match just {
        0..=2 => (0, just),
        3..=5 => (0, just - 3),
        6..=8 => (1, just - 6),
        9..=11 => (2, just - 9),
        12..=14 => (2, just - 12),
        16 => (0, 3),
        20 => (1, 3),
        24 => (2, 3),
        _ => (0, 2),
    }
}

/// The justification point of a text: its lower left moved along the
/// baseline by the part of its length, up by the part of its height (a
/// descender justification 0.3 heights below the baseline).
fn just_point(origin: P3, rotation: f64, length: f64, height: f64, just: u8) -> P3 {
    let (h, v) = just_parts(just);
    let along = length * h as f64 / 2.0;
    let up = match v {
        // Centre top is placed half a height up.
        0 if just == 6 => height / 2.0,
        0 => height,
        1 => height / 2.0,
        2 => 0.0,
        _ => -0.3 * height,
    };
    let (c, s) = (rotation.cos(), rotation.sin());
    [
        origin[0] + along * c - up * s,
        origin[1] + along * s + up * c,
        origin[2],
    ]
}

fn attachment(just: u8) -> AttachmentPoint {
    use AttachmentPoint as A;
    match just_parts(just) {
        (0, 0) => A::TopLeft,
        (1, 0) => A::TopCenter,
        (2, 0) => A::TopRight,
        (0, 1) => A::MiddleLeft,
        (1, 1) => A::MiddleCenter,
        (2, 1) => A::MiddleRight,
        (1, _) => A::BottomCenter,
        (2, _) => A::BottomRight,
        _ => A::BottomLeft,
    }
}

/// Text as MTEXT contents: backslashes and braces stay characters instead
/// of becoming formatting codes.
fn mtext_escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('{', "\\{")
        .replace('}', "\\}")
}

/// A number with six significant digits, trailing zeros dropped.
fn sig6(x: f64) -> String {
    let digits = if x == 0.0 {
        0
    } else {
        (5 - x.abs().log10().floor() as i32).max(0) as usize
    };
    let s = format!("{x:.digits$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// The colour code a V8 text carries in its contents: 256 for the level's
/// colour, 0 for the cell's, else the colour index nearest its table entry.
fn color_code(color: By<crate::scene::model::underlay_vector::DgnColor>) -> u16 {
    match color {
        By::Level => 256,
        By::Cell => 0,
        By::Own(c) => {
            codec::types::aci_table::nearest_aci(c.table[0], c.table[1], c.table[2]) as u16
        }
    }
}

/// The text style of a line of text (V7: one per font number, "dgnNNN";
/// V8: the style the text names), added to `styles` with its font file
/// the first time.
fn text_style(geom: &DgnGeom, import: &DgnImport, styles: &mut Vec<(String, String)>) -> String {
    match geom {
        DgnGeom::Text { style: Some(id), .. } => match import.text_styles.iter().find(|(i, _)| i == id) {
            Some((_, name)) if !name.eq_ignore_ascii_case("Standard") => {
                let name = symbol_name(name);
                if !styles.iter().any(|s| s.0 == name) {
                    styles.push((name.clone(), "txt.shx".to_string()));
                }
                name
            }
            _ => "Standard".to_string(),
        },
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

/// A V7 text as TEXT, justified at its justification point.
fn text_entity(geom: &DgnGeom, style: &str) -> Option<EntityType> {
    let DgnGeom::Text {
        text,
        origin,
        height,
        width_factor,
        length,
        rotation,
        just,
        ..
    } = geom
    else {
        return None;
    };
    let mut t = Text::with_value(text.clone(), v3(*origin))
        .with_height(*height)
        .with_rotation(rotation.rem_euclid(std::f64::consts::TAU));
    t.width_factor = if *width_factor > 0.0 { *width_factor } else { 1.0 };
    t.style = style.to_string();
    let (h, v) = just_parts(*just);
    if (h, v) != (0, 2) {
        t.horizontal_alignment = match h {
            1 => TextHorizontalAlignment::Center,
            2 => TextHorizontalAlignment::Right,
            _ => TextHorizontalAlignment::Left,
        };
        t.vertical_alignment = match v {
            0 => TextVerticalAlignment::Top,
            1 => TextVerticalAlignment::Middle,
            3 => TextVerticalAlignment::Bottom,
            _ => TextVerticalAlignment::Baseline,
        };
        t.alignment_point = Some(v3(just_point(*origin, *rotation, *length, *height, *just)));
    }
    Some(EntityType::Text(t))
}

fn text_line(g: &DgnGeom) -> Option<(&str, P3, f64, f64, f64)> {
    match g {
        DgnGeom::Text {
            text,
            origin,
            height,
            rotation,
            width_factor,
            ..
        } => Some((text.as_str(), *origin, *height, *rotation, *width_factor)),
        _ => None,
    }
}

/// A V8 text as MTEXT: a bottom left text
/// attached at its bottom left a third of the height below the baseline
/// (exact line spacing 0.8), any other at its justification point; its
/// contents carry its colour (and width factor).
fn mtext_of_text(geom: &DgnGeom, color: u16, style: &str) -> Option<EntityType> {
    let DgnGeom::Text {
        text,
        origin,
        height,
        width_factor,
        length,
        rotation,
        just,
        ..
    } = geom
    else {
        return None;
    };
    let mut m = MText::new();
    let width = if (width_factor - 1.0).abs() > 1e-6 {
        format!("\\W{};", sig6(*width_factor))
    } else {
        String::new()
    };
    m.value = format!("{{{width}\\C{color};{}}}", mtext_escape(text));
    if *just == 2 {
        let up = -0.33 * height;
        m.insertion_point = Vector3::new(
            origin[0] - rotation.sin() * up,
            origin[1] + rotation.cos() * up,
            origin[2],
        );
        m.attachment_point = AttachmentPoint::BottomLeft;
        m.line_spacing_style = LineSpacingStyle::Exactly;
        m.line_spacing_factor = 0.8;
    } else {
        m.insertion_point = v3(just_point(*origin, *rotation, *length, *height, *just));
        m.attachment_point = attachment(*just);
        m.line_spacing_style = LineSpacingStyle::AtLeast;
        m.line_spacing_factor = 1.0;
    }
    m.height = *height;
    m.rectangle_height = Some(*height);
    m.rotation = rotation.rem_euclid(std::f64::consts::TAU);
    m.rectangle_width = 0.0;
    m.drawing_direction = DrawingDirection::ByStyle;
    m.style = style.to_string();
    Some(EntityType::MText(Box::new(m)))
}

/// A text node as one MTEXT, a paragraph a line, attached at the node's
/// origin by its justification. V8: its contents carry its colour, its
/// defined height spans its lines; V7: exact spacing of the node's line
/// spacing.
fn mtext_of_node(
    lines: &[DgnGeom],
    origin: Option<P3>,
    just: u8,
    line_spacing: Option<f64>,
    style: String,
    color: Option<u16>,
) -> Option<EntityType> {
    let (_, first, height, rotation, _) = lines.iter().find_map(text_line)?;
    let texts: Vec<String> = lines
        .iter()
        .filter_map(text_line)
        .map(|l| mtext_escape(l.0))
        .collect();
    let mut m = MText::new();
    let joined = texts.join("\\P");
    m.value = match color {
        Some(c) => format!("{{\\C{c};{joined}}}"),
        None => joined,
    };
    let up = [-rotation.sin(), rotation.cos(), 0.0];
    let (just, insertion) = match origin {
        Some(o) => (just, o),
        None => (0, std::array::from_fn(|c| first[c] + up[c] * height)),
    };
    m.insertion_point = v3(insertion);
    m.attachment_point = attachment(just);
    match line_spacing {
        // V7: the gap between lines over the default pitch (5/3 heights).
        Some(gap) => {
            m.line_spacing_style = LineSpacingStyle::Exactly;
            m.line_spacing_factor = if height > 0.0 {
                (height + gap) / (height * 5.0 / 3.0)
            } else {
                1.0
            };
            m.rectangle_height = None;
        }
        None => {
            let last = lines.iter().filter_map(text_line).last().map_or(first, |l| l.1);
            m.line_spacing_style = LineSpacingStyle::AtLeast;
            m.line_spacing_factor = 1.0;
            m.rectangle_height = Some(height + dot(sub(first, last), up).max(0.0));
        }
    }
    m.height = height;
    m.rotation = rotation.rem_euclid(std::f64::consts::TAU);
    m.rectangle_width = 0.0;
    m.drawing_direction = DrawingDirection::ByStyle;
    m.style = style;
    Some(EntityType::MText(Box::new(m)))
}

/// An arc, circle or ellipse in its own plane.
fn arc_entity(center: P3, u: P3, v: P3, start: f64, sweep: f64) -> Vec<EntityType> {
    let full = sweep.abs() >= std::f64::consts::TAU - 1e-9;
    let (normal, flipped) = arc_plane(u, v);
    let at = |t: f64| -> P3 { std::array::from_fn(|c| t.cos() * u[c] + t.sin() * v[c]) };
    if circular(u, v) {
        let radius = norm(u);
        let c = to_ocs(normal, center);
        if full {
            let mut circle = Circle::from_center_radius(c, radius);
            circle.normal = v3(normal);
            return vec![EntityType::Circle(circle)];
        }
        let (a0, a1) = (ocs_angle(normal, at(start)), ocs_angle(normal, at(start + sweep)));
        // The arc runs counter-clockwise about its normal.
        let ccw = (sweep > 0.0) != flipped;
        let (from, to) = if ccw { (a0, a1) } else { (a1, a0) };
        let mut arc = Arc::from_center_radius_angles(
            c,
            radius,
            from.rem_euclid(std::f64::consts::TAU),
            to.rem_euclid(std::f64::consts::TAU),
        );
        arc.normal = v3(normal);
        return vec![EntityType::Arc(arc)];
    }
    let (lu, lv) = (norm(u), norm(v));
    if dot(u, v).abs() > 1e-9 * lu * lv || lu < 1e-12 || lv < 1e-12 {
        // ponytail: axes not at right angles cannot come out of one plane's
        // ellipse; kept as points.
        return vec![polyline(&arc_points(center, u, v, start, sweep), &[])];
    }
    // P(t) = c + cos t·u + sin t·v as an ellipse on its major axis with the
    // minor axis turned counter-clockwise from it about the normal.
    let ccw = !flipped;
    let (major, ratio, map): (P3, f64, Box<dyn Fn(f64) -> f64>) = if lu >= lv {
        if ccw {
            (u, lv / lu, Box::new(|t| t))
        } else {
            (u, lv / lu, Box::new(|t: f64| -t))
        }
    } else if ccw {
        (v, lu / lv, Box::new(|t: f64| t - std::f64::consts::FRAC_PI_2))
    } else {
        (v, lu / lv, Box::new(|t: f64| std::f64::consts::FRAC_PI_2 - t))
    };
    let mut e = Ellipse::from_center_axes(v3(center), v3(major), ratio);
    e.normal = v3(normal);
    if !full {
        let (p0, p1) = (map(start), map(start + sweep));
        let (from, to) = if p1 >= p0 { (p0, p1) } else { (p1, p0) };
        e.start_parameter = from.rem_euclid(std::f64::consts::TAU);
        e.end_parameter = e.start_parameter + (to - from);
    }
    vec![EntityType::Ellipse(e)]
}

/// A shared cell instance as an INSERT: a mirrored placement looks along
/// -z (x negated in its entity coordinates, z scale negated).
fn insert_entity(block: String, m: [f64; 4], origin: P3, z_scale: f64) -> EntityType {
    let det = m[0] * m[3] - m[1] * m[2];
    let mirrored = det < 0.0;
    let m = if mirrored { [-m[0], -m[1], m[2], m[3]] } else { m };
    let sx = m[0].hypot(m[2]);
    let sy = if sx > 0.0 { (m[0] * m[3] - m[1] * m[2]) / sx } else { 1.0 };
    let (point, z) = if mirrored {
        (Vector3::new(-origin[0], origin[1], -origin[2]), -z_scale)
    } else {
        (v3(origin), z_scale)
    };
    let mut insert = Insert::new(block, point)
        .with_scale(sx, sy, z)
        .with_rotation(m[2].atan2(m[0]));
    if mirrored {
        insert.normal = Vector3::new(0.0, 0.0, -1.0);
    }
    EntityType::Insert(insert)
}

/// How a run of elements converts: a V7 cell copy turns every weight (and
/// a cell of one colour its colour) over to the inserting block.
#[derive(Clone, Copy, Default)]
struct CellCopy {
    active: bool,
    color_by_block: bool,
}

/// Conversion state: the output, the V7 cell copies made so far and the
/// definitions they copy.
struct Converter<'a> {
    import: &'a DgnImport,
    explode_text_nodes: bool,
    out: Converted,
    defs: HashMap<String, &'a [DgnElement]>,
    /// (cell, made inside a copy) → copy block name.
    copies: HashMap<(String, bool), String>,
    names: Vec<String>,
    /// Converting a shared cell definition: the cells inside it are not
    /// blocks of their own, their objects are part of the definition.
    in_definition: bool,
}

impl<'a> Converter<'a> {
    /// `base`, or `base` and the first number that makes it a new name.
    fn unique(&mut self, base: &str) -> String {
        let taken = |n: &str| self.names.iter().any(|t| t.eq_ignore_ascii_case(n));
        let name = if taken(base) {
            (1..)
                .map(|k| format!("{base}{k}"))
                .find(|n| !taken(n))
                .unwrap_or_default()
        } else {
            base.to_string()
        };
        self.names.push(name.clone());
        name
    }

    /// Every colour index a cell draws with, through the cells it places.
    fn cell_colors(&self, name: &str, depth: usize, out: &mut Vec<u32>) {
        let Some(def) = self.defs.get(&name.to_ascii_uppercase()) else {
            return;
        };
        for e in def.iter() {
            if let By::Own(c) = e.color {
                out.push(c.index);
            }
            if let (DgnGeom::Cell { name, .. }, true) = (&e.geom, depth < 8) {
                self.cell_colors(name, depth + 1, out);
            }
        }
    }

    /// The block a V7 instance of `cell` inserts: a copy of the definition
    /// whose weights (and, when the whole cell is one colour, colour) come
    /// from the INSERT, one per definition placed at the top or inside
    /// definitions and another for those inside copies.
    fn v7_copy(&mut self, cell: &str, in_copy: bool, depth: usize) -> String {
        let key = (cell.to_ascii_uppercase(), in_copy);
        if let Some(name) = self.copies.get(&key) {
            return name.clone();
        }
        let name = self.unique(&block_name(cell));
        self.copies.insert(key, name.clone());
        let Some(def) = self.defs.get(&cell.to_ascii_uppercase()).copied() else {
            return name;
        };
        let mut colors = Vec::new();
        self.cell_colors(cell, 0, &mut colors);
        let copy = CellCopy {
            active: true,
            color_by_block: colors.windows(2).all(|w| w[0] == w[1]),
        };
        let entities = self.elements(def, true, copy, depth + 1);
        self.out.blocks.push((name.clone(), entities));
        name
    }

    fn layer(&mut self, element: &DgnElement, in_cell: bool) -> String {
        // A cell's objects are on layer 0 of its block.
        let layer = if in_cell {
            "0".to_string()
        } else {
            layer_name(self.import, element.level)
        };
        if !self.out.layers.iter().any(|l| l.0 == layer) {
            let color = self
                .import
                .levels
                .get(&element.level)
                .and_then(|l| l.1)
                .filter(|_| !in_cell)
                .map(color_of)
                .unwrap_or(Color::Index(7));
            self.out.layers.push((layer.clone(), color));
        }
        layer
    }

    /// The object(s) one element converts to.
    fn geom(
        &mut self,
        element: &DgnElement,
        geom: &DgnGeom,
        copy: CellCopy,
        depth: usize,
    ) -> Vec<EntityType> {
        match geom {
            DgnGeom::Line(a, b) if dot(sub(*a, *b), sub(*a, *b)) < 1e-24 => {
                vec![EntityType::Point(Point::from_coords(a[0], a[1], a[2]))]
            }
            DgnGeom::Line(a, b) => vec![EntityType::Line(Line::from_points(v3(*a), v3(*b)))],
            // A filled shape is its outline and a solid hatch inside it.
            DgnGeom::Points(pts) if pts.len() >= 2 => {
                let mut out = vec![polyline(pts, &[])];
                if element.filled {
                    out.extend(solid_hatch(pts, &[]));
                }
                out
            }
            DgnGeom::Points(_) => Vec::new(),
            DgnGeom::Arc {
                center,
                u,
                v,
                start,
                sweep,
            } => arc_entity(*center, *u, *v, *start, *sweep),
            DgnGeom::Spline {
                degree,
                knots,
                poles,
                closed,
            } => {
                let mut s = Spline::new();
                s.degree = *degree as i32;
                s.knots = knots.clone();
                s.control_points = poles.iter().map(|p| v3(*p)).collect();
                s.flags.closed = *closed;
                // A V8 closed curve is periodic (its knots do not start at
                // 0); a V7 one is clamped.
                s.flags.periodic = *closed && knots.first().is_some_and(|k| *k > 0.0);
                s.flags.planar = true;
                vec![EntityType::Spline(s)]
            }
            DgnGeom::Chain(parts) => {
                let (points, bulges) = chain(parts);
                if points.len() < 2 {
                    return Vec::new();
                }
                let mut out = vec![polyline(&points, &bulges)];
                if element.filled {
                    out.extend(solid_hatch(&points, &bulges));
                }
                out
            }
            // V7 text is TEXT, V8 text an MTEXT.
            DgnGeom::Text { font: Some(_), .. } => {
                let style = text_style(geom, self.import, &mut self.out.styles);
                text_entity(geom, &style).into_iter().collect()
            }
            DgnGeom::Text { .. } => {
                let style = text_style(geom, self.import, &mut self.out.styles);
                mtext_of_text(geom, color_code(element.color), &style)
                    .into_iter()
                    .collect()
            }
            // Exploded, each line converts as a text element of its own.
            DgnGeom::TextNode { lines, .. } if self.explode_text_nodes => lines
                .iter()
                .flat_map(|g| self.geom(element, g, copy, depth))
                .collect(),
            DgnGeom::TextNode {
                lines,
                origin,
                just,
                line_spacing,
            } => {
                let Some(first) = lines.first() else {
                    return Vec::new();
                };
                let style = text_style(first, self.import, &mut self.out.styles);
                let color = (!self.import.v7).then(|| color_code(element.color));
                mtext_of_node(lines, *origin, *just, *line_spacing, style, color)
                    .into_iter()
                    .collect()
            }
            // A cell that is not shared becomes a block of its own ("the
            // cell's name, else AnonymousCell" and a number) placed at the
            // origin; its objects keep their layers.
            DgnGeom::CellGroup { parts, .. } if self.in_definition => {
                self.elements(parts, true, copy, depth + 1)
            }
            DgnGeom::CellGroup { name, parts } => {
                let base = if name.is_empty() { "AnonymousCell" } else { name.as_str() };
                let block = self.unique(&block_name(base));
                let entities = self.elements(parts, false, copy, depth + 1);
                self.out.blocks.push((block.clone(), entities));
                vec![insert_entity(block, [1.0, 0.0, 0.0, 1.0], [0.0; 3], 1.0)]
            }
            DgnGeom::Cell {
                name,
                m,
                origin,
                z_scale,
            } => {
                let block = if self.import.v7 && depth < 8 {
                    self.v7_copy(name, copy.active, depth)
                } else {
                    block_name(name)
                };
                vec![insert_entity(block, *m, *origin, *z_scale)]
            }
        }
    }

    fn elements(
        &mut self,
        elements: &[DgnElement],
        in_cell: bool,
        copy: CellCopy,
        depth: usize,
    ) -> Vec<EntityType> {
        let mut entities = Vec::new();
        for element in elements {
            let layer = self.layer(element, in_cell);
            for mut entity in self.geom(element, &element.geom, copy, depth) {
                let common = entity.common_mut();
                common.layer = layer.clone();
                common.color = match element.color {
                    By::Level => Color::ByLayer,
                    By::Cell => Color::ByBlock,
                    By::Own(c) => color_of(c.rgb),
                };
                common.line_weight = match element.weight {
                    By::Level => LineWeight::ByLayer,
                    By::Cell => LineWeight::ByBlock,
                    By::Own(w) => LineWeight::Value(lineweight_of(w)),
                };
                // The Standard mapping draws every line style Continuous.
                match element.style {
                    By::Own(_) => common.linetype = "Continuous".to_string(),
                    By::Cell => common.linetype = "ByBlock".to_string(),
                    By::Level => {}
                }
                if copy.active {
                    common.line_weight = LineWeight::ByBlock;
                    if copy.color_by_block {
                        common.color = Color::ByBlock;
                    }
                }
                entities.push(entity);
            }
        }
        entities
    }
}

/// The import's objects, in sub units when `sub_units`. V7 cell
/// definitions become blocks as drawn; each is placed through a copy (the
/// definition's name and a number), made before the blocks that place it. V8 cells are blocks placed as they
/// are. A V8 file's levels all become layers, in its level table's order.
pub fn convert(import: &DgnImport, sub_units: bool, explode_text_nodes: bool) -> Converted {
    let k = if sub_units { import.sub_per_master } else { 1.0 };
    let scale = |elements: &[DgnElement]| {
        crate::scene::model::underlay_vector::scaled_elements(elements, k, [0.0, 0.0])
    };
    let cells: Vec<(String, Vec<DgnElement>)> = import
        .cells
        .iter()
        .map(|(name, elements)| (name.clone(), scale(elements)))
        .collect();
    let top = scale(&import.elements);
    let mut c = Converter {
        import,
        explode_text_nodes,
        out: Converted::default(),
        defs: cells
            .iter()
            .map(|(name, elements)| (name.to_ascii_uppercase(), elements.as_slice()))
            .collect(),
        copies: HashMap::new(),
        names: cells.iter().map(|(name, _)| block_name(name)).collect(),
        in_definition: false,
    };
    if !import.v7 {
        for level in &import.level_order {
            let element = DgnElement {
                level: *level,
                color: By::Level,
                weight: By::Level,
                style: By::Level,
                geom: DgnGeom::Points(Vec::new()),
                filled: false,
            };
            c.layer(&element, false);
        }
    }
    // Every text style of the file's table comes in, used or not.
    for (_, name) in &import.text_styles {
        let name = symbol_name(name);
        if !name.eq_ignore_ascii_case("Standard") && !c.out.styles.iter().any(|s| s.0 == name) {
            c.out.styles.push((name, "txt.shx".to_string()));
        }
    }
    c.in_definition = true;
    for (name, elements) in &cells {
        let entities = c.elements(elements, true, CellCopy::default(), 0);
        if !name.is_empty() && !entities.is_empty() {
            c.out.blocks.push((block_name(name), entities));
        }
    }
    c.in_definition = false;
    let entities = c.elements(&top, false, CellCopy::default(), 0);
    c.out.entities = entities;
    // A cell left without a block (the point display cell, an empty or
    // missing definition) places nothing: its instances are dropped.
    let blocks = &c.out.blocks;
    let defined: HashSet<String> = blocks.iter().map(|b| b.0.to_ascii_uppercase()).collect();
    let placed = |e: &EntityType| match e {
        EntityType::Insert(i) => defined.contains(&i.block_name.to_ascii_uppercase()),
        _ => true,
    };
    c.out.entities.retain(placed);
    for (_, entities) in &mut c.out.blocks {
        entities.retain(placed);
    }
    c.out
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
                By::Own(s) => add(s.to_string(), "Continuous".to_string()),
                By::Cell => add("ByCell".to_string(), "ByBlock".to_string()),
                By::Level => add("ByLevel".to_string(), "ByLayer".to_string()),
            },
            2 => match e.weight {
                By::Own(w) => add(
                    w.to_string(),
                    format!("{:.2} mm", lineweight_of(w) as f64 / 100.0),
                ),
                By::Cell => add("ByCell".to_string(), "ByBlock".to_string()),
                By::Level => add("ByLevel".to_string(), "ByLayer".to_string()),
            },
            _ => match e.color {
                By::Own(c) => {
                    let rgb = c.rgb;
                    add(
                        format!("RGB {},{},{}", rgb[0], rgb[1], rgb[2]),
                        match color_of(rgb) {
                            Color::Index(i) => i.to_string(),
                            _ => format!("{},{},{}", rgb[0], rgb[1], rgb[2]),
                        },
                    )
                }
                By::Cell => add("ByCell".to_string(), "ByBlock".to_string()),
                By::Level => add("ByLevel".to_string(), "ByLayer".to_string()),
            },
        }
    }
    rows
}

// ── Settings ───────────────────────────────────────────────────────────────

/// The import settings kept in the profile: DGNIMPORTMODE (0 a new
/// drawing, 1 the current drawing keeping its own definitions, 2 the
/// current drawing with prefixed names) and the DGN Import window's last
/// choices.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DgnImportSettings {
    pub mode: u8,
    pub explode_text_nodes: bool,
    pub references: ReferenceMode,
    pub existing_reference: ExistingReference,
}

static SETTINGS: std::sync::Mutex<Option<DgnImportSettings>> = std::sync::Mutex::new(None);

pub fn settings() -> DgnImportSettings {
    SETTINGS
        .lock()
        .ok()
        .and_then(|s| *s)
        .unwrap_or_default()
}

pub fn set_settings(mut settings: DgnImportSettings) {
    settings.mode = settings.mode.min(2);
    if let Ok(mut s) = SETTINGS.lock() {
        *s = Some(settings);
    }
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
        let models = crate::scene::model::underlay_vector::dgn_import_models(&path)
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
        // The rest comes from DGNIMPORTMODE and the import settings the
        // window last kept.
        let s = settings();
        CmdResult::DgnImport(DgnImportRequest {
            path: self.path.clone(),
            model: self.model.clone(),
            sub_units: self.sub_units,
            explode_text_nodes: s.explode_text_nodes,
            current_drawing: s.mode != 0,
            prefix_duplicates: s.mode == 2,
            references: s.references,
            existing_reference: s.existing_reference,
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
