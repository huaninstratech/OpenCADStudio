// Check: ranks of the fragment-zone entities (Text 1A55 vs its own wipeout
// 1A51 and the nearby plate hatches 1A4E/1A50, and 1DD0/1DD3, 1DEC/1DF0).
use OpenCADStudio::io::load_bytes_finalized;
use OpenCADStudio::scene::Scene;

#[test]
fn bs33_fragment_zone_ranks() {
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

    for h in [
        0x1A51u64, 0x1A55, 0x1A4E, 0x1A50, //
        0x1DD0, 0x1DD3, 0x1DCF, 0x1DEC, 0x1DF0, 0x1DE9, 0x1DEB, 0x1DFA,
    ] {
        eprintln!(
            "handle {h:#X} ({h}) -> {:?}",
            depths.get(&h).copied()
        );
    }
}
