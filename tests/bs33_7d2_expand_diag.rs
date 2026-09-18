// Diag: expand INSERT 7D2 (block "Unknown-530909461-13 - 8299") and report
// where its content actually is, and how its TEXT wires order against the
// WIPEOUT fill wires (draw order = visibility vs the wipeout).
use acadrust::entities::Insert;
use acadrust::types::Vector3;
use acadrust::EntityType;
use OpenCADStudio::io::load_bytes_finalized;
use OpenCADStudio::scene::cache::block_cache::{expand_insert, BlockCache};
use OpenCADStudio::scene::view::render::InheritStyle;

#[test]
fn bs33_insert_7d2_content_and_order() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("bs33_copy.dwg");
    if !path.exists() {
        eprintln!("skipped: target/bs33_copy.dwg not found");
        return;
    }
    let bytes = std::fs::read(&path).expect("read");
    let (doc, _) = load_bytes_finalized(&path, bytes).expect("load");
    let mut scene = OpenCADStudio::scene::Scene::new();
    scene.document = doc;
    let handle = acadrust::Handle::new(0x7D2);
    let entity = scene
        .document
        .get_entity(handle)
        .expect("insert 7D2 exists");
    let ins = match entity {
        EntityType::Insert(ins) => ins.clone(),
        other => panic!("7D2 is not an Insert: {other:?}"),
    };

    let cache = BlockCache::build(
        &scene.document, 1.0, None, true, [0.0, 0.0, 0.0, 1.0], None,
        &scene.draw_depth_map(),
    );
    let wires = expand_insert(
        &scene.document, &cache, &ins, handle,
        [1.0, 1.0, 1.0, 1.0], 0, 0.0, [0.0; 8], 1.0,
        InheritStyle { color: [1.0, 1.0, 1.0, 1.0], pat_len: 0.0, pat: [0.0; 8], lw_px: 1.0 },
        0, false, false, 1.0, None, None, false, [0.0, 0.0, 0.0, 1.0], 1.0,
        OpenCADStudio::scene::BlockScalePolicy::FromInsert, false,
    )
    .expect("block defn cached");

    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    let mut text_wires = 0usize;
    let mut fill_wires = 0usize;
    let mut first_text_idx = None;
    let mut first_fill_idx = None;
    for (idx, w) in wires.iter().enumerate() {
        let mut pts = w.points.iter().chain(w.fill_tris.iter());
        for p in pts.by_ref() {
            for d in 0..2 {
                min[d] = min[d].min(p[d] as f64);
                max[d] = max[d].max(p[d] as f64);
            }
        }
        for v in &w.text_verts {
            for d in 0..2 {
                min[d] = min[d].min(v.pos[d] as f64);
                max[d] = max[d].max(v.pos[d] as f64);
            }
        }
        if !w.text_verts.is_empty() {
            text_wires += 1;
            if first_text_idx.is_none() {
                first_text_idx = Some(idx);
            }
        }
        if !w.fill_tris.is_empty() {
            fill_wires += 1;
            if first_fill_idx.is_none() {
                first_fill_idx = Some(idx);
            }
        }
    }
    eprintln!(
        "wires={} text_wires={text_wires} fill_wires={fill_wires} bbox=({:.3},{:.3})..({:.3},{:.3})",
        wires.len(), min[0], min[1], max[0], max[1]
    );
    eprintln!(
        "first fill wire idx={first_fill_idx:?}, first text wire idx={first_text_idx:?} (later index draws on top)"
    );
    // where do the text glyphs sit?
    for w in wires.iter().filter(|w| !w.text_verts.is_empty()) {
        let xs: Vec<f64> = w
            .text_verts
            .iter()
            .map(|v| v.pos[0] as f64 + v.pos_low[0] as f64)
            .collect();
        let ys: Vec<f64> = w
            .text_verts
            .iter()
            .map(|v| v.pos[1] as f64 + v.pos_low[1] as f64)
            .collect();
        let depths: Vec<f32> =
            w.text_verts.iter().map(|v| v.draw_depth).collect();
        let dmin = depths.iter().cloned().fold(f32::INFINITY, f32::min);
        let dmax = depths.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        eprintln!(
            "text wire: glyphs={} x {:.2}..{:.2} y {:.2}..{:.2} depth_override={:?} tv.draw_depth={:.6}..{:.6}",
            w.text_verts.len() / 4,
            xs.iter().cloned().fold(f64::INFINITY, f64::min),
            xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            ys.iter().cloned().fold(f64::INFINITY, f64::min),
            ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            w.depth_override,
            dmin,
            dmax,
        );
    }
    let _ = Vector3::new(0.0, 0.0, 0.0);
    let _ = Insert::new("", Vector3::new(0.0, 0.0, 0.0));
}
