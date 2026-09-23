// IFC4 writer — serialises imported/edited elements back into an
// ISO-10303-21 exchange file.
//
// Dependency-free on purpose (std only): the round trip starts from flat
// triangle soup in millimetres plus flat property rows — exactly what the
// importer produced. The writer rebuilds the whole spatial chain
// (IfcProject → IfcSite → IfcBuilding → one IfcBuildingStorey per distinct
// storey name), attaches each element to its storey with
// IfcRelContainedInSpatialStructure, gives every element an IfcFacetedBrep
// ('Body' / 'Facetation') with deduplicated corners, and exposes property
// rows as IfcPropertySet / IfcPropertySingleValue through
// IfcRelDefinesByProperties. Lengths are written in the file's unit, the
// millimetre, so geometry passes through unscaled.
//
// Deliberate fallbacks, so one malformed record can never abort an export:
//   - Class names outside the IfcElement allowlist become
//     IFCBUILDINGELEMENTPROXY.
//   - An empty GlobalId is replaced by a deterministic 22-character id.
//   - An empty storey name puts the element into a storey named "Level 1".
//   - Degenerate triangles (quantised-equal corners) are dropped; an
//     element left without any face is written without a representation.
//   - Very large elements stop at MAX_FACES_PER_ELEMENT so a pathological
//     mesh cannot flood the file.
//
// The unit tests validate output by parsing it back through `super::spf` —
// the same tolerant reader the import path uses.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// One element to export.
#[derive(Clone, Debug, Default)]
pub struct IfcExportElement {
    /// IFC GlobalId (22-character code or any non-empty string; written as-is).
    pub guid: String,
    /// e.g. "IFCWALL", "IFCPIPESEGMENT" — without leading '#'; upper-case.
    pub ifc_class: String,
    pub name: String,
    /// Building storey display name ("" → assigned to the "Level 1" storey).
    pub storey: String,
    /// Flat triangle vertices in MILLIMETRES, world coordinates, 3 per triangle.
    pub verts: Vec<[f64; 3]>,
    /// (property set, property name, value) rows — emitted as IfcPropertySets.
    pub props: Vec<(String, String, String)>,
}

/// The model to export.
#[derive(Clone, Debug, Default)]
pub struct IfcExportModel {
    pub elements: Vec<IfcExportElement>,
}

/// Storey assigned to elements that carry no storey name.
const DEFAULT_STOREY: &str = "Level 1";

/// Common IFC4 IfcElement subclasses accepted as element classes; anything
/// else falls back to IFCBUILDINGELEMENTPROXY.
const ELEMENT_CLASSES: &[&str] = &[
    "IFCWALL",
    "IFCWALLSTANDARDCASE",
    "IFCBEAM",
    "IFCBEAMSTANDARDCASE",
    "IFCCOLUMN",
    "IFCSLAB",
    "IFCMEMBER",
    "IFCPLATE",
    "IFCFOOTING",
    "IFCPILE",
    "IFCROOF",
    "IFCSTAIR",
    "IFCSTAIRFLIGHT",
    "IFCRAILING",
    "IFCDOOR",
    "IFCWINDOW",
    "IFCCURTAINWALL",
    "IFCCOVERING",
    "IFCCHIMNEY",
    "IFCFLOWSEGMENT",
    "IFCFLOWFITTING",
    "IFCFLOWTERMINAL",
    "IFCFLOWCONTROLLER",
    "IFCPIPESEGMENT",
    "IFCFURNISHINGELEMENT",
    "IFCBUILDINGELEMENTPROXY",
    "IFCELEMENTASSEMBLY",
    "IFCREINFORCINGBAR",
    "IFCREINFORCINGMESH",
    "IFCTENDON",
    "IFCTENDONANCHOR",
    "IFCMECHANICALFASTENER",
    "IFCVOIDINGFEATURE",
];

/// Face ceiling per element. Each face costs at most six ids (three corner
/// points + polyloop + outer bound + face), so this bounds how many entities
/// a single element can contribute to the file.
const MAX_FACES_PER_ELEMENT: usize = 50_000;

/// Write `model` as a complete ISO-10303-21 / IFC4 exchange file. Returns
/// `Err` when there is nothing to write.
pub fn write_ifc(model: &IfcExportModel) -> Result<String, String> {
    if model.elements.is_empty() {
        return Err("no elements to export".to_string());
    }

    let mut w = SpfWriter::new();
    let mut guid_serial: u64 = 0;

    // --- units and the 3D representation context --------------------------
    // Geometry is authored in millimetres; the length unit below says so and
    // coordinates are written unchanged.
    let origin = w.emit(format!(
        "IFCCARTESIANPOINT(({}))",
        [0.0, 0.0, 0.0].map(fmt_real).join(",")
    ));
    let world = w.emit(format!("IFCAXIS2PLACEMENT3D({origin},$,$)"));
    let ctx = w.emit(format!(
        "IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,{},{},$)",
        fmt_real(1.0e-5),
        world
    ));
    let len_unit = w.emit("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))".to_string());
    let area_unit = w.emit("(AREA_UNIT()NAMED_UNIT(*)SI_UNIT($,.SQUARE_METRE.))".to_string());
    let vol_unit = w.emit("(VOLUME_UNIT()NAMED_UNIT(*)SI_UNIT($,.CUBIC_METRE.))".to_string());
    let angle_unit = w.emit("(PLANE_ANGLE_UNIT()NAMED_UNIT(*)SI_UNIT($,.RADIAN.))".to_string());
    let units = w.emit(format!(
        "IFCUNITASSIGNMENT(({len_unit},{area_unit},{vol_unit},{angle_unit}))"
    ));

    // --- project / site / building ----------------------------------------
    // Every placement below is the identity, and each spatial level anchors
    // on the previous one, so world-space vertex coordinates stay valid.
    guid_serial += 1;
    let g = step_string(&synthetic_guid(guid_serial));
    let project = w.emit(format!(
        "IFCPROJECT({g},$,'Project',$,$,$,$,({ctx}),{units})"
    ));

    let site_axis = w.emit(format!("IFCAXIS2PLACEMENT3D({origin},$,$)"));
    let site_plc = w.emit(format!("IFCLOCALPLACEMENT($,{site_axis})"));
    guid_serial += 1;
    let g = step_string(&synthetic_guid(guid_serial));
    let site = w.emit(format!("IFCSITE({g},$,'Site',$,$,{site_plc},$,$,$,$,$,$,$,$)"));

    let building_axis = w.emit(format!("IFCAXIS2PLACEMENT3D({origin},$,$)"));
    let building_plc = w.emit(format!("IFCLOCALPLACEMENT({site_plc},{building_axis})"));
    guid_serial += 1;
    let g = step_string(&synthetic_guid(guid_serial));
    let building = w.emit(format!(
        "IFCBUILDING({g},$,'Building',$,$,{building_plc},$,$,$,$,$,$)"
    ));

    // --- storeys, in first-seen order --------------------------------------
    let mut storey_names: Vec<String> = Vec::new();
    let mut storey_index: HashMap<String, usize> = HashMap::new();
    let mut storey_of_element: Vec<usize> = Vec::with_capacity(model.elements.len());
    for el in &model.elements {
        let trimmed = el.storey.trim();
        let name = if trimmed.is_empty() {
            DEFAULT_STOREY.to_string()
        } else {
            trimmed.to_string()
        };
        let idx = match storey_index.get(&name) {
            Some(&i) => i,
            None => {
                storey_names.push(name.clone());
                storey_index.insert(name, storey_names.len() - 1);
                storey_names.len() - 1
            }
        };
        storey_of_element.push(idx);
    }

    let mut storey_ref: Vec<String> = Vec::with_capacity(storey_names.len());
    let mut storey_plc_ref: Vec<String> = Vec::with_capacity(storey_names.len());
    for name in &storey_names {
        let axis = w.emit(format!("IFCAXIS2PLACEMENT3D({origin},$,$)"));
        let plc = w.emit(format!("IFCLOCALPLACEMENT({building_plc},{axis})"));
        guid_serial += 1;
        let g = step_string(&synthetic_guid(guid_serial));
        let label = step_string(name);
        storey_ref.push(w.emit(format!(
            "IFCBUILDINGSTOREY({g},$,{label},$,$,{plc},$,{label},$,$)"
        )));
        storey_plc_ref.push(plc);
    }

    // --- elements: placement, faceted brep, entity --------------------------
    let mut element_ref: Vec<String> = Vec::with_capacity(model.elements.len());
    for (i, el) in model.elements.iter().enumerate() {
        let class = resolve_class(&el.ifc_class);
        let guid = if el.guid.trim().is_empty() {
            guid_serial += 1;
            step_string(&synthetic_guid(guid_serial))
        } else {
            // Written as-is; escaped only for the exchange syntax.
            step_string(&el.guid)
        };
        let name = step_string(&el.name);

        let axis = w.emit(format!("IFCAXIS2PLACEMENT3D({origin},$,$)"));
        let placement = w.emit(format!(
            "IFCLOCALPLACEMENT({rel},{axis})",
            rel = storey_plc_ref[storey_of_element[i]],
        ));

        let shape = match write_brep(&mut w, &el.verts) {
            Some(brep) => {
                let repr = w.emit(format!(
                    "IFCSHAPEREPRESENTATION({ctx},'Body','Facetation',({brep}))"
                ));
                w.emit(format!("IFCPRODUCTDEFINITIONSHAPE($,$,({repr}))"))
            }
            None => "$".to_string(), // nothing survived the degenerate filter
        };

        // IfcRoot + IfcObject + IfcProduct give the eight attributes up to
        // Tag; IFC4 adds optional trailing attributes per class, written $.
        let mut args = format!("{guid},$,{name},$,$,{placement},{shape},{name}");
        for _ in 8..class_attr_count(class) {
            args.push_str(",$");
        }
        element_ref.push(w.emit(format!("{class}({args})")));
    }

    // --- containment: elements grouped per storey ---------------------------
    for (si, name) in storey_names.iter().enumerate() {
        let members: Vec<&str> = element_ref
            .iter()
            .zip(&storey_of_element)
            .filter(|&(_, st)| *st == si)
            .map(|(r, _)| r.as_str())
            .collect();
        if members.is_empty() {
            continue; // cannot happen today — storeys only exist on demand
        }
        guid_serial += 1;
        let g = step_string(&synthetic_guid(guid_serial));
        let label = step_string(name);
        w.emit(format!(
            "IFCRELCONTAINEDINSPATIALSTRUCTURE({g},$,{label},$,{members},{storey})",
            members = format!("({})", members.join(",")),
            storey = storey_ref[si],
        ));
    }

    // --- aggregate chain: project → site → building → storeys ---------------
    guid_serial += 1;
    let g = step_string(&synthetic_guid(guid_serial));
    w.emit(format!("IFCRELAGGREGATES({g},$,$,$,{project},({site}))"));
    guid_serial += 1;
    let g = step_string(&synthetic_guid(guid_serial));
    w.emit(format!("IFCRELAGGREGATES({g},$,$,$,{site},({building}))"));
    guid_serial += 1;
    let g = step_string(&synthetic_guid(guid_serial));
    w.emit(format!(
        "IFCRELAGGREGATES({g},$,$,$,{building},({}))",
        storey_ref.join(",")
    ));

    // --- property sets -------------------------------------------------------
    for (i, el) in model.elements.iter().enumerate() {
        if el.props.is_empty() {
            continue;
        }
        // Group rows by property set name, keeping first-seen set order.
        let mut set_order: Vec<String> = Vec::new();
        let mut rows_of_set: HashMap<String, Vec<usize>> = HashMap::new();
        for (ri, (set, _, _)) in el.props.iter().enumerate() {
            let trimmed = set.trim();
            let key = if trimmed.is_empty() {
                "Pset_Common".to_string()
            } else {
                trimmed.to_string()
            };
            rows_of_set
                .entry(key.clone())
                .or_insert_with(|| {
                    set_order.push(key);
                    Vec::new()
                })
                .push(ri);
        }

        let mut set_refs: Vec<String> = Vec::new();
        for set_name in &set_order {
            let mut prop_refs: Vec<String> = Vec::new();
            for ri in &rows_of_set[set_name] {
                let (prop, value) = {
                    let (_, p, v) = &el.props[*ri]; // (.0 set, .1 name, .2 value)
                    (p, v)
                };
                // Numeric strings become a length measure, everything else a
                // label; both travel as typed IfcValue selects.
                let nominal = match value.trim().parse::<f64>() {
                    Ok(v) => format!("IFCLENGTHMEASURE({})", fmt_real(v)),
                    Err(_) => format!("IFCLABEL({})", step_string(value)),
                };
                prop_refs.push(w.emit(format!(
                    "IFCPROPERTYSINGLEVALUE({},$,{nominal},$)",
                    step_string(prop)
                )));
            }
            guid_serial += 1;
            let g = step_string(&synthetic_guid(guid_serial));
            set_refs.push(w.emit(format!(
                "IFCPROPERTYSET({g},$,{set},$,{props})",
                set = step_string(set_name),
                props = format!("({})", prop_refs.join(","))
            )));
        }

        guid_serial += 1;
        let g = step_string(&synthetic_guid(guid_serial));
        let sets = set_refs.join(",");
        let element = &element_ref[i];
        w.emit(format!(
            "IFCRELDEFINESBYPROPERTIES({g},$,$,$,({element}),({sets}))"
        ));
    }

    // --- file assembly --------------------------------------------------------
    let body_len: usize = w.lines.iter().map(|l| l.len() + 1).sum();
    let mut out = String::with_capacity(1024 + body_len);
    out.push_str("ISO-10303-21;\n");
    out.push_str("HEADER;\n");
    out.push_str("FILE_DESCRIPTION(('ViewDefinition [DesignTransferView_V1.0]'),'2;1');\n");
    out.push_str(&format!(
        "FILE_NAME('model.ifc','{}',('OpenCADStudio'),('OpenCADStudio'),'OpenCADStudio IFC4 writer','OpenCADStudio','');\n",
        iso_utc_now()
    ));
    out.push_str("FILE_SCHEMA(('IFC4'));\n");
    out.push_str("ENDSEC;\n");
    out.push_str("DATA;\n");
    for line in &w.lines {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("ENDSEC;\n");
    out.push_str("END-ISO-10303-21;\n");
    Ok(out)
}

/// Sequential entity emitter: hands out `#1..#N` in write order, never
/// reusing an id.
struct SpfWriter {
    lines: Vec<String>,
    next_id: u64,
}

impl SpfWriter {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            next_id: 1,
        }
    }

    /// Append one record; `rhs` is everything after `=` — an entity keyword
    /// with its argument list, or a complex-entity `(A(..)B(..))` body.
    /// Returns the `#id` reference string.
    fn emit(&mut self, rhs: String) -> String {
        let id = self.next_id;
        self.next_id += 1;
        self.lines.push(format!("#{id}={rhs};"));
        format!("#{id}")
    }
}

/// Map a raw class name to an allowlisted entity keyword. Tolerates case and
/// a stray leading '#'; a missing `IFC` prefix is added before falling back.
fn resolve_class(raw: &str) -> &'static str {
    let cleaned = raw.trim().trim_start_matches('#').to_ascii_uppercase();
    if let Some(hit) = ELEMENT_CLASSES.iter().copied().find(|c| *c == cleaned.as_str()) {
        return hit;
    }
    let prefixed = format!("IFC{cleaned}");
    if let Some(hit) = ELEMENT_CLASSES.iter().copied().find(|c| *c == prefixed.as_str()) {
        return hit;
    }
    "IFCBUILDINGELEMENTPROXY"
}

/// Total IFC4 attribute count for a written element entity. The eight
/// inherited Root/Object/Product attributes through Tag are always written;
/// the padding covers each class's optional trailing attributes so the slot
/// count matches the IFC4 schema.
fn class_attr_count(class: &str) -> usize {
    match class {
        // + PredefinedType, CompositionType
        "IFCBUILDINGELEMENTPROXY" => 10,
        // + PredefinedType, NumberOfRiser, NumberOfTreads, RiserHeight, TreadLength
        "IFCSTAIRFLIGHT" => 13,
        // + PredefinedType
        _ => 9,
    }
}

/// Build an IfcFacetedBrep from flat triangle soup: deduplicate corners on
/// 1e-4 mm quantised keys, drop quantised-degenerate triangles, and wrap
/// each surviving triangle as IfcFace / IfcFaceOuterBound / IfcPolyLoop
/// inside one IfcClosedShell. Returns the brep's `#id`, or `None` when no
/// valid triangle is left — the element is then written without a
/// representation.
fn write_brep(w: &mut SpfWriter, verts: &[[f64; 3]]) -> Option<String> {
    let mut points: HashMap<(i64, i64, i64), String> = HashMap::new();
    let mut faces: Vec<String> = Vec::new();

    for tri in verts.chunks_exact(3) {
        if faces.len() >= MAX_FACES_PER_ELEMENT {
            break; // pathological-input guard — see MAX_FACES_PER_ELEMENT
        }
        let keys = [quantise(tri[0]), quantise(tri[1]), quantise(tri[2])];
        if keys[0] == keys[1] || keys[1] == keys[2] || keys[0] == keys[2] {
            continue; // a polyloop needs three distinct corners
        }
        let mut corners: Vec<String> = Vec::with_capacity(3);
        for i in 0..3 {
            let r = match points.get(&keys[i]) {
                Some(r) => r.clone(),
                None => {
                    let r = w.emit(format!(
                        "IFCCARTESIANPOINT(({}))",
                        tri[i].map(fmt_real).join(",")
                    ));
                    points.insert(keys[i], r.clone());
                    r
                }
            };
            corners.push(r);
        }
        let pl = w.emit(format!(
            "IFCPOLYLOOP(({},{},{}))",
            corners[0], corners[1], corners[2]
        ));
        let bound = w.emit(format!("IFCFACEOUTERBOUND({pl},.T.)"));
        faces.push(w.emit(format!("IFCFACE({bound})")));
    }

    if faces.is_empty() {
        return None;
    }
    let shell = w.emit(format!("IFCCLOSEDSHELL({})", faces.join(",")));
    Some(w.emit(format!("IFCFACETEDBREP({shell})")))
}

/// Vertex dedup key: coordinates quantised to 1e-4 mm buckets. `as i64`
/// saturates instead of panicking on extreme or non-finite values.
fn quantise(v: [f64; 3]) -> (i64, i64, i64) {
    (
        (v[0] * 10_000.0).round() as i64,
        (v[1] * 10_000.0).round() as i64,
        (v[2] * 10_000.0).round() as i64,
    )
}

/// Deterministic 22-character IFC Base64 GlobalId for structural entities
/// (and for elements exported without one). Charset per the IFC spec:
/// 0-9 A-Z a-z _ $.
fn synthetic_guid(serial: u64) -> String {
    const ALPHABET: &[u8; 64] =
        b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";
    let mut chars = ['0'; 22];
    let mut v = serial;
    for slot in (0..22).rev() {
        chars[slot] = ALPHABET[(v % 64) as usize] as char;
        v /= 64;
    }
    chars.into_iter().collect()
}

/// Encode a Rust string as a STEP string literal: quotes and backslashes are
/// doubled, and everything outside printable ASCII travels as a `\X2\`
/// UTF-16BE hex run (ISO 10303-21 §7.3).
fn step_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    let mut hex_run = String::new();
    out.push('\'');
    for c in s.chars() {
        match c {
            '\'' => {
                flush_hex(&mut out, &mut hex_run);
                out.push_str("''");
            }
            '\\' => {
                flush_hex(&mut out, &mut hex_run);
                out.push_str("\\\\");
            }
            '\u{20}'..='\u{7e}' => {
                flush_hex(&mut out, &mut hex_run);
                out.push(c);
            }
            _ => {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    hex_run.push_str(&format!("{u:04X}"));
                }
            }
        }
    }
    flush_hex(&mut out, &mut hex_run);
    out.push('\'');
    out
}

/// Emit a pending `\X2\…\X0\` run before the next plain character.
fn flush_hex(out: &mut String, hex_run: &mut String) {
    if !hex_run.is_empty() {
        out.push_str("\\X2\\");
        out.push_str(hex_run);
        out.push_str("\\X0\\");
        hex_run.clear();
    }
}

/// Format a float as a STEP real — the decimal point is mandatory, so bare
/// integers get ".0". Non-finite values become 0.
fn fmt_real(v: f64) -> String {
    if !v.is_finite() {
        return "0.0".to_string();
    }
    let s = format!("{v}");
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{s}.0")
    }
}

/// Current UTC time as an ISO 8601 timestamp for FILE_NAME.
fn iso_utc_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    let rem = secs % 86_400;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}", rem / 3600, (rem % 3600) / 60, rem % 60)
}

/// Days since 1970-01-01 → (year, month, day) in the proleptic Gregorian
/// calendar (Howard Hinnant's civil_from_days).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as i64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::spf::{Spf, Val};

    /// Corner vertices of an axis-aligned box as 12 flat triangles.
    fn box_verts(lo: [f64; 3], hi: [f64; 3]) -> Vec<[f64; 3]> {
        let corner = |dx: usize, dy: usize, dz: usize| {
            [
                if dx == 0 { lo[0] } else { hi[0] },
                if dy == 0 { lo[1] } else { hi[1] },
                if dz == 0 { lo[2] } else { hi[2] },
            ]
        };
        let c = [
            corner(0, 0, 0),
            corner(1, 0, 0),
            corner(1, 1, 0),
            corner(0, 1, 0),
            corner(0, 0, 1),
            corner(1, 0, 1),
            corner(1, 1, 1),
            corner(0, 1, 1),
        ];
        let quads: [[usize; 4]; 6] = [
            [0, 3, 2, 1], // bottom
            [4, 5, 6, 7], // top
            [0, 1, 5, 4], // front
            [1, 2, 6, 5], // right
            [2, 3, 7, 6], // back
            [3, 0, 4, 7], // left
        ];
        let mut v = Vec::new();
        for q in &quads {
            for (a, b, cc) in [(q[0], q[1], q[2]), (q[0], q[2], q[3])] {
                v.push(c[a]);
                v.push(c[b]);
                v.push(c[cc]);
            }
        }
        v
    }

    /// One wall "W-1" on storey "L1": a 6-per-face box (12 triangles) and
    /// two property rows in one set.
    fn sample_model() -> IfcExportModel {
        IfcExportModel {
            elements: vec![IfcExportElement {
                guid: "1kTvXnbbzCWw8lcMd1d$4o".to_string(),
                ifc_class: "IFCWALL".to_string(),
                name: "W-1".to_string(),
                storey: "L1".to_string(),
                verts: box_verts([0.0, 0.0, 0.0], [1000.0, 200.0, 3000.0]),
                props: vec![
                    (
                        "Pset_WallCommon".to_string(),
                        "FireRating".to_string(),
                        "REI 60".to_string(),
                    ),
                    (
                        "Pset_WallCommon".to_string(),
                        "Length".to_string(),
                        "4250".to_string(),
                    ),
                ],
            }],
        }
    }

    #[test]
    fn writes_a_round_trippable_ifc4_file() {
        let text = write_ifc(&sample_model()).expect("write succeeds");
        let spf = Spf::parse(text.as_bytes()).expect("own output parses");
        assert_eq!(spf.schema(), "IFC4");

        // The reader is tolerant of a missing comma between adjacent values,
        // so scan the raw lines as well: an unset `$` must never run straight
        // into the next value token.
        for line in text.lines().filter(|l| l.starts_with('#')) {
            assert!(
                !line.contains("$'") && !line.contains("$(") && !line.contains("$$"),
                "merged argument tokens in {line}"
            );
        }

        // The wall exists, with its guid and name in the root slots.
        let wall = spf.first_of_type("IFCWALL").expect("wall entity");
        assert_eq!(wall.str(0), Some("1kTvXnbbzCWw8lcMd1d$4o"));
        assert_eq!(wall.str(2), Some("W-1"));

        // A faceted brep exists: 12 faces over 8 deduplicated box corners,
        // plus the one shared origin point.
        let brep = spf.first_of_type("IFCFACETEDBREP").expect("faceted brep");
        assert_eq!(spf.iter().filter(|e| e.is("IFCFACE")).count(), 12);
        assert_eq!(
            spf.iter().filter(|e| e.ty() == "IFCCARTESIANPOINT").count(),
            9
        );

        // The wall's representation chain reaches that brep.
        let shape = wall.ref_id(6).and_then(|id| spf.get(id)).expect("product shape");
        let repr = shape
            .list(2)
            .and_then(|l| l.first())
            .and_then(Val::as_ref)
            .and_then(|id| spf.get(id))
            .expect("shape representation");
        let item = repr
            .list(3)
            .and_then(|l| l.first())
            .and_then(Val::as_ref)
            .and_then(|id| spf.get(id))
            .expect("brep item");
        assert_eq!(item.id, brep.id);

        // Storey "L1" exists and contains the wall.
        let storey = spf
            .of_type("IFCBUILDINGSTOREY")
            .find(|s| s.str(2) == Some("L1"))
            .expect("storey L1");
        let rel = spf
            .first_of_type("IFCRELCONTAINEDINSPATIALSTRUCTURE")
            .expect("containment rel");
        let members: Vec<u64> = rel
            .list(4)
            .expect("related elements")
            .iter()
            .filter_map(Val::as_ref)
            .collect();
        assert!(members.contains(&wall.id), "wall must be contained");
        assert_eq!(rel.ref_id(5), Some(storey.id));

        // Full aggregate chain: project → site → building → storeys.
        assert_eq!(spf.of_type("IFCRELAGGREGATES").count(), 3);
        assert!(spf.first_of_type("IFCPROJECT").is_some());
        assert!(spf.first_of_type("IFCSITE").is_some());
        assert!(spf.first_of_type("IFCBUILDING").is_some());

        // Millimetre length unit.
        let si = spf.first_of_type("SI_UNIT").expect("si unit");
        assert!(si.is("LENGTH_UNIT"));
        assert_eq!(si.get(1).and_then(Val::as_enum), Some("MILLI"));
        assert_eq!(si.get(2).and_then(Val::as_enum), Some("METRE"));

        // Both property rows survive, with the right value types
        // (Name, Description, NominalValue, Unit — value in slot 2).
        let fire = spf
            .of_type("IFCPROPERTYSINGLEVALUE")
            .find(|p| p.str(0) == Some("FireRating"))
            .expect("FireRating row");
        assert_eq!(fire.get(2).and_then(Val::as_str), Some("REI 60"));
        let length = spf
            .of_type("IFCPROPERTYSINGLEVALUE")
            .find(|p| p.str(0) == Some("Length"))
            .expect("Length row");
        assert_eq!(length.get(2).and_then(Val::as_f64), Some(4250.0));

        // One property set carrying both rows, linked to the wall.
        let pset = spf.first_of_type("IFCPROPERTYSET").expect("property set");
        assert_eq!(pset.str(2), Some("Pset_WallCommon"));
        assert_eq!(pset.list(4).expect("has properties").len(), 2);
        let defines = spf
            .first_of_type("IFCRELDEFINESBYPROPERTIES")
            .expect("defines-by-properties rel");
        let objects: Vec<u64> = defines
            .list(4)
            .expect("related objects")
            .iter()
            .filter_map(Val::as_ref)
            .collect();
        assert!(objects.contains(&wall.id));
        let definitions: Vec<u64> = defines
            .list(5)
            .expect("property definitions")
            .iter()
            .filter_map(Val::as_ref)
            .collect();
        assert!(definitions.contains(&pset.id));
    }

    #[test]
    fn rejects_an_empty_model() {
        let model = IfcExportModel {
            elements: Vec::new(),
        };
        assert!(write_ifc(&model).is_err());
    }

    #[test]
    fn normalises_and_falls_back_on_class_names() {
        // Lower-case, missing prefix → resolved to the allowlist keyword.
        let mut model = sample_model();
        model.elements[0].ifc_class = "wall".into();
        let text = write_ifc(&model).expect("write");
        assert!(text.contains("IFCWALL("));

        // Unknown class → IFCBUILDINGELEMENTPROXY.
        model.elements[0].ifc_class = "IFCSPACEship".into();
        let text = write_ifc(&model).expect("write");
        assert!(text.contains("IFCBUILDINGELEMENTPROXY("));
        assert!(!text.contains("IFCSPACESHIP"));
    }

    #[test]
    fn empty_storey_lands_in_level_1_and_degenerate_geometry_is_dropped() {
        let mut model = sample_model();
        model.elements[0].storey = String::new();
        model.elements[0].verts = vec![[0.0; 3]; 6]; // six copies of one point
        let text = write_ifc(&model).expect("write");
        let spf = Spf::parse(text.as_bytes()).expect("parse");

        let storey = spf
            .of_type("IFCBUILDINGSTOREY")
            .find(|s| s.str(2) == Some("Level 1"))
            .expect("default storey");
        let wall = spf.first_of_type("IFCWALL").expect("wall");
        let rel = spf
            .first_of_type("IFCRELCONTAINEDINSPATIALSTRUCTURE")
            .expect("containment rel");
        assert_eq!(rel.ref_id(5), Some(storey.id));
        assert!(rel
            .list(4)
            .expect("related elements")
            .iter()
            .any(|v| v.as_ref() == Some(wall.id)));

        // Degenerate triangles leave no faces, so the wall carries no
        // representation at all.
        assert!(spf.first_of_type("IFCFACETEDBREP").is_none());
        assert_eq!(wall.ref_id(6), None);
    }

    #[test]
    fn escapes_quotes_backslashes_and_unicode_in_strings() {
        let mut model = sample_model();
        model.elements[0].name = "W\\1 'odd'".into();
        let text = write_ifc(&model).expect("write");
        let spf = Spf::parse(text.as_bytes()).expect("parse");
        let wall = spf.first_of_type("IFCWALL").expect("wall");
        assert_eq!(wall.str(2), Some("W\\1 'odd'"));

        model.elements[0].name = "Mur café".into();
        let text = write_ifc(&model).expect("write");
        let spf = Spf::parse(text.as_bytes()).expect("parse");
        let wall = spf.first_of_type("IFCWALL").expect("wall");
        assert_eq!(wall.str(2), Some("Mur café"));
    }
}
