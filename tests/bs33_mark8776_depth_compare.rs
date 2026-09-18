// Diag: compute the FINAL GPU depths for the Mark-8776 text glyphs vs its
// own wipeout masks, using the same composition the live paths use:
//   mask  = depths[insert].0 + depths[child].0 * depths[insert].1  (scene graph)
//   text  = per-vertex draw_depth from the block expansion          (block cache)
use acadrust::EntityType;
use OpenCADStudio::io::load_bytes_finalized;
use OpenCADStudio::scene::cache::block_cache::{expand_insert, BlockCache};
use OpenCADStudio::scene::view::render::InheritStyle;
use OpenCADStudio::scene::Scene;

#[test]
fn bs33_mark8776_final_depths() {
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

    let depths = scene.draw_depth_map();
    let get = |h: u64| -> String {
        match depths.get(&h) {
            Some(&[d, half]) => format!("d={d:.9} half={half:.9}"),
            None => "absent".into(),
        }
    };
    eprintln!("insert 2349: {}", get(0x2349));
    eprintln!("wipeout 234A: {}", get(0x234A));
    eprintln!("wipeout 234B: {}", get(0x234B));
    eprintln!("text    234D: {}", get(0x234D));

    let composed = |child: u64| -> Option<f32> {
        depths.get(&0x2349).and_then(|&[base, scale]| {
            depths.get(&child).map(|&[d, _]| base + d * scale)
        })
    };
    for h in [0x234Au64, 0x234B] {
        eprintln!("mask {h:#X} final = {:?}", composed(h));
    }

    let handle = acadrust::Handle::new(0x2349);
    let ins = match scene.document.get_entity(handle).expect("2349") {
        EntityType::Insert(ins) => ins.clone(),
        other => panic!("not an insert: {other:?}"),
    };
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
    for (i, w) in wires.iter().enumerate() {
        eprintln!(
            "wire {i}: name={:?} points={} fill_tris={} text_verts={} depth_override={:?} first_tv_depth={:?} first_pt_depth={:?}",
            w.name,
            w.points.len(),
            w.fill_tris.len(),
            w.text_verts.len(),
            w.depth_override,
            w.text_verts.first().map(|v| v.draw_depth),
            w.points.first().map(|p| p[2]),
        );
    }
}

#[test]
fn bs33_mark8776_bogus_instance_depth() {
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
    let depths = scene.draw_depth_map();

    // The wire name "9033" parses as a handle and hits this entry.
    eprintln!("depths[9033] = {:?}", depths.get(&9033));
    match scene.document.get_entity(acadrust::Handle::new(9033)) {
        Some(e) => eprintln!("entity with handle-value 9033 (0x2339): {:?}", e.common().handle),
        None => eprintln!("no entity with handle-value 9033"),
    }

    // Final text depth as the GPU sees it:
    // vertex(-0.082784235) + instance wire_draw_depth(name="9033", override=-0.082784235)
    let bogus = depths.get(&9033).copied();
    let inst = match (bogus, Some(-0.082784235f32)) {
        (Some([d, half]), Some(local)) => d + local * half,
        (Some([d, _]), None) => d,
        _ => 0.0,
    };
    eprintln!(
        "bogus instance depth = {inst:.9}; text total = {:.9}",
        -0.082784235f32 + inst
    );
    eprintln!("masks sit at 0.62452656 / 0.62473315");

    // Contrast: 7D2's wire names (non-numeric -> instance depth 0).
    let handle = acadrust::Handle::new(0x7D2);
    if let Some(EntityType::Insert(ins)) = scene.document.get_entity(handle) {
        eprintln!("7D2 block name = {:?}", ins.block_name);
    }
    eprintln!("mark block name = {:?}", 
        if let Some(EntityType::Insert(i)) = scene.document.get_entity(acadrust::Handle::new(0x2349)) {
            i.block_name.clone()
        } else { String::new() });
}

#[test]
fn bs33_7d2_cover_analysis() {
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
    let depths = scene.draw_depth_map();

    eprintln!("depths[2002] = {:?}", depths.get(&2002));
    let (ins_d, ins_half) = match depths.get(&2002) {
        Some(&[d, h]) => (d, h),
        None => return,
    };
    for h in [0x7D9u64, 0x7DA, 0x7DB] {
        let label = depths.get(&h).copied();
        eprintln!(
            "text {h:#X} label={label:?} composed={:?}",
            label.map(|p| ins_d + p[0] * ins_half)
        );
    }

    let boxes: Vec<(&str, [f64; 4])> = vec![
        ("BSECTION", [120.879, 34.973, 162.784, 50.973]),
        ("G-3", [176.6, 129.0, 193.5, 139.3]),
    ];
    for entity in scene.document.entities() {
        let EntityType::Wipeout(w) = entity else { continue };
        let o = w.insertion_point;
        let c0 = (o.x, o.y);
        let c1 = (o.x + w.u_vector.x, o.y + w.u_vector.y);
        let c2 = (o.x + w.v_vector.x, o.y + w.v_vector.y);
        let c3 = (o.x + w.u_vector.x + w.v_vector.x, o.y + w.u_vector.y + w.v_vector.y);
        let mnx = [c0, c1, c2, c3].iter().fold(f64::INFINITY, |a, c| a.min(c.0));
        let mxx = [c0, c1, c2, c3].iter().fold(f64::NEG_INFINITY, |a, c| a.max(c.0));
        let mny = [c0, c1, c2, c3].iter().fold(f64::INFINITY, |a, c| a.min(c.1));
        let mxy = [c0, c1, c2, c3].iter().fold(f64::NEG_INFINITY, |a, c| a.max(c.1));
        for (name, b) in &boxes {
            let (x0, y0, x1, y1) = (b[0], b[1], b[2], b[3]);
            if mnx < x1 && mxx > x0 && mny < y1 && mxy > y0 {
                let h = w.common.handle.value();
                let owner = w.common.owner_handle;
                let composed = depths
                    .get(&owner.value())
                    .and_then(|oc| depths.get(&h).map(|c| oc[0] + c[0] * oc[1]));
                let own = depths.get(&h);
                let owner_val = owner.value();
                eprintln!("wipeout {h:#X} owner={owner_val:#X} overlaps {name} own={own:?} composed={composed:?}");
            }
        }
    }
}
