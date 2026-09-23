// IFC (Industry Foundation Classes) reader — geometry, spatial structure,
// and property extraction.
//
// Built on the shared STEP Physical File parser (`super::spf`). The reading
// strategy follows ThatOpen's web-ifc (Apache-2.0): resolve placements along
// IfcLocalPlacement chains, tessellate body representations (extrusions,
// faceted breps, triangulated/polygonal face sets, mapped items), and walk
// the IFC relationship graph for structure and properties — re-implemented
// here in Rust.
//
// v1 limits (each degrades gracefully, recorded in `warnings`):
//   - Boolean subtraction (openings, clips) is skipped: only the first
//     operand of a clipping result is imported, so walls lose their window/
//     door voids.
//   - Profile curves beyond polylines/indexed curves/circles/ellipses and
//     swept solids beyond extrusions are skipped.

use super::meshutil::{ear_clip, poly_normal, TriSink};
use super::spf::{Spf, Val};
use crate::scene::model::mesh_model::MeshModel;

const DEFAULT_COLOR: [f32; 4] = [0.72, 0.72, 0.78, 1.0];
const MAX_PLACEMENT_DEPTH: usize = 64;
const MAX_ITEM_DEPTH: usize = 12;

/// One element's extracted record for the property report.
#[derive(Clone, Debug)]
pub struct IfcElement {
    pub guid: String,
    pub name: String,
    pub type_name: String,
    pub storey: String,
    /// (property set, property name, value)
    pub props: Vec<(String, String, String)>,
}

/// Result of parsing an IFC file.
#[derive(Clone, Debug)]
pub struct IfcImportResult {
    pub schema: String,
    pub meshes: Vec<MeshModel>,
    pub elements: Vec<IfcElement>,
    pub warnings: Vec<String>,
}

struct Reader<'a> {
    spf: &'a Spf,
    len_scale: f64,
    /// element entity id → spatial container path ("Project / Site / Storey")
    container: std::collections::HashMap<u64, String>,
    /// aggregate child id → parent id (site→building→storey chains)
    parents: std::collections::HashMap<u64, u64>,
    /// element entity id → property-definition entity ids
    prop_defs: std::collections::HashMap<u64, Vec<u64>>,
    /// type entity id → its own property-definition ids
    type_props: std::collections::HashMap<u64, Vec<u64>>,
    /// element entity id → relating type entity id
    element_type: std::collections::HashMap<u64, u64>,
    /// shape item entity id → RGBA colour
    colors: std::collections::HashMap<u64, [f32; 4]>,
    /// Shared so meshing can fan out across threads (rayon) on desktop.
    warnings: std::sync::Mutex<Vec<String>>,
}

/// Parse an IFC file into meshes plus extracted element records.
pub fn parse_ifc(bytes: &[u8]) -> Result<IfcImportResult, String> {
    let spf = Spf::parse(bytes)?;
    let warnings = Vec::new();
    if spf.is_empty() {
        return Err("no data entities in file".into());
    }
    let len_scale = length_scale(&spf);
    let mut reader = Reader {
        spf: &spf,
        len_scale,
        container: std::collections::HashMap::new(),
        parents: std::collections::HashMap::new(),
        prop_defs: std::collections::HashMap::new(),
        type_props: std::collections::HashMap::new(),
        element_type: std::collections::HashMap::new(),
        colors: std::collections::HashMap::new(),
        warnings: std::sync::Mutex::new(warnings),
    };
    reader.build_colors();
    reader.build_relationships();

    // Mesh every product that carries a placement and a representation.
    // Elements are independent, so meshing fans out across threads on
    // desktop builds — multi-hundred-megabyte models are the common case.
    struct Pending {
        placement_id: u64,
        rep_id: u64,
        name: String,
        color: [f32; 4],
    }
    let pending: Vec<Pending> = spf
        .iter()
        .filter(|ent| ent.args.len() >= 7 && ent.args[0].as_str().is_some())
        .filter(|ent| !ent.is("IFCOPENINGELEMENT"))
        .filter_map(|ent| {
            let placement_id = ent.ref_id(5)?;
            let rep_id = ent.ref_id(6)?;
            let placement = spf.get(placement_id)?;
            if !placement.is("IFCLOCALPLACEMENT") {
                return None;
            }
            spf.get(rep_id)?;
            let type_name = ent.ty().trim_start_matches("IFC").to_string();
            let name = element_display_name(ent, &type_name);
            let color = reader.colors.get(&ent.id).copied().unwrap_or(DEFAULT_COLOR);
            Some(Pending {
                placement_id,
                rep_id,
                name,
                color,
            })
        })
        .collect();

    let mesh_one = |item: &Pending| -> Option<MeshModel> {
        let base = reader.placement_matrix(item.placement_id, 0);
        let mut sink = TriSink::default();
        reader.mesh_representation(item.rep_id, &base, 0, &mut sink);
        eprintln!("[ifc-debug] item {} tris={}", item.name, sink.tris.len());
        if sink.tris.is_empty() {
            return None;
        }
        Some(sink_to_mesh(&sink, &item.name, item.color, reader.len_scale))
    };

    #[cfg(not(target_arch = "wasm32"))]
    let meshes: Vec<MeshModel> = {
        use rayon::prelude::*;
        pending.par_iter().filter_map(mesh_one).collect()
    };
    #[cfg(target_arch = "wasm32")]
    let meshes: Vec<MeshModel> = pending.iter().filter_map(mesh_one).collect();

    reader.warn("openings and boolean cuts are not subtracted in this version".into());

    let elements = reader.collect_elements();
    let warnings = reader.warnings.into_inner().unwrap_or_default();
    let schema = spf.schema().to_string();
    Ok(IfcImportResult {
        schema,
        meshes,
        elements,
        warnings,
    })
}

/// Build the property/quantity CSV report for an IFC file.
pub fn properties_csv_string(bytes: &[u8]) -> Result<String, String> {
    let result = parse_ifc(bytes)?;
    let mut out = String::from("GUID,Type,Name,Storey,PropertySet,Property,Value\n");
    for element in &result.elements {
        let base = [
            csv_field(&element.guid),
            csv_field(&element.type_name),
            csv_field(&element.name),
            csv_field(&element.storey),
        ]
        .join(",");
        if element.props.is_empty() {
            out.push_str(&base);
            out.push_str(",,,\n");
            continue;
        }
        for (set, prop, value) in &element.props {
            out.push_str(&base);
            out.push(',');
            out.push_str(&csv_field(set));
            out.push(',');
            out.push_str(&csv_field(prop));
            out.push(',');
            out.push_str(&csv_field(value));
            out.push('\n');
        }
    }
    Ok(out)
}

fn csv_field(text: &str) -> String {
    if text.contains(',') || text.contains('"') || text.contains('\n') {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

// ── units ────────────────────────────────────────────────────────────────

/// IFC file length unit → millimetres (the drawing-unit convention imports
/// land on; a 5 m wall arrives 5000 units long).
fn length_scale(spf: &Spf) -> f64 {
    // Prefer the project's unit assignment, fall back to the first length
    // unit in the file, and finally assume metres.
    if let Some(project) = spf.first_of_type("IFCPROJECT") {
        if let Some(assignment) = project.ent(spf, 8) {
            if let Some(units) = assignment.list(0) {
                for unit in units {
                    if let Some(id) = unit.as_ref() {
                        if let Some(scale) = unit_scale(spf, id) {
                            return scale;
                        }
                    }
                }
            }
        }
    }
    for ent in spf.iter() {
        if ent.is("LENGTH_UNIT") {
            if let Some(scale) = unit_scale(spf, ent.id) {
                return scale;
            }
        }
    }
    1000.0
}

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

fn unit_scale(spf: &Spf, unit_id: u64) -> Option<f64> {
    let ent = spf.get(unit_id)?;
    if ent.is("IFCCONVERSIONBASEDUNIT") || ent.is("CONVERSIONBASEDUNIT") {
        let name = ent
            .args
            .iter()
            .find_map(Val::as_str)
            .unwrap_or("")
            .to_uppercase();
        let factor = ent.args.iter().find_map(|v| {
            v.as_ref()
                .and_then(|id| spf.get(id))
                .filter(|m| m.is("IFCMEASUREWITHUNIT"))
                .and_then(|m| m.num(0))
        })?;
        return Some(match name.as_str() {
            "INCH" | "INCHES" => factor * 25.4,
            "FOOT" | "FEET" => factor * 304.8,
            "MILLIMETRE" | "MM" => factor,
            "METRE" | "METER" | "M" => factor * 1000.0,
            "CENTIMETRE" | "CM" => factor * 10.0,
            _ => factor,
        });
    }
    if !ent.is("LENGTH_UNIT") {
        return None;
    }
    // SI unit: gather the enumeration args; one names the measure, an
    // optional other the prefix. Complex-entity argument order varies.
    let enums: Vec<String> = ent
        .args
        .iter()
        .filter_map(Val::as_enum)
        .map(|e| e.trim_end_matches('.').to_uppercase())
        .collect();
    if !enums.iter().any(|e| e == "METRE") {
        return None;
    }
    for (prefix, scale) in SI_PREFIXES {
        if enums.iter().any(|e| e == *prefix) {
            return Some(1000.0 * scale);
        }
    }
    Some(1000.0)
}

// ── naming, relationships ────────────────────────────────────────────────

fn element_display_name(ent: &super::spf::Ent, type_name: &str) -> String {
    let name = ent.str(2).unwrap_or("").trim().to_string();
    if !name.is_empty() {
        name
    } else {
        format!("{type_name} #{}", ent.id)
    }
}

/// Display label for a spatial structure entity, cached per id.
fn container_label(spf: &Spf, id: u64, cache: &mut std::collections::HashMap<u64, String>) -> String {
    if let Some(name) = cache.get(&id) {
        return name.clone();
    }
    let path = match spf.get(id) {
        Some(ent) => {
            let name = ent.str(2).unwrap_or("").trim();
            if name.is_empty() {
                ent.ty().trim_start_matches("IFC").to_string()
            } else {
                name.to_string()
            }
        }
        None => format!("#{id}"),
    };
    cache.insert(id, path.clone());
    path
}

impl<'a> Reader<'a> {
    fn warn(&self, message: String) {
        if let Ok(mut guard) = self.warnings.lock() {
            if !guard.contains(&message) {
                guard.push(message);
            }
        }
    }

    /// Styled-item colours: IfcStyledItem(Item, Styles) → surface style →
    /// rendering → colour RGB.
    fn build_colors(&mut self) {
        for ent in self.spf.iter() {
            if !ent.is("IFCSTYLEDITEM") {
                continue;
            }
            let Some(item) = ent.ref_id(0) else { continue };
            let Some(styles) = ent.list(1) else { continue };
            for style_ref in styles {
                let Some(style_id) = style_ref.as_ref() else { continue };
                let Some(style) = self.spf.get(style_id) else { continue };
                if !(style.is("IFCSURFACESTYLE")) {
                    continue;
                }
                let Some(subs) = style.list(1) else { continue };
                for sub in subs {
                    let Some(sub_id) = sub.as_ref() else { continue };
                    let Some(rendering) = self.spf.get(sub_id) else { continue };
                    if !rendering.is("IFCSURFACESTYLERENDERING") {
                        continue;
                    }
                    let Some(colour_id) = rendering.ref_id(0) else { continue };
                    let Some(colour) = self.spf.get(colour_id) else { continue };
                    if !colour.is("IFCCOLOURRGB") {
                        continue;
                    }
                    let r = colour.num(1).unwrap_or(0.0) as f32;
                    let g = colour.num(2).unwrap_or(0.0) as f32;
                    let b = colour.num(3).unwrap_or(0.0) as f32;
                    self.colors.insert(item, [r, g, b, 1.0]);
                }
            }
        }
    }

    fn build_relationships(&mut self) {
        use std::collections::HashMap;
        // Aggregates: project → site → building → storey → (space) chains.
        for ent in self.spf.iter() {
            if ent.is("IFCRELAGGREGATES") {
                if let (Some(parent), Some(related)) = (ent.ref_id(4), ent.list(5)) {
                    for child in related.iter().filter_map(Val::as_ref) {
                        self.parents.insert(child, parent);
                    }
                }
            }
        }
        // Containment: element → its spatial structure container.
        // IfcRelContainedInSpatialStructure(RelatedElements, RelatingStructure).
        let mut contained: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
        for ent in self.spf.iter() {
            if ent.is("IFCRELCONTAINEDINSPATIALSTRUCTURE") {
                if let (Some(structure), Some(elements)) = (ent.ref_id(5), ent.list(4)) {
                    for element in elements.iter().filter_map(Val::as_ref) {
                        contained.insert(element, structure);
                    }
                }
            }
        }
        // Resolve a display path for each referenced container.
        let mut names: HashMap<u64, String> = HashMap::new();
        for (element, structure) in &contained {
            let mut chain = vec![container_label(self.spf, *structure, &mut names)];
            let mut cursor = *structure;
            for _ in 0..4 {
                let Some(parent) = self.parents.get(&cursor) else { break };
                chain.push(container_label(self.spf, *parent, &mut names));
                cursor = *parent;
            }
            chain.reverse();
            self.container.insert(*element, chain.join(" / "));
        }

        // Property relations.
        for ent in self.spf.iter() {
            if ent.is("IFCRELDEFINESBYPROPERTIES") {
                let Some(defs) = ent.list(4) else { continue };
                let Some(definition) = ent.ref_id(5) else { continue };
                for object in defs.iter().filter_map(Val::as_ref) {
                    self.prop_defs.entry(object).or_default().push(definition);
                }
            } else if ent.is("IFCRELDEFINESBYTYPE") {
                if let (Some(relating), Some(related)) = (ent.ref_id(4), ent.list(5)) {
                    for object in related.iter().filter_map(Val::as_ref) {
                        self.element_type.insert(object, relating);
                    }
                }
            } else if ent.is("IFCTYPEPRODUCT") {
                if let Some(sets) = ent.list(5) {
                    let ids: Vec<u64> = sets.iter().filter_map(Val::as_ref).collect();
                    if !ids.is_empty() {
                        self.type_props.insert(ent.id, ids);
                    }
                }
            }
        }
    }

    fn collect_elements(&self) -> Vec<IfcElement> {
        let mut elements = Vec::new();
        for ent in self.spf.iter() {
            // Element-like products: guid in slot 0, and known to the
            // structure or property graph (keeps units/contexts out).
            let guid = ent.str(0).unwrap_or("");
            if guid.is_empty() {
                continue;
            }
            let is_element = self.container.contains_key(&ent.id)
                || self.prop_defs.contains_key(&ent.id)
                || ent.is("IFCWALL")
                || ent.is("IFCWALLSTANDARDCASE")
                || ent.is("IFCSLAB")
                || ent.is("IFCCOLUMN")
                || ent.is("IFCBEAM")
                || ent.is("IFCDOOR")
                || ent.is("IFCWINDOW")
                || ent.is("IFCSPACE")
                || ent.is("IFCSTAIR")
                || ent.is("IFCROOF")
                || ent.is("IFCMEMBER")
                || ent.is("IFCPLATE")
                || ent.is("IFCFOOTING")
                || ent.is("IFCPILE")
                || ent.is("IFCFURNISHINGELEMENT")
                || ent.is("IFCCOVERING")
                || ent.is("IFCCURTAINWALL");
            if !is_element {
                continue;
            }
            let type_name = ent.ty().trim_start_matches("IFC").to_string();
            let name = element_display_name(ent, &type_name);
            let storey = self
                .container
                .get(&ent.id)
                .cloned()
                .unwrap_or_default();
            let mut props: Vec<(String, String, String)> = Vec::new();
            if let Some(defs) = self.prop_defs.get(&ent.id) {
                for def in defs {
                    self.collect_property_set(self.spf.get(*def), &mut props);
                }
            }
            if let Some(type_id) = self.element_type.get(&ent.id) {
                let type_ent = self.spf.get(*type_id);
                if let Some(type_ent) = type_ent {
                    let type_label = type_ent.str(2).unwrap_or(type_ent.ty());
                    props.push((
                        "Type".into(),
                        "Type name".into(),
                        type_label.to_string(),
                    ));
                }
                if let Some(sets) = self.type_props.get(type_id) {
                    for set in sets {
                        self.collect_property_set(self.spf.get(*set), &mut props);
                    }
                }
            }
            elements.push(IfcElement {
                guid: guid.to_string(),
                name,
                type_name,
                storey,
                props,
            });
        }
        elements
    }

    fn collect_property_set(&self, def: Option<&super::spf::Ent>, props: &mut Vec<(String, String, String)>) {
        let Some(def) = def else { return };
        if def.is("IFCPROPERTYSET") {
            let set_name = def.str(2).unwrap_or("Property set").to_string();
            let Some(items) = def.list(4) else { return };
            for item in items {
                let Some(id) = item.as_ref() else { continue };
                let Some(prop) = self.spf.get(id) else { continue };
                if prop.is("IFCPROPERTYSINGLEVALUE") {
                    let name = prop.str(0).unwrap_or("");
                    let value = self.value_string(prop.get(2));
                    props.push((set_name.clone(), name.to_string(), value));
                } else if prop.is("IFCPROPERTYENUMERATEDVALUE") {
                    let name = prop.str(0).unwrap_or("");
                    let value = prop
                        .list(2)
                        .map(|vals| {
                            vals.iter()
                                .map(|v| self.value_string(Some(v)))
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default();
                    props.push((set_name.clone(), name.to_string(), value));
                } else if prop.is("IFCPROPERTYLISTVALUE") {
                    let name = prop.str(0).unwrap_or("");
                    let value = prop
                        .list(2)
                        .map(|vals| {
                            vals.iter()
                                .map(|v| self.value_string(Some(v)))
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default();
                    props.push((set_name.clone(), name.to_string(), value));
                } else if prop.is("IFCPROPERTYBOUNDEDVALUE") {
                    let name = prop.str(0).unwrap_or("");
                    let upper = self.value_string(prop.get(2));
                    let lower = self.value_string(prop.get(3));
                    props.push((
                        set_name.clone(),
                        name.to_string(),
                        format!("{lower} … {upper}"),
                    ));
                }
            }
        } else if def.is("IFCELEMENTQUANTITY") {
            let set_name = def.str(2).unwrap_or("Quantities").to_string();
            let Some(items) = def.list(5) else { return };
            for item in items {
                let Some(id) = item.as_ref() else { continue };
                let Some(q) = self.spf.get(id) else { continue };
                let is_quantity = q.is("IFCQUANTITYLENGTH")
                    || q.is("IFCQUANTITYAREA")
                    || q.is("IFCQUANTITYVOLUME")
                    || q.is("IFCQUANTITYWEIGHT")
                    || q.is("IFCQUANTITYCOUNT")
                    || q.is("IFCQUANTITYTIME")
                    || q.is("IFCQUANTITYPERIMETER")
                    || q.is("IFCQUANTITYNUMBER");
                if is_quantity {
                    let name = q.str(0).unwrap_or("");
                    let value = q
                        .num(3)
                        .map(format_measure)
                        .unwrap_or_else(|| "-".into());
                    props.push((set_name.clone(), name.to_string(), value));
                }
            }
        }
    }

    fn value_string(&self, value: Option<&Val>) -> String {
        match value {
            None | Some(Val::Unset) | Some(Val::Star) => "-".into(),
            Some(Val::Str(s)) => s.clone(),
            Some(Val::Enum(e)) => match e.as_str() {
                "T" | "TRUE" => "TRUE".into(),
                "F" | "FALSE" => "FALSE".into(),
                other => other.trim_end_matches('.').to_string(),
            },
            Some(Val::Int(i)) => i.to_string(),
            Some(Val::Num(n)) => format_measure(*n),
            Some(Val::List(items)) => items
                .iter()
                .map(|v| self.value_string(Some(v)))
                .collect::<Vec<_>>()
                .join(", "),
            Some(Val::Typed(_, inner)) => self.value_string(Some(inner)),
            Some(Val::Ref(id)) => format!("#{id}"),
        }
    }

    // ── geometry ─────────────────────────────────────────────────────────

    /// Resolve an IfcLocalPlacement chain into a world matrix (file units).
    fn placement_matrix(&self, placement_id: u64, depth: usize) -> glam::DMat4 {
        if depth > MAX_PLACEMENT_DEPTH {
            return glam::DMat4::IDENTITY;
        }
        let Some(placement) = self.spf.get(placement_id) else {
            return glam::DMat4::IDENTITY;
        };
        let parent = placement
            .ref_id(0)
            .map(|id| self.placement_matrix(id, depth + 1))
            .unwrap_or(glam::DMat4::IDENTITY);
        let relative = placement
            .ref_id(1)
            .and_then(|id| self.spf.get(id))
            .map(|axis| self.axis_placement_matrix(axis))
            .unwrap_or(glam::DMat4::IDENTITY);
        parent * relative
    }

    fn axis_placement_matrix(&self, axis: &super::spf::Ent) -> glam::DMat4 {
        if axis.is("IFCAXIS2PLACEMENT2D") {
            let origin = axis
                .ref_id(0)
                .and_then(|id| self.point(id))
                .unwrap_or([0.0, 0.0, 0.0]);
            let mut x = axis
                .ref_id(1)
                .and_then(|id| self.direction(id))
                .map(|d| glam::DVec3::new(d[0], d[1], 0.0))
                .unwrap_or(glam::DVec3::X);
            if x.length() < 1e-9 {
                x = glam::DVec3::X;
            }
            let y = glam::DVec3::new(-x.y, x.x, 0.0);
            return glam::DMat4::from_cols(
                x.extend(0.0),
                y.extend(0.0),
                glam::DVec3::Z.extend(0.0),
                glam::DVec3::new(origin[0], origin[1], origin[2]).extend(1.0),
            );
        }
        // IfcAxis2Placement3D
        let origin = axis
            .ref_id(0)
            .and_then(|id| self.point(id))
            .unwrap_or([0.0, 0.0, 0.0]);
        let z = axis
            .ref_id(1)
            .and_then(|id| self.direction(id))
            .map(|d| glam::DVec3::new(d[0], d[1], d[2]))
            .unwrap_or(glam::DVec3::Z);
        let z = if z.length() < 1e-9 { glam::DVec3::Z } else { z.normalize() };
        let x0 = axis
            .ref_id(2)
            .and_then(|id| self.direction(id))
            .map(|d| glam::DVec3::new(d[0], d[1], d[2]))
            .unwrap_or_else(|| {
                if z.x.abs() < 0.9 {
                    glam::DVec3::X
                } else {
                    glam::DVec3::Y
                }
            });
        let mut x = x0 - z * x0.dot(z);
        if x.length() < 1e-9 {
            x = if z.x.abs() < 0.9 {
                z.cross(glam::DVec3::X)
            } else {
                z.cross(glam::DVec3::Y)
            };
        }
        let x = x.normalize_or(x0);
        let y = z.cross(x);
        glam::DMat4::from_cols(
            x.extend(0.0),
            y.extend(0.0),
            z.extend(0.0),
            glam::DVec3::new(origin[0], origin[1], origin[2]).extend(1.0),
        )
    }

    fn point(&self, id: u64) -> Option<[f64; 3]> {
        let ent = self.spf.get(id)?;
        if !ent.is("IFCCARTESIANPOINT") {
            return None;
        }
        let coords = ent.list(0)?;
        Some([
            coords.first().and_then(Val::as_f64).unwrap_or(0.0),
            coords.get(1).and_then(Val::as_f64).unwrap_or(0.0),
            coords.get(2).and_then(Val::as_f64).unwrap_or(0.0),
        ])
    }

    fn direction(&self, id: u64) -> Option<[f64; 3]> {
        let ent = self.spf.get(id)?;
        if !ent.is("IFCDIRECTION") {
            return None;
        }
        let ratios = ent.list(0)?;
        Some([
            ratios.first().and_then(Val::as_f64).unwrap_or(0.0),
            ratios.get(1).and_then(Val::as_f64).unwrap_or(0.0),
            ratios.get(2).and_then(Val::as_f64).unwrap_or(0.0),
        ])
    }

    /// Mesh an IfcShapeRepresentation (or any representation) into `sink`.
    fn mesh_representation(
        &self,
        rep_id: u64,
        xform: &glam::DMat4,
        depth: usize,
        sink: &mut TriSink,
    ) {
        if depth > MAX_ITEM_DEPTH {
            return;
        }
        let spf: &'a Spf = self.spf;
        let Some(rep) = spf.get(rep_id) else { return };
        let items_idx = if rep.is("IFCSHAPEREPRESENTATION") { 3 } else { rep.args.len().saturating_sub(1) };
        let Some(items) = rep.list(items_idx) else { return };
        for item in items {
            let Some(item_id) = item.as_ref() else { continue };
            // IfcProductRepresentation lists nested representations rather
            // than shape items — recurse into those.
            let nested = spf
                .get(item_id)
                .map(|e| e.ty().ends_with("REPRESENTATION"))
                .unwrap_or(false);
            if nested {
                self.mesh_representation(item_id, xform, depth + 1, sink);
            } else {
                self.mesh_item(item_id, xform, depth, sink);
            }
        }
    }

    fn mesh_item(
        &self,
        item_id: u64,
        xform: &glam::DMat4,
        depth: usize,
        sink: &mut TriSink,
    ) {
        if depth > MAX_ITEM_DEPTH {
            return;
        }
        let spf: &'a Spf = self.spf;
        let Some(ent) = spf.get(item_id) else { return };
        if ent.is("IFCEXTRUDEDAREASOLID") {
            self.mesh_extrusion(ent, xform, sink);
        } else if ent.is("IFCFACETEDBREP") || ent.is("IFCBREPWITHVOIDS") {
            let Some(shell_id) = ent.ref_id(0) else { return };
            self.mesh_connected_face_set(shell_id, xform, sink);
        } else if ent.is("IFCSHELLBASEDSURFACEMODEL") {
            if let Some(shells) = ent.list(0) {
                for shell in shells {
                    if let Some(shell_id) = shell.as_ref() {
                        self.mesh_connected_face_set(shell_id, xform, sink);
                    }
                }
            }
        } else if ent.is("IFCTRIANGULATEDFACESET") {
            self.mesh_triangulated_face_set(ent, xform, sink);
        } else if ent.is("IFCPOLYGONALFACESET") {
            self.mesh_polygonal_face_set(ent, xform, sink);
        } else if ent.is("IFCMAPPEDITEM") {
            self.mesh_mapped_item(ent, xform, depth, sink);
        } else if ent.is("IFCBOOLEANCLIPPINGRESULT") || ent.is("IFCBOOLEANRESULT") {
            if let Some(first) = ent.ref_id(1) {
                self.mesh_item(first, xform, depth + 1, sink);
            }
        } else if ent.is("IFCSOLIDMODEL") || ent.is("IFCHALFSPACESOLID") {
            self.warn("unsupported solid representation skipped".into());
        }
    }

    fn mesh_mapped_item(
        &self,
        ent: &super::spf::Ent,
        parent: &glam::DMat4,
        depth: usize,
        sink: &mut TriSink,
    ) {
        let spf: &'a Spf = self.spf;
        let Some(source_id) = ent.ref_id(0) else { return };
        let Some(source) = spf.get(source_id) else { return };
        if !source.is("IFCREPRESENTATIONMAP") {
            return;
        }
        let target = ent
            .ref_id(1)
            .and_then(|id| spf.get(id))
            .map(|op| self.transform_operator_matrix(op))
            .unwrap_or(glam::DMat4::IDENTITY);
        let origin = source
            .get(0)
            .and_then(Val::as_ref)
            .and_then(|id| spf.get(id))
            .map(|axis| self.axis_placement_matrix(axis))
            .unwrap_or(glam::DMat4::IDENTITY);
        let mapped = *parent * target * origin;
        if let Some(rep_id) = source.ref_id(1) {
            self.mesh_representation(rep_id, &mapped, depth + 1, sink);
        }
    }

    fn transform_operator_matrix(&self, op: &super::spf::Ent) -> glam::DMat4 {
        let origin = op
            .ref_id(2)
            .and_then(|id| self.point(id))
            .unwrap_or([0.0, 0.0, 0.0]);
        let scale = op.num(3).unwrap_or(1.0);
        if scale.abs() < 1e-12 {
            return glam::DMat4::IDENTITY;
        }
        let sx = scale;
        let sy = op.num(4).unwrap_or(scale);
        let sz = op.num(5).unwrap_or(scale);
        let axis1 = op.ref_id(0).and_then(|id| self.direction(id));
        let axis2 = op.ref_id(1).and_then(|id| self.direction(id));
        let axis3 = op.ref_id(4).and_then(|id| self.direction(id));
        let z = axis3
            .map(|d| glam::DVec3::new(d[0], d[1], d[2]))
            .unwrap_or(glam::DVec3::Z)
            .normalize_or(glam::DVec3::Z);
        let x = axis1
            .map(|d| glam::DVec3::new(d[0], d[1], d[2]))
            .map(|v| v - z * v.dot(z))
            .unwrap_or_else(|| {
                if z.x.abs() < 0.9 {
                    glam::DVec3::X - z * z.x
                } else {
                    glam::DVec3::Y - z * z.y
                }
            })
            .normalize_or(glam::DVec3::X);
        let y_raw = axis2
            .map(|d| glam::DVec3::new(d[0], d[1], d[2]))
            .unwrap_or_else(|| z.cross(x));
        let y = y_raw - z * y_raw.dot(z);
        let y = y.normalize_or(z.cross(x));
        glam::DMat4::from_cols(
            (x * sx).extend(0.0),
            (y * sy).extend(0.0),
            (z * sz).extend(0.0),
            glam::DVec3::new(origin[0], origin[1], origin[2]).extend(1.0),
        )
    }

    /// IfcExtrudedAreaSolid: sweep the profile along a direction.
    fn mesh_extrusion(&self, ent: &super::spf::Ent, parent: &glam::DMat4, sink: &mut TriSink) {
        let spf: &'a Spf = self.spf;
        let Some(profile_id) = ent.ref_id(0) else { return };
        let Some(profile) = spf.get(profile_id) else { return };
        let Some(mut loops) = self.profile_loops(profile) else { return };
        // Profile placement (origin + in-plane rotation), applied to every ring.
        if let Some(pos_id) = profile.ref_id(2) {
            if let Some(pos) = spf.get(pos_id) {
                let m = self.axis_placement_matrix(pos);
                for loop_pts in &mut loops {
                    for p in loop_pts.iter_mut() {
                        let v = m.transform_point3(glam::DVec3::new(p[0], p[1], 0.0));
                        *p = [v.x, v.y];
                    }
                }
            }
        }
        let position = ent
            .ref_id(1)
            .and_then(|id| self.spf.get(id))
            .map(|axis| self.axis_placement_matrix(axis))
            .unwrap_or(glam::DMat4::IDENTITY);
        let dir = ent
            .ref_id(2)
            .and_then(|id| self.direction(id))
            .map(|d| {
                let v = glam::DVec3::new(d[0], d[1], d[2]);
                v.normalize_or(glam::DVec3::Z)
            })
            .unwrap_or(glam::DVec3::Z);
        let depth = ent.num(3).unwrap_or(0.0);
        if depth.abs() < 1e-12 {
            return;
        }
        let m = *parent * position;
        let lift = |p: [f64; 2]| -> [f64; 3] {
            let v = m.transform_point3(glam::DVec3::new(p[0], p[1], 0.0));
            [v.x, v.y, v.z]
        };
        let bottom: Vec<Vec<[f64; 3]>> = loops
            .iter()
            .map(|l| l.iter().map(|p| lift(*p)).collect())
            .collect();
        let offset = glam::DVec3::new(dir.x, dir.y, dir.z) * depth;
        let top: Vec<Vec<[f64; 3]>> = bottom
            .iter()
            .map(|l| {
                l.iter()
                    .map(|p| [p[0] + offset.x, p[1] + offset.y, p[2] + offset.z])
                    .collect()
            })
            .collect();

        // Sides: every ring (outer and voids alike) contributes walls.
        for (bi, ring) in bottom.iter().enumerate() {
            let top_ring = &top[bi];
            let n = ring.len();
            for k in 0..n {
                let a0 = ring[k];
                let b0 = ring[(k + 1) % n];
                let a1 = top_ring[k];
                let b1 = top_ring[(k + 1) % n];
                sink.push(a0, b0, b1);
                sink.push(a0, b1, a1);
            }
        }
        // Caps: triangulate in profile space, lift each triangle.
        let outer = &loops[0];
        let holes: Vec<&[[f64; 2]]> = loops[1..].iter().map(|l| l.as_slice()).collect();
        for tri in ear_clip(outer, &holes) {
            let a = lift(tri[0]);
            let b = lift(tri[1]);
            let c = lift(tri[2]);
            let n = poly_normal(&[a, b, c]);
            // Bottom cap faces -dir, top cap faces +dir: match windings.
            let up = [dir.x, dir.y, dir.z];
            if dot3(n, up) >= 0.0 {
                // Ear-clip winding follows +dir: flip the bottom, keep the top.
                sink.push(a, c, b);
                sink.push(add3(a, offset), add3(b, offset), add3(c, offset));
            } else {
                sink.push(a, b, c);
                sink.push(add3(a, offset), add3(c, offset), add3(b, offset));
            }
        }
    }

    /// Profile → 2D rings (outer first, then voids) in profile coordinates.
    fn profile_loops(&self, profile: &super::spf::Ent) -> Option<Vec<Vec<[f64; 2]>>> {
        let ty = profile.ty();
        match ty {
            "IFCRECTANGLEPROFILEDEF" | "IFCRECTANGLEHOLLOWPROFILEDEF" => {
                let x = profile.num(3).unwrap_or(0.0) / 2.0;
                let y = profile.num(4).unwrap_or(0.0) / 2.0;
                let mut loops = vec![vec![
                    [-x, -y],
                    [x, -y],
                    [x, y],
                    [-x, y],
                ]];
                if ty.ends_with("HOLLOWPROFILEDEF") {
                    let t = profile.num(5).unwrap_or(0.0);
                    let ix = (x - t).max(0.0);
                    let iy = (y - t).max(0.0);
                    loops.push(vec![
                        [-ix, -iy],
                        [ix, -iy],
                        [ix, iy],
                        [-ix, iy],
                    ]);
                }
                Some(loops)
            }
            "IFCCIRCLEPROFILEDEF" => {
                let r = profile.num(3).unwrap_or(0.0);
                Some(vec![sample_circle(
                    [0.0, 0.0],
                    r.max(0.0),
                    36,
                )])
            }
            "IFCCIRCLEHOLLOWPROFILEDEF" => {
                let r = profile.num(3).unwrap_or(0.0);
                let t = profile.num(4).unwrap_or(0.0);
                Some(vec![
                    sample_circle([0.0, 0.0], r.max(0.0), 36),
                    sample_circle([0.0, 0.0], (r - t).max(0.0), 36),
                ])
            }
            "IFCTRAPEZIUMPROFILEDEF" => {
                let bottom = profile.num(3).unwrap_or(0.0);
                let top = profile.num(4).unwrap_or(0.0);
                let y = profile.num(5).unwrap_or(0.0);
                let x_off = profile.num(6).unwrap_or(0.0);
                Some(vec![vec![
                    [-bottom / 2.0, -y / 2.0],
                    [bottom / 2.0, -y / 2.0],
                    [x_off + top / 2.0, y / 2.0],
                    [x_off - top / 2.0, y / 2.0],
                ]])
            }
            "IFCISHAPEPROFILEDEF" => {
                let w = profile.num(3).unwrap_or(0.0);
                let d = profile.num(4).unwrap_or(0.0);
                let tw = profile.num(5).unwrap_or(0.0);
                let tf = profile.num(6).unwrap_or(0.0);
                Some(vec![i_shaped(w, d, tw, tf)])
            }
            "IFCLSHAPEPROFILEDEF" => {
                let d = profile.num(3).unwrap_or(0.0);
                let w = profile.num(4).unwrap_or(0.0);
                let t = profile.num(5).unwrap_or(0.0);
                Some(vec![vec![
                    [-w / 2.0, -d / 2.0],
                    [w / 2.0, -d / 2.0],
                    [w / 2.0, -d / 2.0 + t],
                    [-w / 2.0 + t, -d / 2.0 + t],
                    [-w / 2.0 + t, d / 2.0],
                    [-w / 2.0, d / 2.0],
                ]])
            }
            "IFCARBITRARYCLOSEDPROFILEDEF" => {
                let curve = profile.ref_id(2)?;
                let poly = self.profile_curve_points(curve)?;
                Some(vec![poly])
            }
            "IFCARBITRARYPROFILEDEFWITHVOIDS" => {
                let curve = profile.ref_id(2)?;
                let outer = self.profile_curve_points(curve)?;
                let mut loops = vec![outer];
                if let Some(voids) = profile.list(3) {
                    for void in voids {
                        if let Some(void_id) = void.as_ref() {
                            if let Some(hole) = self.profile_curve_points(void_id) {
                                loops.push(hole);
                            }
                        }
                    }
                }
                Some(loops)
            }
            other => {
                self.warn(format!("profile {other} skipped"));
                None
            }
        }
    }

    /// A profile boundary curve → 2D polyline.
    fn profile_curve_points(&self, curve_id: u64) -> Option<Vec<[f64; 2]>> {
        let curve = self.spf.get(curve_id)?;
        if curve.is("IFCPOLYLINE") {
            let pts = curve.list(0)?;
            return Some(
                pts.iter()
                    .filter_map(|v| v.as_ref())
                    .filter_map(|id| self.point(id))
                    .map(|p| [p[0], p[1]])
                    .collect(),
            );
        }
        if curve.is("IFCINDEXEDPOLYCURVE") {
            let list_id = curve.ref_id(0)?;
            let list = self.spf.get(list_id)?;
            let coords = list.list(0)?;
            let pts: Vec<[f64; 2]> = coords
                .iter()
                .filter_map(|v| v.as_list())
                .map(|pair| {
                    [
                        pair.first().and_then(Val::as_f64).unwrap_or(0.0),
                        pair.get(1).and_then(Val::as_f64).unwrap_or(0.0),
                    ]
                })
                .collect();
            let Some(segments) = curve.list(1) else {
                return Some(pts);
            };
            if segments.is_empty() {
                return Some(pts);
            }
            let mut out: Vec<[f64; 2]> = Vec::new();
            for segment in segments {
                let indices = segment.as_list()?;
                if indices.len() == 3 {
                    // Arc through three 1-based indices.
                    let pick = |i: i64| -> [f64; 2] {
                        let k = (i.max(1) - 1) as usize;
                        pts.get(k).copied().unwrap_or([0.0, 0.0])
                    };
                    let (a, m, b) = (pick(indices[0].as_i64()?), pick(indices[1].as_i64()?), pick(indices[2].as_i64()?));
                    append_arc(&mut out, a, m, b, 12);
                } else {
                    for v in indices.iter().skip(1) {
                        let Some(i) = v.as_i64() else { continue };
                        let k = (i.max(1) - 1) as usize;
                        out.push(pts.get(k).copied().unwrap_or([0.0, 0.0]));
                    }
                }
            }
            return Some(out);
        }
        if curve.is("IFCCIRCLE") {
            let center = curve
                .ref_id(0)
                .and_then(|id| self.point(id))
                .unwrap_or([0.0, 0.0, 0.0]);
            let r = curve.num(1).unwrap_or(0.0);
            return Some(sample_circle([center[0], center[1]], r, 36));
        }
        if curve.is("IFCELLIPSE") {
            let center = curve
                .ref_id(0)
                .and_then(|id| self.point(id))
                .unwrap_or([0.0, 0.0, 0.0]);
            let a = curve.num(1).unwrap_or(0.0);
            let b = curve.num(2).unwrap_or(0.0);
            let mut out = Vec::with_capacity(49);
            for k in 0..48 {
                let t = k as f64 / 48.0 * std::f64::consts::TAU;
                out.push([center[0] + a * t.cos(), center[1] + b * t.sin()]);
            }
            return Some(out);
        }
        self.warn(format!("curve {} skipped in profile", curve.ty()));
        None
    }

    /// IfcFacetedBrep / shell-based models: a connected face set of polygon
    /// loops. Advanced (curved) faces inside IFC shells fall back to their
    /// loop polygons — a coarse but useful outline.
    fn mesh_connected_face_set(
        &self,
        shell_id: u64,
        xform: &glam::DMat4,
        sink: &mut TriSink,
    ) {
        let Some(shell) = self.spf.get(shell_id) else { return };
        let Some(faces) = shell.list(0) else { return };
        for face in faces {
            let Some(face_id) = face.as_ref() else { continue };
            let Some(face_ent) = self.spf.get(face_id) else { continue };
            let Some(bounds) = face_ent.list(0) else { continue };
            let mut loops: Vec<Vec<[f64; 3]>> = Vec::new();
            for bound in bounds {
                let Some(bound_ref) = bound.as_ref() else { continue };
                let Some(bound_ent) = self.spf.get(bound_ref) else { continue };
                let flip = matches!(bound_ent.get(1), Some(Val::Enum(e)) if e.as_str() == "F");
                let Some(loop_id) = bound_ent.ref_id(0) else { continue };
                let Some(loop_ent) = self.spf.get(loop_id) else { continue };
                let mut pts: Vec<[f64; 3]> = if loop_ent.is("IFCPOLYLOOP") {
                    loop_ent
                        .list(0)
                        .map(|list| {
                            list.iter()
                                .filter_map(|v| v.as_ref())
                                .filter_map(|id| self.point(id))
                                .collect()
                        })
                        .unwrap_or_default()
                } else if loop_ent.is("IFCEDGELOOP") || loop_ent.is("IFCVERTEXLOOP") {
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
                loops.push(pts);
            }
            if loops.is_empty() {
                continue;
            }
            push_planar_loops(&loops, xform, sink);
        }
    }

    /// Walk an IfcEdgeLoop through oriented edge curves (lines/arcs) to 3D
    /// points — used by IFC files that carry B-rep style loops.
    fn edge_loop_points(&self, loop_ent: &super::spf::Ent) -> Vec<[f64; 3]> {
        let Some(oriented) = loop_ent.list(0) else { return Vec::new() };
        let mut pts: Vec<[f64; 3]> = Vec::new();
        for oedge in oriented {
            let Some(oid) = oedge.as_ref() else { continue };
            let Some(oent) = self.spf.get(oid) else { continue };
            // IfcOrientedEdge(Edge, Orientation): .F. walks the edge backwards.
            let forward =
                !matches!(oent.get(1), Some(Val::Enum(e)) if e.as_str() == "F");
            let Some(edge_id) = oent.ref_id(0) else { continue };
            let Some(edge) = self.spf.get(edge_id) else { continue };
            let v0 = edge.ref_id(0).and_then(|id| self.vertex_point(id));
            let v1 = edge.ref_id(1).and_then(|id| self.vertex_point(id));
            let (Some(a), Some(b)) = (v0, v1) else { continue };
            let (a, b) = if forward { (a, b) } else { (b, a) };
            if pts
                .last()
                .map(|last| dist3(*last, a) > 1e-9)
                .unwrap_or(true)
            {
                pts.push(a);
            }
            pts.push(b);
        }
        pts
    }

    fn vertex_point(&self, id: u64) -> Option<[f64; 3]> {
        let vertex = self.spf.get(id)?;
        if vertex.is("IFCVERTEXPOINT") {
            vertex.ref_id(0).and_then(|id| self.point(id))
        } else {
            None
        }
    }

    fn mesh_triangulated_face_set(
        &self,
        ent: &super::spf::Ent,
        xform: &glam::DMat4,
        sink: &mut TriSink,
    ) {
        let Some(list_id) = ent.ref_id(0) else { return };
        let Some(list) = self.spf.get(list_id) else { return };
        let Some(coords) = list.list(0) else { return };
        let vertices: Vec<[f64; 3]> = coords
            .iter()
            .filter_map(|v| v.as_list())
            .map(|c| {
                [
                    c.first().and_then(Val::as_f64).unwrap_or(0.0),
                    c.get(1).and_then(Val::as_f64).unwrap_or(0.0),
                    c.get(2).and_then(Val::as_f64).unwrap_or(0.0),
                ]
            })
            .collect();
        let pn: Vec<usize> = ent
            .get(4)
            .and_then(Val::as_list)
            .map(|l| {
                l.iter()
                    .filter_map(Val::as_i64)
                    .map(|i| (i.max(1) - 1) as usize)
                    .collect()
            })
            .unwrap_or_default();
        let Some(triangles) = ent.list(3) else { return };
        for tri in triangles {
            let Some(idx) = tri.as_list() else { continue };
            if idx.len() < 3 {
                continue;
            }
            let pick = |raw: &Val| -> Option<[f64; 3]> {
                let i = raw.as_i64()? - 1;
                let i = i.max(0) as usize;
                let i = if pn.is_empty() { i } else { pn.get(i).copied().unwrap_or(0) };
                vertices.get(i).copied()
            };
            let (Some(a), Some(b), Some(c)) = (
                pick(&idx[0]),
                pick(&idx[1]),
                pick(&idx[2]),
            ) else {
                continue;
            };
            push_tri(sink, xform, a, b, c);
        }
    }

    fn mesh_polygonal_face_set(
        &self,
        ent: &super::spf::Ent,
        xform: &glam::DMat4,
        sink: &mut TriSink,
    ) {
        let Some(list_id) = ent.ref_id(0) else { return };
        let Some(list) = self.spf.get(list_id) else { return };
        let Some(coords) = list.list(0) else { return };
        let vertices: Vec<[f64; 3]> = coords
            .iter()
            .filter_map(|v| v.as_list())
            .map(|c| {
                [
                    c.first().and_then(Val::as_f64).unwrap_or(0.0),
                    c.get(1).and_then(Val::as_f64).unwrap_or(0.0),
                    c.get(2).and_then(Val::as_f64).unwrap_or(0.0),
                ]
            })
            .collect();
        let pn: Vec<usize> = ent
            .get(3)
            .and_then(Val::as_list)
            .map(|l| {
                l.iter()
                    .filter_map(Val::as_i64)
                    .map(|i| (i.max(1) - 1) as usize)
                    .collect()
            })
            .unwrap_or_default();
        let Some(faces) = ent.list(2) else { return };
        for face in faces {
            let Some(face_id) = face.as_ref() else { continue };
            let Some(face_ent) = self.spf.get(face_id) else { continue };
            let pick_index = |raw: i64| -> Option<usize> {
                let i = (raw.max(1) - 1) as usize;
                if pn.is_empty() {
                    Some(i)
                } else {
                    pn.get(i).copied()
                }
            };
            let Some(indexes) = face_ent.list(0) else { continue };
            let outer: Vec<[f64; 3]> = indexes
                .iter()
                .filter_map(Val::as_i64)
                .filter_map(pick_index)
                .filter_map(|i| vertices.get(i).copied())
                .collect();
            if outer.len() < 3 {
                continue;
            }
            let mut loops = vec![outer];
            if let Some(voids) = face_ent.list(1) {
                for void in voids {
                    let Some(void_indexes) = void.as_list() else { continue };
                    let ring: Vec<[f64; 3]> = void_indexes
                        .iter()
                        .filter_map(Val::as_i64)
                        .filter_map(pick_index)
                        .filter_map(|i| vertices.get(i).copied())
                        .collect();
                    if ring.len() >= 3 {
                        loops.push(ring);
                    }
                }
            }
            push_planar_loops(&loops, xform, sink);
        }
    }
}

// ── free helpers ─────────────────────────────────────────────────────────

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn add3(a: [f64; 3], d: glam::DVec3) -> [f64; 3] {
    [a[0] + d.x, a[1] + d.y, a[2] + d.z]
}

fn dist3(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn format_measure(value: f64) -> String {
    if (value - value.round()).abs() < 1e-9 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.3}")
    }
}

fn sample_circle(center: [f64; 2], radius: f64, segments: usize) -> Vec<[f64; 2]> {
    let segments = segments.max(8);
    let mut pts = Vec::with_capacity(segments);
    for k in 0..segments {
        let t = k as f64 / segments as f64 * std::f64::consts::TAU;
        pts.push([center[0] + radius * t.cos(), center[1] + radius * t.sin()]);
    }
    pts
}

/// Arc through three points (start, on-curve, end), appended without the
/// duplicate start point.
fn append_arc(out: &mut Vec<[f64; 2]>, a: [f64; 2], m: [f64; 2], b: [f64; 2], segments: usize) {
    // Circumcentre of the three points.
    let ax = a[0] - m[0];
    let ay = a[1] - m[1];
    let bx = b[0] - m[0];
    let by = b[1] - m[1];
    let d = 2.0 * (ax * by - ay * bx);
    if d.abs() < 1e-12 {
        // Collinear: a straight run.
        if out.last().map(|p| *p != a).unwrap_or(true) {
            out.push(a);
        }
        out.push(m);
        out.push(b);
        return;
    }
    let a2 = ax * ax + ay * ay;
    let b2 = bx * bx + by * by;
    let cx = m[0] + (by * a2 - ay * b2) / d;
    let cy = m[1] + (ax * b2 - bx * a2) / d;
    let radius = ((a[0] - cx).powi(2) + (a[1] - cy).powi(2)).sqrt();
    let angle = |p: [f64; 2]| (p[1] - cy).atan2(p[0] - cx);
    let a0 = angle(a);
    let mut am = angle(m);
    let mut a1 = angle(b);
    // Unwrap so the sweep passes through the mid point.
    let tau = std::f64::consts::TAU;
    while am < a0 {
        am += tau;
    }
    while a1 < am {
        a1 += tau;
    }
    let steps = segments.max(4);
    let mut previous = out.last().copied();
    for k in 0..=steps {
        let t = a0 + (a1 - a0) * k as f64 / steps as f64;
        let p = [cx + radius * t.cos(), cy + radius * t.sin()];
        if previous.map(|q| q != p).unwrap_or(true) {
            out.push(p);
            previous = Some(p);
        }
    }
}

fn i_shaped(w: f64, d: f64, tw: f64, tf: f64) -> Vec<[f64; 2]> {
    let hw = w / 2.0;
    let hd = d / 2.0;
    let half_web = tw / 2.0;
    let inner_d = hd - tf;
    vec![
        [-hw, -hd],
        [hw, -hd],
        [hw, -inner_d],
        [half_web, -inner_d],
        [half_web, inner_d],
        [hw, inner_d],
        [hw, hd],
        [-hw, hd],
        [-hw, inner_d],
        [-half_web, inner_d],
        [-half_web, -inner_d],
        [-hw, -inner_d],
    ]
}

fn push_tri(sink: &mut TriSink, xform: &glam::DMat4, a: [f64; 3], b: [f64; 3], c: [f64; 3]) {
    let t = |p: [f64; 3]| {
        let v = xform.transform_point3(glam::DVec3::new(p[0], p[1], p[2]));
        [v.x, v.y, v.z]
    };
    sink.push(t(a), t(b), t(c));
}

/// Triangulate planar 3D loops (outer first, then voids) and push the result.
fn push_planar_loops(loops: &[Vec<[f64; 3]>], xform: &glam::DMat4, sink: &mut TriSink) {
    let outer = &loops[0];
    let normal = poly_normal(outer);
    // 2D basis in the loop plane.
    let n = glam::DVec3::new(normal[0], normal[1], normal[2]);
    let helper = if n.x.abs() < 0.9 {
        glam::DVec3::X
    } else {
        glam::DVec3::Y
    };
    let u = n.cross(helper).normalize_or(glam::DVec3::X);
    let v = n.cross(u);
    let origin = glam::DVec3::new(outer[0][0], outer[0][1], outer[0][2]);
    let flat = |pts: &[[f64; 3]]| -> Vec<[f64; 2]> {
        pts.iter()
            .map(|p| {
                let d = glam::DVec3::new(p[0], p[1], p[2]) - origin;
                [d.dot(u), d.dot(v)]
            })
            .collect()
    };
    let flat_outer = flat(outer);
    let flat_holes: Vec<Vec<[f64; 2]>> = loops[1..].iter().map(|l| flat(l)).collect();
    let holes: Vec<&[[f64; 2]]> = flat_holes.iter().map(|l| l.as_slice()).collect();
    for tri in ear_clip(&flat_outer, &holes) {
        // Reconstruct the 3D points from their in-plane coordinates.
        let lift = |q: [f64; 2]| -> [f64; 3] {
            let v = origin + u * q[0] + v * q[1];
            [v.x, v.y, v.z]
        };
        let (a, b, c) = (lift(tri[0]), lift(tri[1]), lift(tri[2]));
        let tn = poly_normal(&[a, b, c]);
        if dot3(tn, normal) >= 0.0 {
            push_tri(sink, xform, a, b, c);
        } else {
            push_tri(sink, xform, a, c, b);
        }
    }
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
        indices.push(base);
        indices.push(base + 1);
        indices.push(base + 2);
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

    const SAMPLE: &str = r"ISO-10303-21;
HEADER;
FILE_DESCRIPTION((''),'2;1');
FILE_NAME('wall.ifc','2026-01-01',('t'),('t'),'x','x','');
FILE_SCHEMA(('IFC4'));
ENDSEC;
DATA;
#1=IFCPROJECT('0gu$uq9LrC$BpZyzcX1r2s',$,'Demo',$,$,$,(#3),#4,$);
#2=IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Body','Model',*,*,*,*,#3,$,.MODEL_VIEW.,$);
#3=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#5,$);
#4=IFCUNITASSIGNMENT((#6));
#5=IFCAXIS2PLACEMENT3D(#7,$,$);
#6=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));
#7=IFCCARTESIANPOINT((0.,0.,0.));
#8=IFCSITE('0site',$,'Plot',$,$,#9,$,$,.ELEMENT.,$,$,0.,$,$);
#9=IFCLOCALPLACEMENT($,#5);
#10=IFCBUILDING('0bldg',$,'Tower',$,$,#11,$,$,.ELEMENT.,$,$,$);
#11=IFCLOCALPLACEMENT(#9,#5);
#12=IFCBUILDINGSTOREY('0stor',$,'Floor 1',$,$,#13,$,$,.ELEMENT.,0.);
#13=IFCLOCALPLACEMENT(#11,#5);
#20=IFCWALL('0wall1',$,'Wall A',$,$,#21,#30,$);
#21=IFCLOCALPLACEMENT(#13,#22);
#22=IFCAXIS2PLACEMENT3D(#23,$,$);
#23=IFCCARTESIANPOINT((1000.,2000.,0.));
#30=IFCPRODUCTREPRESENTATION($,$,(#31));
#31=IFCSHAPEREPRESENTATION(#2,'Body','SweptSolid',(#32));
#32=IFCEXTRUDEDAREASOLID(#33,#34,#35,3000.);
#33=IFCRECTANGLEPROFILEDEF(.AREA.,$,#34,200.,400.);
#34=IFCAXIS2PLACEMENT3D(#7,$,$);
#35=IFCDIRECTION((0.,0.,1.));
#40=IFCRELAGGREGATES('r1',$,$,$,#1,(#8));
#41=IFCRELAGGREGATES('r2',$,$,$,#8,(#10));
#42=IFCRELAGGREGATES('r3',$,$,$,#10,(#12));
#50=IFCRELCONTAINEDINSPATIALSTRUCTURE('r4',$,$,$,(#20),#12);
#60=IFCRELDEFINESBYPROPERTIES('r5',$,$,$,(#20),#61);
#61=IFCPROPERTYSET('ps1',$,'Pset_WallCommon',$,(#62,#63));
#62=IFCPROPERTYSINGLEVALUE('FireRating',$,IFCLABEL('REI 60'),$);
#63=IFCPROPERTYSINGLEVALUE('IsExternal',$,IFCBOOLEAN(.T.),$);
ENDSEC;
END-ISO-10303-21;
";

    #[test]
    fn imports_a_wall_with_structure_and_properties() {
        let result = parse_ifc(SAMPLE.as_bytes()).expect("parse");
        assert_eq!(result.schema, "IFC4");
        // FILE units are millimetres, so no rescaling happens.
        let mesh = result.meshes.first().expect("wall mesh");
        assert!(!mesh.verts.is_empty());
        // Wall sits at (1000, 2000, 0) and is 400 tall after the profile's
        // 400 Y-dimension gets extruded 3000 along Z… profile XY: 200 wide,
        // 400 deep, 3000 high.
        let max_z = mesh.verts.iter().map(|v| v[2]).fold(f32::NEG_INFINITY, f32::max);
        let min_z = mesh.verts.iter().map(|v| v[2]).fold(f32::INFINITY, f32::min);
        assert!((max_z - 3000.0).abs() < 1.0, "max z {max_z}");
        assert!(min_z.abs() < 1.0, "min z {min_z}");
        let element = result.elements.iter().find(|e| e.name == "Wall A").expect("element");
        assert_eq!(element.storey, "Demo / Plot / Tower / Floor 1");
        assert!(element
            .props
            .iter()
            .any(|(s, p, v)| s == "Pset_WallCommon" && p == "FireRating" && v == "REI 60"));
        assert!(element
            .props
            .iter()
            .any(|(_, p, v)| p == "IsExternal" && v == "TRUE"));
    }

    #[test]
    fn csv_report_includes_properties() {
        let csv = properties_csv_string(SAMPLE.as_bytes()).expect("csv");
        assert!(csv.starts_with("GUID,Type,Name,Storey,PropertySet,Property,Value"));
        assert!(csv.contains("FireRating"));
        assert!(csv.contains("REI 60"));
    }

    /// Large-file sanity run against the generated fixture
    /// (`scripts\gen_big_ifc.rs` output). Ignored by default:
    /// `cargo test --lib io::ifc -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn parses_the_generated_medium_fixture() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("scripts/testdata/medium.ifc");
        let Ok(bytes) = std::fs::read(&path) else {
            eprintln!("fixture missing at {} — generate it first", path.display());
            return;
        };
        let started = std::time::Instant::now();
        let result = parse_ifc(&bytes).expect("parse");
        println!(
            "medium.ifc: {} MB parsed in {:.1}s — {} meshes, {} elements, {} warnings",
            bytes.len() / (1024 * 1024),
            started.elapsed().as_secs_f32(),
            result.meshes.len(),
            result.elements.len(),
            result.warnings.len(),
        );
        assert!(result.meshes.len() >= 19_000, "expected most walls to mesh");
    }
}
