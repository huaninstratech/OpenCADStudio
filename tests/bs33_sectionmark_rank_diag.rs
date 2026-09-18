// Diag: does the draw-depth map rank SectionMark block children (9099..9105)
// the same way it ranks the Unknown-...-13 block children (2003..2011)?
use OpenCADStudio::io::load_bytes_finalized;
use OpenCADStudio::scene::Scene;

#[test]
fn bs33_sectionmark_depth_ranks() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("bs33_copy.dwg");
    if !path.exists() {
        eprintln!("skipped: target/bs33_copy.dwg not found");
        return;
    }
    let bytes = std::fs::read(&path).expect("read");
    let (doc, _) = load_bytes_finalized(&path, bytes).expect("load");
    let mut scene = Scene::new();
    scene.document = doc;
    let depths = scene.draw_depth_map();

    eprintln!("— Unknown-...-13 - 8299 children (render fine after fix):");
    for h in [2003u64, 2004, 2005, 2006, 2007, 2008, 2009, 2010, 2011] {
        eprintln!("  handle {h:X} ({h}) -> {:?}", depths.get(&h).copied());
    }
    eprintln!("— SectionMark-530909461-8959 - 8299 children (still vanish):");
    for h in [9099u64, 9100, 9101, 9102, 9103, 9104, 9105] {
        eprintln!("  handle {h:X} ({h}) -> {:?}", depths.get(&h).copied());
    }
    eprintln!("— insert 238A (0x238A): {:?}", depths.get(&(0x238A)).copied());
    eprintln!("— insert 7D2 (0x7D2):    {:?}", depths.get(&(0x7D2)).copied());
    eprintln!("total ranked entities: {}", depths.len());
}
