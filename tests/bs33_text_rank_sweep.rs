// Sweep: every block's Text/MText children vs its Wipeout/Hatch fills.
// A fill that draws BEFORE a text (earlier entity order) must own a SMALLER
// draw rank than that text — otherwise the fill still masks text that the
// drawing places above it. Flags texts whose rank is missing (0 while the
// block's other children are ranked) or outranked by an earlier fill.
use acadrust::EntityType;
use OpenCADStudio::io::load_bytes_finalized;
use OpenCADStudio::scene::Scene;

#[test]
fn bs33_block_text_rank_sweep() {
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
    let rank = |h: u64| depths.get(&h).map(|d| d[0]);

    let mut blocks_checked = 0usize;
    let mut missing_rank = 0usize;
    let mut outranked = 0usize;
    let mut samples: Vec<String> = Vec::new();

    for br in scene.document.block_records.iter() {
        let brh = br.handle;
        let mut fills: Vec<(u64, f32)> = Vec::new();
        let mut texts: Vec<(u64, f32)> = Vec::new();
        let mut unranked_children = 0usize;
        for entity in scene.document.entities() {
            let common = entity.common();
            if common.owner_handle != brh {
                continue;
            }
            let h = common.handle.value();
            let is_fill = matches!(
                entity,
                EntityType::Wipeout(_) | EntityType::Hatch(_)
            );
            let is_text = matches!(
                entity,
                EntityType::Text(_)
                    | EntityType::MText(_)
                    | EntityType::AttributeDefinition(_)
            );
            if !is_fill && !is_text {
                continue;
            }
            match rank(h) {
                Some(r) => {
                    if is_fill {
                        fills.push((h, r));
                    } else {
                        texts.push((h, r));
                    }
                }
                None => unranked_children += 1,
            }
        }
        if texts.is_empty() {
            continue;
        }
        blocks_checked += 1;
        for (th, tr) in &texts {
            if *tr == 0.0 && fills.iter().any(|(_, fr)| *fr != 0.0) {
                missing_rank += 1;
                if samples.len() < 12 {
                    samples.push(format!(
                        "MISSING-RANK text {th} rank 0 in block '{}' (fills ranked {:?})",
                        br.name,
                        fills.iter().map(|f| f.1).collect::<Vec<_>>()
                    ));
                }
                continue;
            }
            // fill drawn BEFORE this text (lower handle) must not outrank it
            for (fh, fr) in &fills {
                if *fh < *th && *fr >= *tr {
                    outranked += 1;
                    if samples.len() < 12 {
                        samples.push(format!(
                            "OUTRANKED text {th} rank {tr} < fill {fh} rank {fr} in block '{}'",
                            br.name
                        ));
                    }
                    break;
                }
            }
        }
        let _ = unranked_children;
    }

    eprintln!(
        "blocks_with_text={blocks_checked} missing_rank={missing_rank} outranked={outranked}"
    );
    for s in &samples {
        eprintln!("{s}");
    }
}
