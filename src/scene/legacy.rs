//! Purge legacy AutoCAD objects and rebuild them as native geometry.
//!
//! Drawings authored by AutoCAD verticals (Architectural Desktop, Building
//! Systems, Plant 3D P&ID, …) carry custom objects that OpenCADStudio can
//! display only through their cached proxy-graphics previews, plus vendor
//! dictionaries (`Autodesk_PNP`, `AEC_*`, `EXAC_*`, …) whose XRecords can
//! even be corrupt enough to make other applications force a recovery on
//! open. [`plan`] inventories that legacy, [`apply`] replaces it:
//!
//! 1. Every proxy/registered-class/unknown entity is rebuilt from its
//!    proxy-graphics metafile as native `LWPOLYLINE` / `POLYLINE` / `SOLID` /
//!    `TEXT` on the proxy's own layer, color, linetype and lineweight —
//!    visually identical, but plain OCS geometry. Entities without usable
//!    graphics are removed (they drew nothing anyway).
//! 2. Non-standard named-object-dictionary entries are cut, and every
//!    dictionary / XRecord / unknown object they owned becomes unreachable.
//! 3. A reachability pass garbage-collects unreachable objects of those
//!    kinds — including Plant 3D's `PNP_CACHEBLOB2` and friends.
//! 4. Unused vendor APPIDs are dropped and the codec's conservative
//!    `repair()` cleans up dangling references.
//!
//! Fields, dynamic blocks, annotative contexts and every standard table are
//! deliberately preserved: they are DWG features OpenCADStudio writes itself,
//! not vendor payloads.

use std::collections::{HashSet, VecDeque};

use codec::entities::{EntityCommon, ExtendedEntityData, LwVertex, Solid};
use codec::objects::{ObjectType, XRecordValue};
use codec::types::{Color, Handle, LineWeight, Vector2, Vector3};
use codec::xdata::{ExtendedData, XDataValue};
use codec::{CadDocument, EntityType, LwPolyline, Polyline, Text};

use crate::scene::convert::proxy_graphics::{self, ProxyColor, ProxyStyle};

const LT_BYLAYER: u32 = 32767;
const LT_BYBLOCK: u32 = 32766;

/// NOD keys are kept when their uppercase form starts with one of these
/// prefixes; everything else (`Autodesk_PNP`, `AEC_*`, `AbsT_*`, `EXAC_*`,
/// …) is vendor data and gets cut.
fn is_standard_key(key: &str) -> bool {
    let key = key.to_ascii_uppercase();
    ["ACAD_", "ACDB", "ACDS", "GRADIENT"]
        .iter()
        .any(|prefix| key.starts_with(prefix))
}

/// Standard APPID names (`ACAD`, `AcadAnnotative`, `AcDbDynamicBlockGUID`,
/// `AcCmTransparency`, `AcAecLayerStandard`, `GradientColor1ACI`, …) carry no
/// shared prefix pattern beyond the `Ac`-CamelCase families. An unreferenced
/// standard APPID is inert but its raw EED blobs can't be scanned, so it
/// stays; vendor names (`Autodesk_PNP`, `AUTPLANT`, `PnPDataLinks`, …) go.
fn is_standard_app_id(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["acad", "acdb", "accm", "acaec", "gradient"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

/// Object kinds the garbage collector may delete when unreachable. Kept
/// narrow on purpose — layouts, materials, scales and the like stay even if
/// a scan misses their reference chain, and [`CadDocument::repair`] reports
/// anything left dangling.
fn is_purgeable_object(obj: &ObjectType) -> bool {
    matches!(
        obj,
        ObjectType::Dictionary(_)
            | ObjectType::DictionaryWithDefault(_)
            | ObjectType::DictionaryVariable(_)
            | ObjectType::XRecord(_)
            | ObjectType::PlaceHolder(_)
            | ObjectType::Unknown { .. }
            | ObjectType::ProxyObject(_)
            | ObjectType::ClassObject(_)
            | ObjectType::DataObject(_)
    )
}

/// Classify an entity as purgeable legacy: `None` keeps it untouched.
/// `Camera`, `SectionObject`, `GeoPositionMarker`, `PointCloud`, … are real
/// features, not vendor payloads, so only the wrapper kinds go.
fn legacy_kind(entity: &EntityType) -> Option<&'static str> {
    match entity {
        EntityType::Unknown(_) => Some("unknown"),
        EntityType::Extended(ext) => match &ext.data {
            ExtendedEntityData::Proxy(_)
            | ExtendedEntityData::RegisteredClass(_)
            | ExtendedEntityData::Legacy(_) => Some("proxy"),
            _ => None,
        },
        _ => None,
    }
}

/// What [`plan`] found — a cheap read-only inventory.
#[derive(Debug, Default, Clone)]
pub struct LegacyPlan {
    pub proxy_entities: usize,
    pub unknown_entities: usize,
    pub with_graphics: usize,
    pub vendor_nod_keys: Vec<String>,
}

impl LegacyPlan {
    pub fn is_empty(&self) -> bool {
        self.proxy_entities == 0 && self.unknown_entities == 0 && self.vendor_nod_keys.is_empty()
    }
}

/// What [`apply`] did.
#[derive(Debug, Default, Clone)]
pub struct LegacyReport {
    pub entities_before: usize,
    pub entities_after: usize,
    pub proxies_converted: usize,
    pub unknowns_converted: usize,
    pub pieces_added: usize,
    pub no_graphics_removed: usize,
    pub undecodable_removed: usize,
    pub vendor_nod_keys: Vec<String>,
    pub dicts_gced: usize,
    pub xrecords_gced: usize,
    pub other_objects_gced: usize,
    pub app_ids_removed: Vec<String>,
    pub repair_actions: usize,
}

impl LegacyReport {
    pub fn legacy_entities(&self) -> usize {
        self.proxies_converted
            + self.unknowns_converted
            + self.no_graphics_removed
            + self.undecodable_removed
    }

    pub fn objects_gced(&self) -> usize {
        self.dicts_gced + self.xrecords_gced + self.other_objects_gced
    }
}

/// Read-only inventory of the legacy content in `doc`.
pub fn plan(doc: &CadDocument) -> LegacyPlan {
    let mut out = LegacyPlan::default();
    for entity in doc.entities() {
        let with_graphics = entity.common().graphic_data.is_some();
        match legacy_kind(entity) {
            Some("proxy") => {
                out.proxy_entities += 1;
                if with_graphics {
                    out.with_graphics += 1;
                }
            }
            Some(_) => {
                out.unknown_entities += 1;
                if with_graphics {
                    out.with_graphics += 1;
                }
            }
            None => {}
        }
    }
    if let Some(ObjectType::Dictionary(root)) =
        doc.objects.get(&doc.header.named_objects_dict_handle)
    {
        out.vendor_nod_keys = root
            .entries
            .iter()
            .map(|(name, _)| name.clone())
            .filter(|name| !is_standard_key(name))
            .collect();
    }
    out
}

/// Rebuild legacy entities as native geometry, strip vendor dictionaries and
/// garbage-collect. Runs entirely on the codec document, so the GUI command
/// and the headless export share one implementation.
pub fn apply(doc: &mut CadDocument) -> LegacyReport {
    let mut report = LegacyReport {
        entities_before: doc.entities().count(),
        ..LegacyReport::default()
    };

    rebuild_legacy_entities(doc, &mut report);
    prune_named_object_dictionary(doc, &mut report);
    garbage_collect_objects(doc, &mut report);
    doc.object_xdata
        .retain(|handle, _| doc.objects.contains_key(handle));
    purge_unused_app_ids(doc, &mut report);
    let repair = doc.repair();
    report.repair_actions = repair.actions.len();
    report.entities_after = doc.entities().count();
    report
}

/// ── Step 1: rebuild legacy entities from cached proxy graphics ──────────
fn rebuild_legacy_entities(doc: &mut CadDocument, report: &mut LegacyReport) {
    // Snapshot up front: cloning `EntityCommon` detaches us from the entity
    // list so replacements can be added and originals removed afterwards.
    let legacy: Vec<(EntityCommon, &'static str)> = doc
        .entities()
        .filter_map(|entity| legacy_kind(entity).map(|kind| (entity.common().clone(), kind)))
        .collect();
    if legacy.is_empty() {
        return;
    }

    let mut remove: Vec<Handle> = Vec::new();
    for (common, kind) in legacy {
        let Some(blob) = common.graphic_data.clone() else {
            remove.push(common.handle);
            report.no_graphics_removed += 1;
            continue;
        };
        let decoded = proxy_graphics::decode(&blob);
        if decoded.is_empty() {
            remove.push(common.handle);
            report.undecodable_removed += 1;
            continue;
        }
        for piece in entities_from_decoded(&decoded, &common, doc) {
            if doc.add_entity(piece).is_ok() {
                report.pieces_added += 1;
            }
        }
        match kind {
            "proxy" => report.proxies_converted += 1,
            _ => report.unknowns_converted += 1,
        }
        remove.push(common.handle);
    }
    for handle in remove {
        doc.remove_entity(handle);
    }
}

/// Map decoded proxy-graphics primitives onto native codec entities.
fn entities_from_decoded(
    decoded: &proxy_graphics::Decoded,
    base: &EntityCommon,
    doc: &CadDocument,
) -> Vec<EntityType> {
    let mut out = Vec::new();
    for pl in &decoded.polylines {
        out.extend(polyline_entity(pl, base, doc));
    }
    for fill in &decoded.fills {
        // `tris` is a flat list: three consecutive points form one triangle.
        for tri in fill.tris.chunks(3) {
            let [a, b, c] = tri else { continue };
            let corner = |p: &[f64; 3]| Vector3::new(p[0], p[1], p[2]);
            let mut solid = Solid::new(corner(a), corner(b), corner(c), corner(c));
            apply_common(&mut solid.common, base, fill.style, doc);
            out.push(EntityType::Solid(solid));
        }
    }
    for text in &decoded.texts {
        let mut entity = Text::new();
        entity.value = text.text.clone();
        entity.insertion_point = Vector3::new(text.position[0], text.position[1], text.position[2]);
        entity.height = text.height;
        entity.rotation = text.rotation;
        entity.width_factor = if text.width_factor > 0.0 {
            text.width_factor
        } else {
            1.0
        };
        entity.oblique_angle = text.oblique;
        entity.style = "Standard".to_string();
        apply_common(&mut entity.common, base, text.style, doc);
        out.push(EntityType::Text(entity));
    }
    out
}

/// Rebase `common` on the source proxy's, then overlay the proxy-graphics
/// style in force for this piece. Vendor plumbing (extension dictionary,
/// reactors, XDATA) is dropped with the wrapper: the rebuilt geometry is a
/// plain OCS entity, not an AEC stand-in.
fn apply_common(
    common: &mut EntityCommon,
    base: &EntityCommon,
    style: ProxyStyle,
    doc: &CadDocument,
) {
    *common = base.clone();
    common.handle = Handle::NULL;
    common.raw_record = None;
    common.graphic_data = None;
    common.xdictionary_handle = None;
    common.reactors.clear();
    common.extended_data = ExtendedData::default();

    if let Some(index) = style.layer {
        if let Some(layer) = doc.layers.iter().nth(index as usize) {
            common.layer = layer.name.clone();
        }
    }
    common.color = match style.color {
        ProxyColor::ByLayer => Color::ByLayer,
        ProxyColor::ByBlock => Color::ByBlock,
        ProxyColor::Aci(index) => Color::Index(index),
        ProxyColor::Rgb(r, g, b) => Color::Rgb { r, g, b },
    };
    common.linetype = match style.linetype {
        LT_BYLAYER => String::new(),
        LT_BYBLOCK => "ByBlock".to_string(),
        index => doc
            .line_types
            .iter()
            .nth(index as usize)
            .map(|lt| lt.name.clone())
            .unwrap_or_default(),
    };
    common.line_weight = if style.lineweight < 0 {
        base.line_weight
    } else {
        LineWeight::from_value(style.lineweight)
    };
}

/// A proxy-graphics polyline: planar runs become `LWPOLYLINE` (with the
/// common elevation), anything with varying Z stays honest as a 3D
/// `POLYLINE`. A duplicate closing point becomes the closed flag.
fn polyline_entity(
    pl: &proxy_graphics::ProxyPolyline,
    base: &EntityCommon,
    doc: &CadDocument,
) -> Option<EntityType> {
    let points = pl.points.as_slice();
    if points.len() < 2 {
        return None;
    }
    let last = points[points.len() - 1];
    let closed = points[0].iter().zip(last.iter()).all(|(a, b)| (a - b).abs() < 1e-9);
    let pts = if closed {
        &points[..points.len() - 1]
    } else {
        points
    };
    if pts.len() < 2 {
        return None;
    }
    let z0 = pts[0][2];
    let planar = pts.iter().all(|p| (p[2] - z0).abs() < 1e-9);

    let mut entity = if planar {
        let mut lw = LwPolyline::new();
        lw.vertices = pts
            .iter()
            .map(|p| LwVertex::new(Vector2::new(p[0], p[1])))
            .collect();
        lw.is_closed = closed;
        lw.elevation = z0;
        EntityType::LwPolyline(lw)
    } else {
        let mut p3 = Polyline::from_points(
            pts.iter()
                .map(|p| Vector3::new(p[0], p[1], p[2]))
                .collect(),
        );
        if closed {
            p3.close();
        }
        EntityType::Polyline(p3)
    };
    apply_common(entity.common_mut(), base, pl.style, doc);
    Some(entity)
}

/// ── Step 2: cut non-standard NOD entries ────────────────────────────────
fn prune_named_object_dictionary(doc: &mut CadDocument, report: &mut LegacyReport) {
    let root_handle = doc.header.named_objects_dict_handle;
    let Some(ObjectType::Dictionary(root)) = doc.objects.get_mut(&root_handle) else {
        return;
    };
    root.entries.retain(|(name, _)| {
        if is_standard_key(name) {
            true
        } else {
            report.vendor_nod_keys.push(name.clone());
            false
        }
    });
}

/// ── Step 3: garbage-collect unreachable vendor objects ──────────────────
fn garbage_collect_objects(doc: &mut CadDocument, report: &mut LegacyReport) {
    let reachable = reachable_handles(doc);
    let doomed: Vec<Handle> = doc
        .objects
        .iter()
        .filter(|(handle, obj)| !reachable.contains(handle) && is_purgeable_object(obj))
        .map(|(handle, _)| *handle)
        .collect();
    for handle in doomed {
        match doc.objects.remove(&handle) {
            Some(ObjectType::Dictionary(_)) | Some(ObjectType::DictionaryWithDefault(_)) => {
                report.dicts_gced += 1
            }
            Some(ObjectType::XRecord(_)) => report.xrecords_gced += 1,
            Some(_) => report.other_objects_gced += 1,
            None => {}
        }
    }

    // Class-registered vendor objects (`AECB_*` style catalogs, Plant 3D
    // schemas, proxy objects, database helpers) never carry standard
    // content, but they can stay reachable through reference chains the
    // walk above doesn't sever (an entity extension dictionary, an object
    // XDATA key). The cross-version writer cannot encode them at all — a
    // single survivor fails the whole save — so they are deleted outright;
    // `repair()` fixes whatever pointed at them.
    let hard_doomed: Vec<Handle> = doc
        .objects
        .iter()
        .filter(|(_, obj)| {
            matches!(
                obj,
                ObjectType::ClassObject(_) | ObjectType::ProxyObject(_) | ObjectType::DataObject(_)
            )
        })
        .map(|(handle, _)| *handle)
        .collect();
    for handle in hard_doomed {
        if doc.objects.remove(&handle).is_some() {
            report.other_objects_gced += 1;
        }
    }
}

/// Every handle still claimed by live content: the NOD forest, side tables
/// for fields / dynamic blocks / annotative contexts, and the per-entity
/// references (extension dictionary, reactors, XDATA handles).
fn reachable_handles(doc: &CadDocument) -> HashSet<Handle> {
    fn xdata_handles(data: &ExtendedData, out: &mut Vec<Handle>) {
        for record in data.records() {
            for value in &record.values {
                if let XDataValue::Handle(handle) = value {
                    out.push(*handle);
                }
            }
        }
    }

    let mut roots: Vec<Handle> = Vec::new();
    roots.push(doc.header.named_objects_dict_handle);
    roots.extend(doc.fields.keys().copied());
    roots.extend(doc.context_scales.keys().copied());
    roots.extend(doc.context_scales.values().copied());
    roots.extend(doc.block_visibility_params.keys().copied());
    roots.extend(doc.block_representations.keys().copied());
    roots.extend(doc.block_representations.values().copied());
    // `object_xdata` keys are deliberately NOT roots: the map is a side view
    // of each object's own EED, not a reference to it, and its keys include
    // vendor style objects that must go with the rest of the legacy data.
    // Entries whose object dies are dropped after the sweep in [`apply`].
    for entity in doc.entities() {
        let common = entity.common();
        roots.extend(common.reactors.iter().copied());
        if let Some(handle) = common.xdictionary_handle {
            roots.push(handle);
        }
        xdata_handles(&common.extended_data, &mut roots);
    }

    let mut seen: HashSet<Handle> = roots.into_iter().filter(|h| !h.is_null()).collect();
    let mut queue: VecDeque<Handle> = seen.iter().copied().collect();
    while let Some(handle) = queue.pop_front() {
        let Some(obj) = doc.objects.get(&handle) else {
            continue;
        };
        for child in object_children(obj) {
            if !child.is_null() && seen.insert(child) {
                queue.push_back(child);
            }
        }
    }
    seen
}

/// Handles an object pulls in with it — entries, reactors, extension
/// dictionary, and the reference fields of the containers that matter.
fn object_children(obj: &ObjectType) -> Vec<Handle> {
    let mut out = Vec::new();
    match obj {
        ObjectType::Dictionary(dict) => {
            out.extend(dict.entries.iter().map(|(_, handle)| *handle));
            out.extend(dict.reactors.iter().copied());
            out.extend(dict.xdictionary_handle);
        }
        ObjectType::DictionaryWithDefault(dict) => {
            out.extend(dict.entries.iter().map(|(_, handle)| *handle));
            out.push(dict.default_handle);
        }
        ObjectType::XRecord(xrecord) => {
            for entry in &xrecord.entries {
                if let XRecordValue::Handle(handle) = &entry.value {
                    out.push(*handle);
                }
            }
            out.extend(xrecord.reactors.iter().copied());
            out.extend(xrecord.xdictionary_handle);
        }
        ObjectType::Group(group) => {
            out.extend(group.entities.iter().copied());
        }
        // DictionaryVariable and the non-purgeable kinds carry no child
        // objects the GC could miss; `PlaceHolder` has none by definition.
        _ => {}
    }
    out
}

/// ── Step 4: drop unused vendor APPIDs ───────────────────────────────────
fn purge_unused_app_ids(doc: &mut CadDocument, report: &mut LegacyReport) {
    let mut referenced: HashSet<String> = HashSet::new();
    for entity in doc.entities() {
        for record in entity.common().extended_data.records() {
            referenced.insert(record.application_name.clone());
        }
    }
    for data in doc.object_xdata.values() {
        for record in data.records() {
            referenced.insert(record.application_name.clone());
        }
    }
    let doomed: Vec<String> = doc
        .app_ids
        .iter()
        .map(|app| app.name.clone())
        .filter(|name| !referenced.contains(name) && !is_standard_app_id(name))
        .collect();
    for name in &doomed {
        doc.app_ids.remove(name);
    }
    report.app_ids_removed = doomed;
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::entities::{Line, UnknownEntity};
    use codec::objects::{Dictionary, XRecord};
    use codec::tables::{AppId, BlockRecord};
    use codec::xdata::ExtendedDataRecord;

    fn unknown_entity(dxf: &str, graphic: Option<Vec<u8>>) -> EntityType {
        let mut entity = UnknownEntity::new(dxf);
        entity.common.graphic_data = graphic;
        EntityType::Unknown(entity)
    }

    fn native_line() -> EntityType {
        let mut line = Line::new();
        line.end = Vector3::new(1.0, 0.0, 0.0);
        EntityType::Line(line)
    }

    fn add_to_nod(doc: &mut CadDocument, name: &str, handle: Handle) {
        if let Some(ObjectType::Dictionary(root)) =
            doc.objects.get_mut(&doc.header.named_objects_dict_handle)
        {
            root.add_entry(name, handle);
        }
    }

    #[test]
    fn plan_counts_legacy_entities() {
        let mut doc = CadDocument::new();
        doc.add_entity(unknown_entity("AEC_WALL", None)).unwrap();
        doc.add_entity(unknown_entity("AECB_PIPE", Some(vec![1, 2, 3])))
            .unwrap();
        doc.add_entity(native_line()).unwrap();

        let plan = plan(&doc);
        assert_eq!(plan.unknown_entities, 2);
        assert_eq!(plan.proxy_entities, 0);
        assert_eq!(plan.with_graphics, 1);
    }

    #[test]
    fn rebuilds_proxy_graphics_into_native_entities() {
        let doc = CadDocument::new();
        let decoded = proxy_graphics::Decoded {
            polylines: vec![proxy_graphics::ProxyPolyline {
                points: vec![[0.0, 0.0, 5.0], [10.0, 0.0, 5.0], [10.0, 10.0, 5.0]],
                style: ProxyStyle {
                    color: ProxyColor::Aci(3),
                    layer: None,
                    linetype: LT_BYLAYER,
                    lineweight: -1,
                },
            }],
            fills: vec![proxy_graphics::ProxyFill {
                tris: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                style: ProxyStyle::default(),
            }],
            texts: vec![proxy_graphics::ProxyText {
                position: [2.0, 3.0, 0.0],
                height: 2.5,
                rotation: 0.0,
                width_factor: 1.0,
                oblique: 0.0,
                text: "PNP-01".to_string(),
                font: String::new(),
                style: ProxyStyle::default(),
            }],
        };
        let mut base = EntityCommon::new();
        base.layer = "WALLS".to_string();

        let pieces = entities_from_decoded(&decoded, &base, &doc);
        assert_eq!(pieces.len(), 3);
        assert!(matches!(pieces[0], EntityType::LwPolyline(_)));
        assert!(matches!(pieces[1], EntityType::Solid(_)));
        assert!(matches!(pieces[2], EntityType::Text(_)));

        let EntityType::LwPolyline(lw) = &pieces[0] else {
            panic!("expected lwpolyline");
        };
        assert_eq!(lw.vertices.len(), 3);
        assert!(!lw.is_closed);
        assert_eq!(lw.elevation, 5.0);
        assert_eq!(pieces[0].common().layer, "WALLS");
        assert_eq!(pieces[0].common().color, Color::Index(3));

        let EntityType::Text(text) = &pieces[2] else {
            panic!("expected text");
        };
        assert_eq!(text.value, "PNP-01");
        assert_eq!(text.height, 2.5);
    }

    #[test]
    fn apply_removes_legacy_entities_and_keeps_natives() {
        let mut doc = CadDocument::new();
        let native = doc.add_entity(native_line()).unwrap();
        let bare = doc.add_entity(unknown_entity("AEC_WALL", None)).unwrap();
        let garbage = doc
            .add_entity(unknown_entity("AECB_DUCT", Some(vec![0xde, 0xad])))
            .unwrap();

        let report = apply(&mut doc);
        assert_eq!(report.unknowns_converted, 0);
        assert_eq!(report.no_graphics_removed, 1);
        assert_eq!(report.undecodable_removed, 1);
        assert_eq!(report.entities_after, 1);
        let handles: Vec<Handle> = doc.entities().map(|e| e.common().handle).collect();
        assert!(handles.contains(&native));
        assert!(!handles.contains(&bare));
        assert!(!handles.contains(&garbage));
    }

    #[test]
    fn cuts_vendor_nod_and_gcs_subtree_but_keeps_standard() {
        let mut doc = CadDocument::new();

        // Vendor subtree: Autodesk_PNP -> { PNP_CACHEBLOB2 (XRecord) }
        let mut vendor = Dictionary::new();
        vendor.handle = doc.allocate_handle();
        let mut cache = XRecord::new();
        cache.handle = doc.allocate_handle();
        let cache_handle = cache.handle;
        vendor.add_entry("PNP_CACHEBLOB2", cache_handle);
        let vendor_handle = vendor.handle;
        doc.objects.insert(vendor_handle, ObjectType::Dictionary(vendor));
        doc.objects.insert(cache_handle, ObjectType::XRecord(cache));
        add_to_nod(&mut doc, "Autodesk_PNP", vendor_handle);

        // Referenced-by-nothing vendor XRecord sitting directly in the map.
        let mut orphan = XRecord::new();
        orphan.handle = doc.allocate_handle();
        let orphan_handle = orphan.handle;
        doc.objects.insert(orphan_handle, ObjectType::XRecord(orphan));

        let report = apply(&mut doc);
        assert_eq!(report.vendor_nod_keys, vec!["Autodesk_PNP".to_string()]);
        assert!(report.dicts_gced >= 1);
        assert!(report.xrecords_gced >= 2);
        assert!(!doc.objects.contains_key(&vendor_handle));
        assert!(!doc.objects.contains_key(&cache_handle));
        assert!(!doc.objects.contains_key(&orphan_handle));

        // The standard NOD entries survive untouched.
        if let Some(ObjectType::Dictionary(root)) =
            doc.objects.get(&doc.header.named_objects_dict_handle)
        {
            assert!(root.get("ACAD_GROUP").is_some());
            assert!(root.get("ACAD_LAYOUT").is_some());
            assert!(root.get("Autodesk_PNP").is_none());
        } else {
            panic!("root dictionary missing");
        }
    }

    #[test]
    fn entity_xdata_handle_keeps_object_alive() {
        let mut doc = CadDocument::new();
        let mut keep = XRecord::new();
        keep.handle = doc.allocate_handle();
        let keep_handle = keep.handle;
        doc.objects.insert(keep_handle, ObjectType::XRecord(keep));

        // A surviving (native) entity's XDATA keeps the referenced object.
        let mut entity = Line::new();
        let mut record = ExtendedDataRecord::new("AUTPLANT");
        record.add_value(XDataValue::Handle(keep_handle));
        entity.common.extended_data.add_record(record);
        doc.add_entity(EntityType::Line(entity)).unwrap();

        apply(&mut doc);
        assert!(doc.objects.contains_key(&keep_handle));
    }

    #[test]
    fn replacement_lands_in_the_proxys_block() {
        let mut doc = CadDocument::new();
        let mut block = BlockRecord::new("AEC_BLOCK");
        block.handle = doc.allocate_handle();
        block.block_entity_handle = doc.allocate_handle();
        block.block_end_handle = doc.allocate_handle();
        let block_handle = block.handle;
        doc.block_records.add(block).ok();

        let mut base = EntityCommon::new();
        base.owner_handle = block_handle;
        let decoded = proxy_graphics::Decoded {
            polylines: vec![proxy_graphics::ProxyPolyline {
                points: vec![[0.0, 0.0, 0.0], [5.0, 0.0, 0.0]],
                style: ProxyStyle::default(),
            }],
            ..Default::default()
        };
        for piece in entities_from_decoded(&decoded, &base, &doc) {
            doc.add_entity(piece).unwrap();
        }
        let record = doc.block_records.get_mut("AEC_BLOCK").unwrap();
        assert_eq!(record.entity_handles.len(), 1);
    }

    #[test]
    fn purges_unused_vendor_app_ids_only() {
        let mut doc = CadDocument::new();
        let mut vendor = AppId {
            handle: doc.allocate_handle(),
            name: "Autodesk_PNP".to_string(),
        };
        doc.app_ids.add(vendor.clone()).ok();
        vendor.name = "AUTPLANT".to_string();
        doc.app_ids.add(vendor).ok();

        // AUTPLANT is referenced by a surviving entity's XDATA — kept.
        let mut entity = Line::new();
        let mut record = ExtendedDataRecord::new("AUTPLANT");
        record.add_value(XDataValue::String("data".to_string()));
        entity.common.extended_data.add_record(record);
        doc.add_entity(EntityType::Line(entity)).unwrap();

        let report = apply(&mut doc);
        assert_eq!(report.app_ids_removed, vec!["Autodesk_PNP".to_string()]);
        assert!(doc.app_ids.get("AUTPLANT").is_some());
        assert!(doc.app_ids.get("ACAD").is_some());
        assert!(doc.app_ids.get("Autodesk_PNP").is_none());
    }

    #[test]
    fn second_apply_is_a_no_op() {
        let mut doc = CadDocument::new();
        doc.add_entity(unknown_entity("AEC_WALL", None)).unwrap();
        let first = apply(&mut doc);
        let second = apply(&mut doc);
        assert!(first.legacy_entities() > 0);
        assert_eq!(second.legacy_entities(), 0);
        assert_eq!(second.objects_gced(), 0);
        assert_eq!(second.entities_before, second.entities_after);
    }
}

