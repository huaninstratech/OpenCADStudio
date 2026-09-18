// Offline diag: do block-internal text wires survive the NORMAL (unselected)
// render path for BS33-11_R0.dwg, and if they carry glyphs, are those glyphs
// piled at one spot (the "overlapping garbage" the user sees)?
use OpenCADStudio::io::load_bytes_finalized;
use OpenCADStudio::scene::Scene;

#[test]
fn bs33_block_text_normal_path_diag() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("bs33_copy.dwg");
    if !path.exists() {
        eprintln!("skipped: target/bs33_copy.dwg not found");
        return;
    }
    let bytes = std::fs::read(&path).expect("read drawing");
    let (doc, _) = load_bytes_finalized(&path, bytes).expect("load drawing");
    let mut scene = Scene::new();
    scene.document = doc;

    let wires = scene.entity_wires();
    let text_wires: Vec<&OpenCADStudio::scene::WireModel> =
        wires.iter().filter(|w| !w.text_verts.is_empty()).collect();
    eprintln!(
        "wires={} text_wires={} instanced_text={} direct_text={}",
        wires.len(),
        text_wires.len(),
        text_wires.iter().filter(|w| w.render_instance.is_some()).count(),
        text_wires.iter().filter(|w| w.render_instance.is_none()).count(),
    );

    let mut display_hidden = 0usize;
    let mut pileup = 0usize;
    let mut worst: Vec<(f32, String, usize, usize)> = Vec::new();
    for w in &text_wires {
        // Normal path: gather_text_verts (no instance) and upload_block_vertices
        // (instance) both skip wires with display_visible == false.
        if !w.display_visible {
            display_hidden += 1;
        }
        let mut xs: Vec<i64> = w
            .text_verts
            .iter()
            .map(|v| ((v.pos[0] as f64 + v.pos_low[0] as f64) * 100.0) as i64)
            .collect();
        xs.sort_unstable();
        xs.dedup();
        let glyphs = (w.text_verts.len() / 4).max(1);
        let spread = xs.len() as f32 / glyphs as f32;
        if spread < 0.5 {
            pileup += 1;
            worst.push((spread, w.name.clone(), glyphs, xs.len()));
        }
    }
    eprintln!(
        "display_visible=false: {display_hidden}   glyph-pileup wires: {pileup}"
    );
    worst.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    for (spread, handle, glyphs, distinct_x) in worst.iter().take(10) {
        let entity = scene
            .document
            .get_entity(acadrust::Handle::new(
                handle.parse::<u64>().unwrap_or(0),
            ))
            .map(|e| match e {
                acadrust::EntityType::Text(t) => format!(
                    "Text style={:?} h={:?} value={:?}",
                    t.style,
                    t.height,
                    t.value.chars().take(40).collect::<String>()
                ),
                acadrust::EntityType::MText(t) => format!(
                    "MText style={:?} h={:?} value={:?}",
                    t.style,
                    t.height,
                    t.value.chars().take(40).collect::<String>()
                ),
                other => format!("{other:?}"),
            })
            .unwrap_or_else(|| "<unresolved: block-local wire>".into());
        eprintln!(
            "spread={spread:.2} handle={handle} glyphs={glyphs} distinct_x={distinct_x} :: {entity}"
        );
    }

    // The block text must exist AND be reachable on the unselected path.
    assert!(
        !text_wires.is_empty(),
        "no text wires at all — block text never tessellated"
    );
    assert!(
        display_hidden < text_wires.len(),
        "every text wire is display_visible=false — unselected block text cannot render"
    );
}
