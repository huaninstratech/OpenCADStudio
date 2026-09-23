// STEP (ISO 10303-21, AP203/AP214/AP242) reader — B-rep solids to meshes.
//
// Shares the SPF parser with the IFC reader. Product names come from the
// PRODUCT_DEFINITION chain; geometry from MANIFOLD_SOLID_BREP / FACETED_BREP
// shells. Faces are tessellated per surface type:
//   - PLANE: exact projection + ear clipping (holes respected).
//   - Cylinder/cone/sphere/torus: parametric grid over the range implied by
//     the trim loops, clipped in UV against the sampled loops (a visual
//     approximation, not machining tolerance).
//   - B-spline surfaces: de Boor evaluation over the file's knots, clipped
//     by nearest-grid-node UV mapping.
//
// v1 limits, recorded in `warnings`: assembly placement transforms are
// ignored (parts import at their absolute coordinates), and unsupported
// surfaces (surface of revolution/extrusion, offsets) are skipped.

use super::meshutil::{ear_clip, point_in_loops, poly_normal, TriSink};
use super::spf::{Spf, Val};
use crate::scene::model::mesh_model::MeshModel;

const DEFAULT_COLOR: [f32; 4] = [0.72, 0.72, 0.78, 1.0];
const TAU: f64 = std::f64::consts::TAU;

#[derive(Clone, Debug)]
pub struct StepImportResult {
    pub schema: String,
    pub meshes: Vec<MeshModel>,
    pub warnings: Vec<String>,
    pub products: usize,
}

pub fn parse_step(bytes: &[u8]) -> Result<StepImportResult, String> {
    StepImportResult::parse_step_with_progress(bytes, None)
}

impl StepImportResult {
    /// Same parse, reporting into the shared open-progress overlay (parse
    /// scan basis 500–4500, meshing 4500–9500).
    pub fn parse_step_with_progress(
        bytes: &[u8],
        progress: Option<&crate::io::OpenProgressState>,
    ) -> Result<StepImportResult, String> {
        use crate::app::{OPEN_PHASE_CACHING, OPEN_PHASE_FINALIZING, OPEN_PHASE_PARSING, OPEN_PHASE_READING};
        use std::sync::atomic::Ordering;
        let total = bytes.len();
        if let Some(p) = progress {
            p.set(OPEN_PHASE_READING, 400, 0, 1);
        }
        let report_parse = progress.map(|p| {
            move |pos: usize, total_len: usize| {
                let basis =
                    500 + ((pos.min(total_len) as u64) * 4000 / total_len.max(1) as u64) as u16;
                p.set(OPEN_PHASE_PARSING, basis, pos / 1024, total_len / 1024);
            }
        });
        let spf = match report_parse.as_ref() {
            Some(report) => Spf::parse_with_progress(bytes, Some(report as &dyn Fn(usize, usize))),
            None => Spf::parse_with_progress(bytes, None),
        }?;
        if spf.is_empty() {
            return Err("no data entities in file".into());
        }
        let scale = length_scale(&spf);
        let reader = Reader {
            spf: &spf,
            scale,
            names: product_names(&spf),
            colors: styled_colours(&spf),
            warnings: std::sync::Mutex::new(Vec::new()),
        };
        const SOLID_TYPES: [&str; 4] = [
            "MANIFOLD_SOLID_BREP",
            "BREP_WITH_VOIDS",
            "FACETED_BREP",
            "SHELL_BASED_SURFACE_MODEL",
        ];
        // Solids are independent — mesh them across threads on desktop builds.
        let pending: Vec<(u64, String, [f32; 4])> = spf
            .iter()
            .filter(|ent| SOLID_TYPES.iter().any(|t| ent.is(t)))
            .map(|ent| {
                let name = reader
                    .names
                    .get(&ent.id)
                    .cloned()
                    .unwrap_or_else(|| format!("Solid {}", ent.id));
                let color = reader.colors.get(&ent.id).copied().unwrap_or(DEFAULT_COLOR);
                (ent.id, name, color)
            })
            .collect();
        let total_pending = pending.len();
        let done = std::sync::atomic::AtomicUsize::new(0);

        let mesh_one = |item: &(u64, String, [f32; 4])| -> Option<MeshModel> {
            let ent = spf.get(item.0)?;
            let mut sink = TriSink::default();
            if ent.is("SHELL_BASED_SURFACE_MODEL") {
                if let Some(shells) = ent.list(1) {
                    for shell in shells {
                        if let Some(shell_id) = shell.as_ref() {
                            reader.mesh_shell(shell_id, &mut sink);
                        }
                    }
                }
            } else if let Some(shell_id) = ent.ref_id(1) {
                reader.mesh_shell(shell_id, &mut sink);
            }
            if ent.is("BREP_WITH_VOIDS") {
                if let Some(voids) = ent.list(2) {
                    for void in voids {
                        if let Some(void_id) = void.as_ref() {
                            reader.mesh_shell(void_id, &mut sink);
                        }
                    }
                }
            }
            if let Some(p) = progress {
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                let basis = 4500 + ((n as u64) * 5000 / total_pending.max(1) as u64) as u16;
                p.set(OPEN_PHASE_CACHING, basis, n, total_pending);
            }
            if sink.tris.is_empty() {
                return None;
            }
            Some(sink_to_mesh(&sink, &item.1, item.2, reader.scale))
        };

        #[cfg(not(target_arch = "wasm32"))]
        let meshes: Vec<MeshModel> = {
            use rayon::prelude::*;
            pending.par_iter().filter_map(mesh_one).collect()
        };
        #[cfg(target_arch = "wasm32")]
        let meshes: Vec<MeshModel> = pending.iter().filter_map(mesh_one).collect();

        let products = reader.names.len();
        let mut warnings = reader.warnings.into_inner().unwrap_or_default();
        if spf.first_of_type("NEXT_ASSEMBLY_USAGE_OCCURRENCE").is_some() {
            warnings.push(
                "assembly placement transforms are ignored; parts import at absolute coordinates"
                    .into(),
            );
        }
        if meshes.is_empty() {
            warnings.push(
                "no importable solid found (only untrimmed curves or unsupported surfaces?)".into(),
            );
        }
        if let Some(p) = progress {
            p.set(OPEN_PHASE_FINALIZING, 9900, 1, 1);
        }
        Ok(StepImportResult {
            schema: spf.schema().to_string(),
            meshes,
            warnings,
            products,
        })
    }
}

// ── units and naming ─────────────────────────────────────────────────────

const SI_PREFIXES: &[(&str, f64)] = &[
    ("EXA", 1e18),
    ("PETA", 1e15),
    ("TERA", 1e12),
    ("GIGA", 1e9),
    ("MEGA", 1e6),
    ("KILO", 1e3),
    ("HECTO", 1e2),
    ("DECA", 1e1),
    ("DECI", 1e-1),
    ("CENTI", 1e-2),
    ("MILLI", 1e-3),
    ("MICRO", 1e-6),
    ("NANO", 1e-9),
];

/// File length unit → millimetres.
fn length_scale(spf: &Spf) -> f64 {
    for ent in spf.iter() {
        if !ent.is("LENGTH_UNIT") {
            continue;
        }
        if ent.is("CONVERSIONBASEDUNIT") {
            let name = ent
                .args
                .iter()
                .find_map(Val::as_str)
                .unwrap_or("")
                .to_uppercase();
            let factor = ent.args.iter().find_map(|v| {
                v.as_ref()
                    .and_then(|id| spf.get(id))
                    .filter(|m| m.is("MEASURE_WITH_UNIT"))
                    .and_then(|m| m.num(0))
            });
            if let Some(factor) = factor {
                return match name.as_str() {
                    "INCH" | "INCHES" => factor * 25.4,
                    "FOOT" | "FEET" => factor * 304.8,
                    "MILLIMETRE" | "MM" => factor,
                    "CENTIMETRE" | "CM" => factor * 10.0,
                    "METRE" | "METER" | "M" => factor * 1000.0,
                    _ => factor,
                };
            }
            continue;
        }
        let enums: Vec<String> = ent
            .args
            .iter()
            .filter_map(Val::as_enum)
            .map(|e| e.trim_end_matches('.').to_uppercase())
            .collect();
        if !enums.iter().any(|e| e == "METRE") {
            continue;
        }
        for (prefix, factor) in SI_PREFIXES {
            if enums.iter().any(|e| e == *prefix) {
                return 1000.0 * factor;
            }
        }
        return 1000.0;
    }
    1.0
}

/// Map solid entity id → product name via
/// SHAPE_DEFINITION_REPRESENTATION → PRODUCT_DEFINITION_SHAPE → … → PRODUCT.
fn product_names(spf: &Spf) -> std::collections::HashMap<u64, String> {
    use std::collections::HashMap;
    // A Representation carries its geometry in slot 1: (Name, Items, Context).
    let mut item_rep: HashMap<u64, u64> = HashMap::new();
    for ent in spf.iter() {
        let ty = ent.ty();
        if ty.ends_with("SHAPE_REPRESENTATION")
            && !ty.contains("RELATIONSHIP")
            && !ty.contains("CONTEXT")
        {
            if let Some(items) = ent.list(1) {
                for item in items {
                    if let Some(id) = item.as_ref() {
                        item_rep.entry(id).or_insert(ent.id);
                    }
                }
            }
        }
    }
    let mut rep_name: HashMap<u64, String> = HashMap::new();
    for ent in spf.iter() {
        if !ent.is("SHAPE_DEFINITION_REPRESENTATION") {
            continue;
        }
        let Some(rep_id) = ent.ref_id(1) else { continue };
        let name = ent
            .ref_id(0)
            .and_then(|pds| spf.get(pds))
            .and_then(|pds| pds.ref_id(2))
            .and_then(|pd| spf.get(pd))
            .and_then(|pd| pd.ref_id(2))
            .and_then(|formation| spf.get(formation))
            .and_then(|formation| formation.ref_id(2))
            .and_then(|product| spf.get(product))
            .and_then(|product| product.str(1))
            .unwrap_or("");
        if !name.is_empty() {
            rep_name.insert(rep_id, name.to_string());
        }
    }
    item_rep
        .into_iter()
        .filter_map(|(item, rep)| rep_name.get(&rep).map(|n| (item, n.clone())))
        .collect()
}

/// AP214 styled colours: STYLED_ITEM(Name, Styles, Item) → walk the style
/// wrappers (surface style usage, side style, fill area style, …) down to a
/// COLOUR_RGB. Breadth-first over entity references keeps it simple.
fn styled_colours(spf: &Spf) -> std::collections::HashMap<u64, [f32; 4]> {
    use std::collections::HashMap;
    let mut out: HashMap<u64, [f32; 4]> = HashMap::new();
    for ent in spf.iter() {
        if !ent.is("STYLED_ITEM") {
            continue;
        }
        let Some(item) = ent.ref_id(2) else { continue };
        let Some(styles) = ent.list(1) else { continue };
        let mut queue: Vec<u64> = styles.iter().filter_map(Val::as_ref).collect();
        let mut steps = 0usize;
        let mut colour = None;
        while let Some(id) = queue.pop() {
            steps += 1;
            if steps > 64 {
                break;
            }
            let Some(style_ent) = spf.get(id) else { continue };
            if style_ent.is("COLOUR_RGB") {
                colour = Some([
                    style_ent.num(1).unwrap_or(0.0) as f32,
                    style_ent.num(2).unwrap_or(0.0) as f32,
                    style_ent.num(3).unwrap_or(0.0) as f32,
                    1.0,
                ]);
                break;
            }
            for arg in &style_ent.args {
                if let Some(reference) = arg.as_ref() {
                    queue.push(reference);
                } else if let Some(list) = arg.as_list() {
                    for value in list {
                        if let Some(reference) = value.as_ref() {
                            queue.push(reference);
                        }
                    }
                }
            }
        }
        if let Some(colour) = colour {
            out.insert(item, colour);
        }
    }
    out
}

// ── geometry ─────────────────────────────────────────────────────────────

struct Reader<'a> {
    spf: &'a Spf,
    scale: f64,
    names: std::collections::HashMap<u64, String>,
    /// solid entity id → RGBA colour (STYLED_ITEM → … → COLOUR_RGB)
    colors: std::collections::HashMap<u64, [f32; 4]>,
    /// Shared so meshing can fan out across threads (rayon) on desktop.
    warnings: std::sync::Mutex<Vec<String>>,
}

impl<'a> Reader<'a> {
    fn warn_once(&self, message: String) {
        if let Ok(mut guard) = self.warnings.lock() {
            if !guard.contains(&message) {
                guard.push(message);
            }
        }
    }

    fn mesh_shell(&self, shell_id: u64, sink: &mut TriSink) {
        let spf: &'a Spf = self.spf;
        let Some(shell) = spf.get(shell_id) else { return };
        let Some(faces) = shell.list(1) else { return };
        for face in faces {
            let Some(face_id) = face.as_ref() else { continue };
            self.mesh_face(face_id, sink);
        }
    }

    fn mesh_face(&self, face_id: u64, sink: &mut TriSink) {
        let spf: &'a Spf = self.spf;
        let Some(face) = spf.get(face_id) else { return };
        // ORIENTED_FACE wraps another face with a flip flag.
        let (inner, sense) = if face.is("ORIENTED_FACE") {
            let flip = matches!(face.get(2), Some(Val::Enum(e)) if e.as_str() == "F");
            let inner = face.ref_id(1).and_then(|id| spf.get(id));
            match inner {
                Some(inner) => (inner, bool_arg(inner, 3).unwrap_or(true) != flip),
                None => return,
            }
        } else {
            (face, bool_arg(face, 3).unwrap_or(true))
        };
        let Some(bounds) = inner.list(1) else { return };
        let Some(geometry_id) = inner.ref_id(2) else { return };
        let Some(surface) = spf.get(geometry_id) else { return };

        // Sample every bound's loop to a 3D polyline.
        let mut loops: Vec<Vec<[f64; 3]>> = Vec::new();
        let mut outer_flags: Vec<bool> = Vec::new();
        for bound in bounds {
            let Some(bound_id) = bound.as_ref() else { continue };
            let Some(bound) = spf.get(bound_id) else { continue };
            let is_outer = bound.is("FACE_OUTER_BOUND");
            let flip = matches!(bound.get(2), Some(Val::Enum(e)) if e.as_str() == "F");
            let Some(loop_id) = bound.ref_id(1) else { continue };
            let Some(loop_ent) = spf.get(loop_id) else { continue };
            let mut pts: Vec<[f64; 3]> = if loop_ent.is("POLY_LOOP") {
                loop_ent
                    .list(1)
                    .map(|list| {
                        list.iter()
                            .filter_map(|v| v.as_ref())
                            .filter_map(|id| cartesian_point(spf, id))
                            .collect()
                    })
                    .unwrap_or_default()
            } else if loop_ent.is("EDGE_LOOP") || loop_ent.is("ORIENTED_PATH") || loop_ent.is("PATH") {
                self.edge_loop_points(loop_ent)
            } else {
                Vec::new()
            };
            if pts.len() < 3 {
                continue;
            }
            if flip {
                pts.reverse();
            }
            outer_flags.push(is_outer);
            loops.push(pts);
        }
        if loops.is_empty() {
            return;
        }
        // The first bound is conventionally the outer one; trust an explicit
        // FACE_OUTER_BOUND when present, else bound order.
        if let Some(outer_index) = outer_flags.iter().position(|o| *o) {
            loops.swap(0, outer_index);
        }

        let ty = surface.ty();
        if ty == "PLANE" {
            self.mesh_planar_face(surface, &loops, sense, sink);
        } else {
            self.mesh_curved_face(surface, &loops, sense, sink);
        }
    }

    /// EDGE_LOOP → 3D polyline through the oriented edge curves.
    fn edge_loop_points(&self, loop_ent: &super::spf::Ent) -> Vec<[f64; 3]> {
        let spf: &'a Spf = self.spf;
        let Some(edges) = loop_ent.list(1) else { return Vec::new() };
        let mut pts: Vec<[f64; 3]> = Vec::new();
        for oriented in edges {
            let Some(oriented_id) = oriented.as_ref() else { continue };
            let Some(oriented) = spf.get(oriented_id) else { continue };
            let forward = !matches!(oriented.get(4), Some(Val::Enum(e)) if e.as_str() == "F");
            let Some(edge_id) = oriented.ref_id(3) else { continue };
            let Some(edge) = spf.get(edge_id) else { continue };
            if !edge.is("EDGE_CURVE") {
                continue;
            }
            let (Some(a), Some(b)) = (
                edge.ref_id(2).and_then(|id| vertex_point(spf, id)),
                edge.ref_id(1).and_then(|id| vertex_point(spf, id)),
            ) else {
                continue;
            };
            let Some(curve_id) = edge.ref_id(3) else { continue };
            let Some(curve) = spf.get(curve_id) else { continue };
            let same_sense = bool_arg(edge, 4).unwrap_or(true);
            // Arc sweep follows the curve's own direction (same_sense); the
            // oriented-edge flag then flips the walked polyline as a whole.
            let Some(mut segment) = self.sample_curve(curve, a, b, same_sense) else {
                continue;
            };
            if !forward {
                segment.reverse();
            }
            if pts
                .last()
                .map(|last| dist3(*last, segment[0]) > 1e-9)
                .unwrap_or(true)
            {
                pts.extend_from_slice(&segment);
            } else {
                pts.extend_from_slice(&segment[1..]);
            }
        }
        pts
    }

    /// Sample an edge curve between its vertex points. The polyline always
    /// runs start → end; `same_sense` picks the arc sweep direction.
    fn sample_curve(
        &self,
        curve: &super::spf::Ent,
        a: [f64; 3],
        b: [f64; 3],
        same_sense: bool,
    ) -> Option<Vec<[f64; 3]>> {
        let spf: &'a Spf = self.spf;
        let ty = curve.ty();
        if ty == "LINE" {
            return Some(vec![a, b]);
        }
        if ty == "POLYLINE" {
            let pts: Vec<[f64; 3]> = curve
                .list(1)?
                .iter()
                .filter_map(|v| v.as_ref())
                .filter_map(|id| cartesian_point(spf, id))
                .collect();
            return if pts.len() >= 2 { Some(pts) } else { None };
        }
        if ty == "CIRCLE" || ty == "ELLIPSE" {
            let frame = self.frame(curve.ref_id(1)?);
            let (r1, r2) = if ty == "CIRCLE" {
                let r = curve.num(2)?;
                (r, r)
            } else {
                (curve.num(2)?, curve.num(3)?)
            };
            let ua = angle_in_frame(&frame, a);
            let ub = angle_in_frame(&frame, b);
            let sweep = {
                let mut s = ub - ua;
                if same_sense {
                    while s <= 1e-9 {
                        s += TAU;
                    }
                } else {
                    while s >= -1e-9 {
                        s -= TAU;
                    }
                }
                s
            };
            let steps = ((sweep.abs() / TAU) * 48.0).ceil().clamp(4.0, 48.0) as usize;
            let mut pts = Vec::with_capacity(steps + 1);
            for k in 0..=steps {
                let u = ua + sweep * k as f64 / steps as f64;
                pts.push(frame_point(&frame, r1 * u.cos(), r2 * u.sin(), 0.0));
            }
            if let Some(first) = pts.first_mut() {
                *first = a;
            }
            if let Some(last) = pts.last_mut() {
                *last = b;
            }
            return Some(pts);
        }
        if ty.contains("B_SPLINE_CURVE") {
            return self.sample_bspline_curve(curve);
        }
        if ty == "TRIMMED_CURVE" {
            let basis = curve.ref_id(1).and_then(|id| spf.get(id))?;
            return self.sample_curve(basis, a, b, same_sense);
        }
        self.warn_once(format!("edge curve {ty} approximated by a chord"));
        Some(vec![a, b])
    }

    /// B-spline curve: de Boor over the file's knots, or a uniform fallback.
    fn sample_bspline_curve(&self, curve: &super::spf::Ent) -> Option<Vec<[f64; 3]>> {
        let degree = curve.int(1)?.max(1) as usize;
        let control: Vec<[f64; 4]> = curve
            .list(2)?
            .iter()
            .filter_map(|v| v.as_ref())
            .filter_map(|id| cartesian_point(self.spf, id))
            .map(|p| [p[0], p[1], p[2], 1.0])
            .collect();
        // Rational variant: the complex record concatenates a weights list.
        let weights: Option<Vec<f64>> = curve.args.iter().find_map(|arg| {
            let items = arg.as_list()?;
            if items.first().and_then(Val::as_f64).is_none() {
                return None;
            }
            Some(items.iter().filter_map(Val::as_f64).collect::<Vec<f64>>())
        });
        let n = control.len();
        if n < 2 {
            return None;
        }
        let mut control = control;
        if let Some(weights) = &weights {
            if weights.len() == n {
                for (p, w) in control.iter_mut().zip(weights) {
                    if *w != 0.0 {
                        p[0] *= w;
                        p[1] *= w;
                        p[2] *= w;
                        p[3] = *w;
                    }
                }
            }
        }
        let knots = curve_knots(curve, 6, 7, n, degree);
        let span = knot_span(&knots, degree, n);
        let steps = ((span.1 - span.0).abs() * 8.0).ceil().clamp(8.0, 96.0) as usize;
        let mut pts = Vec::with_capacity(steps + 1);
        for k in 0..=steps {
            let t = span.0 + (span.1 - span.0) * k as f64 / steps as f64;
            let p = deboor(&control, degree, &knots, t);
            pts.push(if p[3].abs() < 1e-12 {
                [p[0], p[1], p[2]]
            } else {
                [p[0] / p[3], p[1] / p[3], p[2] / p[3]]
            });
        }
        Some(pts)
    }

    /// PLANE face: exact 2D triangulation.
    fn mesh_planar_face(
        &self,
        surface: &super::spf::Ent,
        loops: &[Vec<[f64; 3]>],
        same_sense: bool,
        sink: &mut TriSink,
    ) {
        let Some(placement_id) = surface.ref_id(1) else { return };
        let frame = self.frame(placement_id);
        let flat = |p: [f64; 3]| -> [f64; 2] {
            let d = [
                p[0] - frame.origin[0],
                p[1] - frame.origin[1],
                p[2] - frame.origin[2],
            ];
            [dot3(d, frame.x), dot3(d, frame.y)]
        };
        let outer_flat: Vec<[f64; 2]> = flat_all(&loops[0], &flat);
        let holes_flat: Vec<Vec<[f64; 2]>> =
            loops[1..].iter().map(|l| flat_all(l, &flat)).collect();
        let holes: Vec<&[[f64; 2]]> = holes_flat.iter().map(|l| l.as_slice()).collect();
        let mut normal = frame.z;
        if !same_sense {
            normal = neg3(normal);
        }
        for tri in ear_clip(&outer_flat, &holes) {
            let lift = |q: [f64; 2]| frame_point(&frame, q[0], q[1], 0.0);
            let (p0, p1, p2) = (lift(tri[0]), lift(tri[1]), lift(tri[2]));
            let tn = poly_normal(&[p0, p1, p2]);
            let (p0, p1, p2) = if dot3(tn, normal) >= 0.0 {
                (p0, p1, p2)
            } else {
                (p0, p2, p1)
            };
            push_scaled(sink, p0, p1, p2, self.scale);
        }
    }

    /// Curved surfaces: parametric grid over the trim-implied range, clipped
    /// in UV against the sampled loops (even-odd fill).
    fn mesh_curved_face(
        &self,
        surface: &super::spf::Ent,
        loops: &[Vec<[f64; 3]>],
        same_sense: bool,
        sink: &mut TriSink,
    ) {
        let ty = surface.ty().to_string();
        let Some(evaluation) = SurfaceEval::build(surface, self, loops) else {
            self.warn_once(format!("surface {ty} skipped"));
            return;
        };
        let (n_u, n_v) = (28usize, 14usize);
        let mut grid: Vec<Vec<[f64; 3]>> = Vec::with_capacity(n_v + 1);
        for j in 0..=n_v {
            let v = j as f64 / n_v as f64;
            let mut row = Vec::with_capacity(n_u + 1);
            for i in 0..=n_u {
                let u = i as f64 / n_u as f64;
                match evaluation.eval(u, v) {
                    Some(p) => row.push(p),
                    None => {
                        self.warn_once(format!("surface {ty} could not be evaluated"));
                        return;
                    }
                }
            }
            grid.push(row);
        }

        // Map trim loops into UV: analytic projection when the surface allows,
        // nearest grid node otherwise.
        let uv_loops: Vec<Vec<[f64; 2]>> = loops
            .iter()
            .map(|loop_pts| {
                if let Some(project) = evaluation.projector() {
                    loop_pts
                        .iter()
                        .filter_map(|p| project(p))
                        .collect()
                } else {
                    loop_pts.iter().map(|p| nearest_grid_uv(&grid, *p)).collect()
                }
            })
            .collect();
        let references: Vec<&[[f64; 2]]> = uv_loops.iter().map(|l| l.as_slice()).collect();

        for j in 0..n_v {
            for i in 0..n_u {
                let uv_centre = [(i as f64 + 0.5) / n_u as f64, (j as f64 + 0.5) / n_v as f64];
                if !point_in_loops(uv_centre, &references) {
                    continue;
                }
                let p00 = grid[j][i];
                let p10 = grid[j][i + 1];
                let p01 = grid[j + 1][i];
                let p11 = grid[j + 1][i + 1];
                if same_sense {
                    push_scaled(sink, p00, p10, p11, self.scale);
                    push_scaled(sink, p00, p11, p01, self.scale);
                } else {
                    push_scaled(sink, p00, p11, p10, self.scale);
                    push_scaled(sink, p00, p01, p11, self.scale);
                }
            }
        }
    }

    /// AXIS2_PLACEMENT_3D → orthonormal frame.
    fn frame(&self, placement_id: u64) -> Frame {
        let spf: &'a Spf = self.spf;
        let Some(ent) = spf.get(placement_id) else {
            return Frame {
                origin: [0.0; 3],
                x: [1.0, 0.0, 0.0],
                y: [0.0, 1.0, 0.0],
                z: [0.0, 0.0, 1.0],
            };
        };
        let origin = ent
            .ref_id(1)
            .and_then(|id| cartesian_point(spf, id))
            .unwrap_or([0.0; 3]);
        let z = ent
            .ref_id(2)
            .and_then(|id| direction(spf, id))
            .and_then(|d| norm3(d))
            .unwrap_or([0.0, 0.0, 1.0]);
        let x0 = ent
            .ref_id(3)
            .and_then(|id| direction(spf, id))
            .and_then(|d| norm3(d))
            .unwrap_or([1.0, 0.0, 0.0]);
        let z = glam::DVec3::new(z[0], z[1], z[2]);
        let mut x = glam::DVec3::new(x0[0], x0[1], x0[2]) - z * z.dot(glam::DVec3::new(x0[0], x0[1], x0[2]));
        if x.length() < 1e-9 {
            x = if z.x.abs() < 0.9 {
                z.cross(glam::DVec3::X)
            } else {
                z.cross(glam::DVec3::Y)
            };
        }
        let x = x.normalize_or(glam::DVec3::X);
        let y = z.cross(x);
        Frame {
            origin,
            x: [x.x, x.y, x.z],
            y: [y.x, y.y, y.z],
            z: [z.x, z.y, z.z],
        }
    }
}

// ── parametric surface evaluation ────────────────────────────────────────

/// Evaluation over a normalised (u, v) ∈ [0,1]² domain. The domain bounds are
/// derived from the trim loops so the grid covers what the face actually uses.
enum SurfaceEval {
    Cylinder {
        frame: Frame,
        radius: f64,
        u0: f64,
        u1: f64,
        v0: f64,
        v1: f64,
    },
    Cone {
        frame: Frame,
        radius: f64,
        slope: f64,
        u0: f64,
        u1: f64,
        v0: f64,
        v1: f64,
    },
    Sphere {
        frame: Frame,
        radius: f64,
        u0: f64,
        u1: f64,
        p0: f64,
        p1: f64,
    },
    Torus {
        frame: Frame,
        major: f64,
        minor: f64,
        u0: f64,
        u1: f64,
        v0: f64,
        v1: f64,
    },
    Spline {
        net: Vec<Vec<[f64; 3]>>,
        weights: Option<Vec<Vec<f64>>>,
        degree_u: usize,
        degree_v: usize,
        knots_u: Vec<f64>,
        knots_v: Vec<f64>,
        u0: f64,
        u1: f64,
        v0: f64,
        v1: f64,
    },
}

impl SurfaceEval {
    fn build(
        surface: &super::spf::Ent,
        reader: &Reader<'_>,
        loops: &[Vec<[f64; 3]>],
    ) -> Option<SurfaceEval> {
        let ty = surface.ty();
        let placement = surface.ref_id(1)?;
        let frame = reader.frame(placement);
        let angular = |loops: &[Vec<[f64; 3]>]| -> (f64, f64) {
            let mut angles: Vec<f64> = loops
                .iter()
                .flatten()
                .map(|p| angle_in_frame(&frame, *p))
                .collect();
            if angles.is_empty() {
                return (0.0, TAU);
            }
            // Unwrap around the first sample, then see how much of the circle
            // the face covers.
            let base = angles[0];
            for a in angles.iter_mut() {
                while *a < base {
                    *a += TAU;
                }
                while *a >= base + TAU {
                    *a -= TAU;
                }
            }
            let min = angles.iter().copied().fold(f64::INFINITY, f64::min);
            let max = angles.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            if max - min > TAU * 0.9 {
                (0.0, TAU)
            } else {
                (min - 0.05, max + 0.05)
            }
        };
        let heights = |loops: &[Vec<[f64; 3]>], fallback: f64| -> (f64, f64) {
            let mut min = f64::INFINITY;
            let mut max = f64::NEG_INFINITY;
            for p in loops.iter().flatten() {
                let h = dot3(sub3(*p, frame.origin), frame.z);
                min = min.min(h);
                max = max.max(h);
            }
            if !min.is_finite() || (max - min).abs() < 1e-9 {
                return (-fallback.abs(), fallback.abs());
            }
            (min, max)
        };
        match ty {
            "CYLINDRICAL_SURFACE" => {
                let radius = surface.num(2).unwrap_or(0.0).abs();
                let (u0, u1) = angular(loops);
                let (v0, v1) = heights(loops, radius);
                Some(SurfaceEval::Cylinder {
                    frame,
                    radius,
                    u0,
                    u1,
                    v0,
                    v1,
                })
            }
            "CONICAL_SURFACE" => {
                let radius = surface.num(2).unwrap_or(0.0).abs();
                let slope = surface.num(3).unwrap_or(0.0).tan();
                let (u0, u1) = angular(loops);
                let (v0, v1) = heights(loops, radius);
                Some(SurfaceEval::Cone {
                    frame,
                    radius,
                    slope,
                    u0,
                    u1,
                    v0,
                    v1,
                })
            }
            "SPHERICAL_SURFACE" => {
                let radius = surface.num(2).unwrap_or(0.0).abs();
                let (u0, u1) = angular(loops);
                let mut p0 = f64::INFINITY;
                let mut p1 = f64::NEG_INFINITY;
                for point in loops.iter().flatten() {
                    let d = sub3(*point, frame.origin);
                    let h = (dot3(d, frame.z) / radius).clamp(-1.0, 1.0);
                    let phi = h.asin();
                    p0 = p0.min(phi);
                    p1 = p1.max(phi);
                }
                if !p0.is_finite() || (p1 - p0).abs() < 1e-6 {
                    (p0, p1) = (-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2);
                }
                Some(SurfaceEval::Sphere {
                    frame,
                    radius,
                    u0,
                    u1,
                    p0,
                    p1,
                })
            }
            "TOROIDAL_SURFACE" | "DEGENERATETOROIDAL_SURFACE" | "DEGENERATE_TOROIDAL_SURFACE" => {
                let major = surface.num(2).unwrap_or(0.0);
                let minor = surface.num(3).unwrap_or(0.0).abs();
                let (u0, u1) = angular(loops);
                // Tube angle: atan2(height, radial - major).
                let mut v0 = f64::INFINITY;
                let mut v1 = f64::NEG_INFINITY;
                for point in loops.iter().flatten() {
                    let d = sub3(*point, frame.origin);
                    let h = dot3(d, frame.z);
                    let radial = sub3(d, mul3(frame.z, h));
                    let rl = len3(radial);
                    let phi = (h).atan2(rl - major);
                    v0 = v0.min(phi);
                    v1 = v1.max(phi);
                }
                if !v0.is_finite() || (v1 - v0).abs() < 1e-6 {
                    (v0, v1) = (0.0, TAU);
                } else {
                    // Torus tube angles wrap; widen if suspiciously thin.
                    if v1 - v0 < 0.1 {
                        (v0, v1) = (v0 - 0.05, v1 + 0.05);
                    }
                }
                Some(SurfaceEval::Torus {
                    frame,
                    major,
                    minor,
                    u0,
                    u1,
                    v0,
                    v1,
                })
            }
            other if other.contains("B_SPLINE_SURFACE") => {
                let degree_u = surface.int(1)?.max(1) as usize;
                let degree_v = surface.int(2)?.max(1) as usize;
                let net: Vec<Vec<[f64; 3]>> = surface
                    .list(3)?
                    .iter()
                    .filter_map(|row| {
                        row.as_list().map(|pts| {
                            pts.iter()
                                .filter_map(|c| c.as_list())
                                .map(|c| {
                                    [
                                        c.first().and_then(Val::as_f64).unwrap_or(0.0),
                                        c.get(1).and_then(Val::as_f64).unwrap_or(0.0),
                                        c.get(2).and_then(Val::as_f64).unwrap_or(0.0),
                                    ]
                                })
                                .collect::<Vec<_>>()
                        })
                    })
                    .collect();
                if net.len() < 2 || net[0].len() < 2 {
                    return None;
                }
                let rows = net.len();
                let cols = net[0].len();
                // Rational weights live in the concatenated complex record.
                let weights: Option<Vec<Vec<f64>>> = surface.args.iter().find_map(|arg| {
                    let rows_v = arg.as_list()?;
                    let candidate: Option<Vec<Vec<f64>>> = rows_v
                        .iter()
                        .map(|row| {
                            row.as_list()
                                .map(|vals| vals.iter().filter_map(Val::as_f64).collect::<Vec<_>>())
                        })
                        .collect();
                    let ok = candidate
                        .as_ref()
                        .is_some_and(|c| c.len() == rows && c.first().map(Vec::len) == Some(cols));
                    ok.then_some(candidate)?
                });
                let knots_u = curve_knots_from(surface, 8, 10, rows, degree_u);
                let knots_v = curve_knots_from(surface, 9, 11, cols, degree_v);
                let (u0, u1) = knot_span(&knots_u, degree_u, rows);
                let (v0, v1) = knot_span(&knots_v, degree_v, cols);
                Some(SurfaceEval::Spline {
                    net,
                    weights,
                    degree_u,
                    degree_v,
                    knots_u,
                    knots_v,
                    u0,
                    u1,
                    v0,
                    v1,
                })
            }
            _ => None,
        }
    }

    fn eval(&self, un: f64, vn: f64) -> Option<[f64; 3]> {
        match self {
            SurfaceEval::Cylinder {
                frame,
                radius,
                u0,
                u1,
                v0,
                v1,
            } => {
                let theta = *u0 + (*u1 - *u0) * un;
                let h = *v0 + (*v1 - *v0) * vn;
                Some(frame_point(frame, radius * theta.cos(), radius * theta.sin(), h))
            }
            SurfaceEval::Cone {
                frame,
                radius,
                slope,
                u0,
                u1,
                v0,
                v1,
            } => {
                let theta = *u0 + (*u1 - *u0) * un;
                let h = *v0 + (*v1 - *v0) * vn;
                let r = (radius + h * slope).max(0.0);
                Some(frame_point(frame, r * theta.cos(), r * theta.sin(), h))
            }
            SurfaceEval::Sphere {
                frame,
                radius,
                u0,
                u1,
                p0,
                p1,
            } => {
                let theta = *u0 + (*u1 - *u0) * un;
                let phi = *p0 + (*p1 - *p0) * vn;
                Some(frame_point(
                    frame,
                    radius * phi.cos() * theta.cos(),
                    radius * phi.cos() * theta.sin(),
                    radius * phi.sin(),
                ))
            }
            SurfaceEval::Torus {
                frame,
                major,
                minor,
                u0,
                u1,
                v0,
                v1,
            } => {
                let theta = *u0 + (*u1 - *u0) * un;
                let phi = *v0 + (*v1 - *v0) * vn;
                let ring = major + minor * phi.cos();
                Some(frame_point(
                    frame,
                    ring * theta.cos(),
                    ring * theta.sin(),
                    minor * phi.sin(),
                ))
            }
            SurfaceEval::Spline {
                net,
                weights,
                degree_u,
                degree_v,
                knots_u,
                knots_v,
                u0,
                u1,
                v0,
                v1,
            } => {
                let u = *u0 + (*u1 - *u0) * un;
                let v = *v0 + (*v1 - *v0) * vn;
                let rows = net.len();
                let mut strip: Vec<[f64; 4]> = Vec::with_capacity(rows);
                for (j, row) in net.iter().enumerate() {
                    let weighted: Vec<[f64; 4]> = row
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let w = weights
                                .as_ref()
                                .and_then(|ws| ws.get(j))
                                .and_then(|r| r.get(i))
                                .copied()
                                .unwrap_or(1.0);
                            if w != 0.0 {
                                [p[0] * w, p[1] * w, p[2] * w, w]
                            } else {
                                [p[0], p[1], p[2], 1.0]
                            }
                        })
                        .collect();
                    if weighted.is_empty() {
                        return None;
                    }
                    strip.push(deboor(&weighted, *degree_u, knots_u, u));
                }
                if strip.is_empty() {
                    return None;
                }
                let p = deboor(&strip, *degree_v, knots_v, v);
                if p[3].abs() < 1e-12 {
                    Some([p[0], p[1], p[2]])
                } else {
                    Some([p[0] / p[3], p[1] / p[3], p[2] / p[3]])
                }
            }
        }
    }

    /// Analytic UV projection for trim clipping; `None` → nearest-grid.
    fn projector(&self) -> Option<Box<dyn Fn(&[f64; 3]) -> Option<[f64; 2]> + '_>> {
        let to_unit = |value: f64, lo: f64, hi: f64| -> Option<f64> {
            if (hi - lo).abs() < 1e-12 {
                return Some(0.0);
            }
            Some(((value - lo) / (hi - lo)).clamp(0.0, 1.0))
        };
        match self {
            SurfaceEval::Cylinder {
                frame,
                u0,
                u1,
                v0,
                v1,
                ..
            }
            | SurfaceEval::Cone {
                frame,
                u0,
                u1,
                v0,
                v1,
                ..
            } => {
                let f = frame.clone_frame();
                let (u0, u1, v0, v1) = (*u0, *u1, *v0, *v1);
                Some(Box::new(move |p: &[f64; 3]| {
                    let d = sub3(*p, f.origin);
                    let theta = dot3(d, f.y).atan2(dot3(d, f.x));
                    let u = to_unit(angle_near(theta, (u0 + u1) * 0.5), u0, u1)?;
                    let v = to_unit(dot3(d, f.z), v0, v1)?;
                    Some([u, v])
                }))
            }
            SurfaceEval::Sphere {
                frame,
                radius,
                u0,
                u1,
                p0,
                p1,
            } => {
                let f = frame.clone_frame();
                let (radius, u0, u1, p0, p1) = (*radius, *u0, *u1, *p0, *p1);
                Some(Box::new(move |p: &[f64; 3]| {
                    let d = sub3(*p, f.origin);
                    let theta = dot3(d, f.y).atan2(dot3(d, f.x));
                    let phi = (dot3(d, f.z) / radius.max(1e-9)).clamp(-1.0, 1.0).asin();
                    let u = to_unit(angle_near(theta, (u0 + u1) * 0.5), u0, u1)?;
                    let v = to_unit(phi, p0, p1)?;
                    Some([u, v])
                }))
            }
            SurfaceEval::Torus {
                frame,
                major,
                u0,
                u1,
                v0,
                v1,
                ..
            } => {
                let f = frame.clone_frame();
                let (major, u0, u1, v0, v1) = (*major, *u0, *u1, *v0, *v1);
                Some(Box::new(move |p: &[f64; 3]| {
                    let d = sub3(*p, f.origin);
                    let h = dot3(d, f.z);
                    let radial = sub3(d, mul3(f.z, h));
                    let theta = dot3(radial, f.y).atan2(dot3(radial, f.x));
                    let phi = h.atan2(len3(radial) - major);
                    let u = to_unit(angle_near(theta, (u0 + u1) * 0.5), u0, u1)?;
                    let v = to_unit(phi, v0, v1)?;
                    Some([u, v])
                }))
            }
            SurfaceEval::Spline { .. } => None,
        }
    }
}

/// Bring an angle near `reference` by removing full turns (so the u range
/// comparison lands inside the evaluated span).
fn angle_near(angle: f64, reference: f64) -> f64 {
    let mut a = angle;
    while a - reference > std::f64::consts::PI {
        a -= TAU;
    }
    while reference - a > std::f64::consts::PI {
        a += TAU;
    }
    a
}

/// Knot vector for curves/surfaces with multiplicities; uniform fallback.
fn curve_knots(ent: &super::spf::Ent, mults_idx: usize, knots_idx: usize, n: usize, degree: usize) -> Vec<f64> {
    curve_knots_from(ent, mults_idx, knots_idx, n, degree)
}

fn curve_knots_from(
    ent: &super::spf::Ent,
    mults_idx: usize,
    knots_idx: usize,
    n: usize,
    degree: usize,
) -> Vec<f64> {
    if let (Some(mults), Some(knots)) = (ent.list(mults_idx), ent.list(knots_idx)) {
        let mut out = Vec::new();
        for (knot, mult) in knots.iter().zip(mults.iter()) {
            let count = mult.as_i64().unwrap_or(1).max(0) as usize;
            let value = knot.as_f64().unwrap_or(0.0);
            for _ in 0..count {
                out.push(value);
            }
        }
        if out.len() == n + degree + 1 {
            return out;
        }
    }
    uniform_clamped_knots(n, degree)
}

fn uniform_clamped_knots(n: usize, degree: usize) -> Vec<f64> {
    let mut knots = Vec::with_capacity(n + degree + 1);
    for i in 0..=(n + degree) {
        let value = if i <= degree {
            0.0
        } else if i >= n {
            (n - degree) as f64
        } else {
            (i - degree) as f64
        };
        knots.push(value);
    }
    knots
}

/// Valid parameter span [k[degree], k[n]] for n control points.
fn knot_span(knots: &[f64], degree: usize, n: usize) -> (f64, f64) {
    let lo = knots.get(degree).copied().unwrap_or(0.0);
    let hi = knots.get(n).copied().unwrap_or(lo + 1.0);
    if hi - lo < 1e-12 {
        (lo, lo + 1.0)
    } else {
        (lo, hi)
    }
}

/// De Boor evaluation at parameter `t` (4D points carry rational weights).
fn deboor(control: &[[f64; 4]], degree: usize, knots: &[f64], t: f64) -> [f64; 4] {
    let n = control.len();
    if n == 0 {
        return [0.0; 4];
    }
    let degree = degree.min(n - 1);
    // Clamped knots: at the very end of the domain the curve is exactly the
    // last control point; the span recursion below is half-open.
    if let Some(last) = control.last() {
        if let Some(&end) = knots.get(n) {
            if t >= end - 1e-9 {
                return *last;
            }
        }
    }
    // Span index: largest k with knots[k] <= t, clamped to [degree, n-1].
    let mut k = degree;
    while k + 1 < n && knots.get(k + 1).copied().unwrap_or(f64::INFINITY) <= t {
        k += 1;
    }
    let mut d: Vec<[f64; 4]> = (0..=degree)
        .map(|j| control.get(j + k - degree).copied().unwrap_or([0.0; 4]))
        .collect();
    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let i = k - degree + j;
            let lo = knots.get(i).copied().unwrap_or(0.0);
            let hi = knots.get(i + degree + 1 - r).copied().unwrap_or(lo + 1.0);
            let alpha = if (hi - lo).abs() < 1e-12 {
                0.0
            } else {
                ((t - lo) / (hi - lo)).clamp(0.0, 1.0)
            };
            let a = d[j - 1];
            let b = d[j];
            d[j] = [
                a[0] + (b[0] - a[0]) * alpha,
                a[1] + (b[1] - a[1]) * alpha,
                a[2] + (b[2] - a[2]) * alpha,
                a[3] + (b[3] - a[3]) * alpha,
            ];
        }
    }
    d[degree]
}

#[derive(Clone, Copy)]
struct Frame {
    origin: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    z: [f64; 3],
}

impl Frame {
    fn clone_frame(&self) -> Frame {
        *self
    }
}

fn frame_point(frame: &Frame, x: f64, y: f64, z: f64) -> [f64; 3] {
    [
        frame.origin[0] + frame.x[0] * x + frame.y[0] * y + frame.z[0] * z,
        frame.origin[1] + frame.x[1] * x + frame.y[1] * y + frame.z[1] * z,
        frame.origin[2] + frame.x[2] * x + frame.y[2] * y + frame.z[2] * z,
    ]
}

fn angle_in_frame(frame: &Frame, p: [f64; 3]) -> f64 {
    let d = [
        p[0] - frame.origin[0],
        p[1] - frame.origin[1],
        p[2] - frame.origin[2],
    ];
    dot3(d, frame.y).atan2(dot3(d, frame.x))
}

fn nearest_grid_uv(grid: &[Vec<[f64; 3]>], p: [f64; 3]) -> [f64; 2] {
    let n_v = grid.len().saturating_sub(1);
    let n_u = grid.first().map(|r| r.len().saturating_sub(1)).unwrap_or(1);
    let mut best = (0usize, 0usize);
    let mut best_d = f64::INFINITY;
    for (j, row) in grid.iter().enumerate() {
        for (i, q) in row.iter().enumerate() {
            let d = dist3(*q, p);
            if d < best_d {
                best_d = d;
                best = (i, j);
            }
        }
    }
    [
        best.0 as f64 / n_u.max(1) as f64,
        best.1 as f64 / n_v.max(1) as f64,
    ]
}

// small vector/format helpers ─────────────────────────────────────────────

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn mul3(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn neg3(a: [f64; 3]) -> [f64; 3] {
    [-a[0], -a[1], -a[2]]
}

fn dist3(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn len3(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn norm3(a: [f64; 3]) -> Option<[f64; 3]> {
    let l = len3(a);
    if l < 1e-12 {
        None
    } else {
        Some([a[0] / l, a[1] / l, a[2] / l])
    }
}

fn flat_all(pts: &[[f64; 3]], flat: &impl Fn([f64; 3]) -> [f64; 2]) -> Vec<[f64; 2]> {
    pts.iter().map(|p| flat(*p)).collect()
}

fn bool_arg(ent: &super::spf::Ent, index: usize) -> Option<bool> {
    match ent.get(index) {
        Some(Val::Enum(e)) => Some(e.as_str() != "F"),
        Some(Val::Int(i)) => Some(*i != 0),
        _ => None,
    }
}

fn cartesian_point(spf: &Spf, id: u64) -> Option<[f64; 3]> {
    let ent = spf.get(id)?;
    if !ent.is("CARTESIAN_POINT") {
        return None;
    }
    let coords = ent.list(1)?;
    Some([
        coords.first().and_then(Val::as_f64).unwrap_or(0.0),
        coords.get(1).and_then(Val::as_f64).unwrap_or(0.0),
        coords.get(2).and_then(Val::as_f64).unwrap_or(0.0),
    ])
}

fn vertex_point(spf: &Spf, id: u64) -> Option<[f64; 3]> {
    let vertex = spf.get(id)?;
    if vertex.is("VERTEX_POINT") {
        vertex.ref_id(1).and_then(|id| cartesian_point(spf, id))
    } else if vertex.is("CARTESIAN_POINT") {
        cartesian_point(spf, id)
    } else {
        None
    }
}

fn direction(spf: &Spf, id: u64) -> Option<[f64; 3]> {
    let ent = spf.get(id)?;
    if !ent.is("DIRECTION") {
        return None;
    }
    let ratios = ent.list(1)?;
    Some([
        ratios.first().and_then(Val::as_f64).unwrap_or(0.0),
        ratios.get(1).and_then(Val::as_f64).unwrap_or(0.0),
        ratios.get(2).and_then(Val::as_f64).unwrap_or(0.0),
    ])
}

fn push_scaled(sink: &mut TriSink, a: [f64; 3], b: [f64; 3], c: [f64; 3], scale: f64) {
    let s = |p: [f64; 3]| [p[0] * scale, p[1] * scale, p[2] * scale];
    sink.push(s(a), s(b), s(c));
}

fn sink_to_mesh(sink: &TriSink, name: &str, color: [f32; 4], scale: f64) -> MeshModel {
    let count = sink.tris.len() * 3;
    let mut verts: Vec<[f32; 3]> = Vec::with_capacity(count);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(count);
    let mut indices: Vec<u32> = Vec::with_capacity(count);
    for tri in &sink.tris {
        let a = glam::DVec3::new(tri[0][0], tri[0][1], tri[0][2]);
        let b = glam::DVec3::new(tri[1][0], tri[1][1], tri[1][2]);
        let c = glam::DVec3::new(tri[2][0], tri[2][1], tri[2][2]);
        let n = (b - a).cross(c - a);
        let len = n.length().max(1e-12);
        let nf = [(n.x / len) as f32, (n.y / len) as f32, (n.z / len) as f32];
        let base = verts.len() as u32;
        for p in [a, b, c] {
            verts.push([(p.x * scale) as f32, (p.y * scale) as f32, (p.z * scale) as f32]);
            normals.push(nf);
        }
        indices.extend([base, base + 1, base + 2]);
    }
    MeshModel {
        name: name.to_string(),
        verts,
        verts_low: Vec::new(),
        normals,
        indices,
        triangle_material_handles: Vec::new(),
        triangle_colors: Vec::new(),
        color,
        selected: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A millimetre box: one planar face via POLY_LOOP plus the unit context.
    const BOX_STEP: &str = r"ISO-10303-21;
HEADER;
FILE_SCHEMA(('AP214IS'));
ENDSEC;
DATA;
#1=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#2,#4))LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));
#2=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));
#4=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));
#10=CARTESIAN_POINT('',(0.,0.,0.));
#11=DIRECTION('',(0.,0.,1.));
#12=DIRECTION('',(1.,0.,0.));
#13=AXIS2_PLACEMENT_3D('',#10,#11,#12);
#20=CARTESIAN_POINT('',(0.,0.,0.));
#21=CARTESIAN_POINT('',(40.,0.,0.));
#22=CARTESIAN_POINT('',(40.,30.,0.));
#23=CARTESIAN_POINT('',(0.,30.,0.));
#24=POLY_LOOP('',(#20,#21,#22,#23));
#25=FACE_OUTER_BOUND('',#24,.T.);
#40=ADVANCED_FACE('',(#25),#50,.T.);
#50=PLANE('',#13);
#60=MANIFOLD_SOLID_BREP('',#61);
#61=CLOSED_SHELL('',(#40));
#70=SHAPE_REPRESENTATION('',(#60),#1);
#80=PRODUCT('','Bracket','',$);
#81=PRODUCT_DEFINITION_FORMATION('','',#80);
#82=PRODUCT_DEFINITION('','',#81,$);
#84=PRODUCT_DEFINITION_SHAPE('','',#82);
#85=SHAPE_DEFINITION_REPRESENTATION(#84,#70);
ENDSEC;
END-ISO-10303-21;
";

    #[test]
    fn parses_units_and_a_planar_face() {
        let result = parse_step(BOX_STEP.as_bytes()).expect("parse");
        assert!(result.schema.contains("AP214"));
        let mesh = result.meshes.first().expect("mesh");
        assert!(!mesh.verts.is_empty());
        let max_x = mesh
            .verts
            .iter()
            .map(|v| v[0])
            .fold(f32::NEG_INFINITY, f32::max);
        assert!((max_x - 40.0).abs() < 1e-3, "max x {max_x}");
        assert_eq!(result.products, 1);
        assert_eq!(mesh.name, "Bracket");
    }

    #[test]
    fn deboor_at_ends_hits_control_points() {
        let ctrl = vec![[0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 0.0, 1.0], [2.0, 0.0, 0.0, 1.0]];
        let knots = uniform_clamped_knots(3, 2);
        let start = deboor(&ctrl, 2, &knots, 0.0);
        assert!((start[0]).abs() < 1e-9 && (start[1]).abs() < 1e-9);
        let end = deboor(&ctrl, 2, &knots, 1.0);
        assert!((end[0] - 2.0).abs() < 1e-9 && end[1].abs() < 1e-9);
    }
}
