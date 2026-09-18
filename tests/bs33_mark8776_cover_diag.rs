// Diag: expand the Mark-8776 INSERT (2349) offline and find every wipeout
// whose draw rank is >= the text's rank and whose boundary overlaps the
// text area — those are the fills that still mask "G-3".
use acadrust::EntityType;
use OpenCADStudio::io::load_bytes_finalized;
use OpenCADStudio::scene::cache::block_cache::{expand_insert, BlockCache};
use OpenCADStudio::scene::view::render::InheritStyle;
use OpenCADStudio::scene::Scene;

#[test]
fn bs33_mark_8776_covering_wipeouts() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("bs33_copy.dwg");
    if !path.exists() {
        eprintln!("skipped: target/bs33_copy.dwg not found");
        return;
    }
    let bytes = std::fs::read(&path).expect("read");
    let (doc, _) = load_bytes_finalized(&path, bytes).expect("load");
    let mut scene = Scene::new();
    scene.document = doc;

    let handle = acadrust::Handle::new(0x2349);
    let ins = match scene.document.get_entity(handle).expect("2349") {
        EntityType::Insert(ins) => ins.clone(),
        other => panic!("not an insert: {other:?}"),
    };

    let depths = scene.draw_depth_map();
    eprintln!(
        "own wipeout ranks: 234A={:?} 234B={:?}  text 234D={:?}",
        depths.get(&9034).copied(),
        depths.get(&9035).copied(),
        depths.get(&9037).copied()
    );

    for h in [0x234Du64, 0x7D9, 0x7DA, 0x7DB] {
        if let Some(EntityType::Text(t)) = scene.document.get_entity(acadrust::Handle::new(h)) {
            eprintln!(
                "text {h:#X}: value={:?} style={:?} height={:?} font_size_ok",
                t.value.chars().take(20).collect::<String>(),
                t.style,
                t.height
            );
        }
    }

    let cache = BlockCache::build(
        &scene.document, 1.0, None, true, [0.0, 0.0, 0.0, 1.0], None, &depths,
    );
    let wires = expand_insert(
        &scene.document, &cache, &ins, handle,
        [1.0, 1.0, 1.0, 1.0], 0, 0.0, [0.0; 8], 1.0,
        InheritStyle { color: [1.0, 1.0, 1.0, 1.0], pat_len: 0.0, pat: [0.0; 8], lw_px: 1.0 },
        0, false, false, 1.0, None, None, false, [0.0, 0.0, 0.0, 1.0], 1.0,
        OpenCADStudio::scene::BlockScalePolicy::FromInsert, false,
    )
    .expect("expand");
    eprintln!("wires={}", wires.len());
    let mut text_rank = None;
    for w in &wires {
        if let Some(v) = w.text_verts.first() {
            text_rank = Some(v.draw_depth);
            eprintln!(
                "text wire glyphs={} depth={}",
                w.text_verts.len() / 4,
                v.draw_depth
            );
        }
    }
    let Some(text_rank) = text_rank else {
        eprintln!("no text wire in expansion");
        return;
    };

    // text bbox for "G-3" (from the earlier expansion run)
    const T_X0: f64 = 176.6;
    const T_X1: f64 = 193.5;
    const T_Y0: f64 = 129.0;
    const T_Y1: f64 = 139.3;

    let mut covers = 0usize;
    for entity in scene.document.entities() {
        let EntityType::Wipeout(w) = entity else { continue };
        let Some(&[r, _]) = depths.get(&w.common.handle.value()) else {
            continue;
        };
        if r < text_rank {
            continue;
        }
        let o = w.insertion_point;
        let corners = [
            (o.x, o.y),
            (o.x + w.u_vector.x, o.y + w.u_vector.y),
            (o.x + w.v_vector.x, o.y + w.v_vector.y),
            (
                o.x + w.u_vector.x + w.v_vector.x,
                o.y + w.u_vector.y + w.v_vector.y,
            ),
        ];
        let mn_x = corners.iter().map(|c| c.0).fold(f64::INFINITY, f64::min);
        let mx_x = corners
            .iter()
            .map(|c| c.0)
            .fold(f64::NEG_INFINITY, f64::max);
        let mn_y = corners.iter().map(|c| c.1).fold(f64::INFINITY, f64::min);
        let mx_y = corners
            .iter()
            .map(|c| c.1)
            .fold(f64::NEG_INFINITY, f64::max);
        if mn_x < T_X1 && mx_x > T_X0 && mn_y < T_Y1 && mx_y > T_Y0 {
            covers += 1;
            eprintln!(
                "COVERING wipeout {:#X} rank {r} at ({mn_x:.1},{mn_y:.1})..({mx_x:.1},{mx_y:.1})",
                w.common.handle.value()
            );
        }
    }
    eprintln!("covering wipeouts: {covers}");
}
