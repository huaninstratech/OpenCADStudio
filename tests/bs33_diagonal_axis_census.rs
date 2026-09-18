// Diag: are the faint gray diagonals + red/green lines through the origin in
// the bs33 screenshot real drawing content or renderer artifacts? Census the
// document: layer states, entity-type counts, every XLine/Ray, every red/green
// line near the origin, and every hatch with its pattern angle.
use acadrust::EntityType;
use OpenCADStudio::io::load_bytes_finalized;

fn color_tag(c: &acadrust::types::Color) -> String {
    match c {
        acadrust::types::Color::ByLayer => "ByLayer".into(),
        acadrust::types::Color::ByBlock => "ByBlock".into(),
        acadrust::types::Color::None => "None".into(),
        acadrust::types::Color::Index(i) => format!("ACI{i}"),
        acadrust::types::Color::Rgb { r, g, b } => format!("RGB({r},{g},{b})"),
    }
}

fn angle_deg(dx: f64, dy: f64) -> f64 {
    dy.atan2(dx).to_degrees()
}

#[test]
fn bs33_diagonal_axis_census() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("bs33_copy.dwg");
    if !path.exists() {
        eprintln!("skipped: target/bs33_copy.dwg not found");
        return;
    }
    let bytes = std::fs::read(&path).expect("read");
    let (doc, _) = load_bytes_finalized(&path, bytes).expect("load");

    eprintln!("=== LAYERS ===");
    for l in doc.layers.iter() {
        eprintln!(
            "{:<28} color={:<10} off={:<5} frozen={:<5} locked={} plottable={}",
            l.name,
            color_tag(&l.color),
            l.flags.off,
            l.flags.frozen,
            l.flags.locked,
            l.is_plottable
        );
    }

    eprintln!("=== ENTITY TYPE COUNTS ===");
    use std::collections::BTreeMap;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for e in doc.entities() {
        let name = match e {
            EntityType::Line(_) => "Line",
            EntityType::XLine(_) => "XLine",
            EntityType::Ray(_) => "Ray",
            EntityType::Arc(_) => "Arc",
            EntityType::Circle(_) => "Circle",
            EntityType::LwPolyline(_) => "LwPolyline",
            EntityType::Polyline(_) => "Polyline",
            EntityType::Text(_) => "Text",
            EntityType::MText(_) => "MText",
            EntityType::Hatch(_) => "Hatch",
            EntityType::Wipeout(_) => "Wipeout",
            EntityType::Insert(_) => "Insert",
            EntityType::Dimension(_) => "Dimension",
            EntityType::Solid(_) => "Solid",
            EntityType::Face3D(_) => "Face3D",
            _ => "Other",
        };
        *counts.entry(name.to_string()).or_default() += 1;
    }
    for (k, v) in &counts {
        eprintln!("{k:<14} {v}");
    }

    eprintln!("=== XLINES ===");
    let mut n = 0usize;
    for e in doc.entities() {
        if let EntityType::XLine(x) = e {
            n += 1;
            eprintln!(
                "{:#X} layer={:<22} color={:<9} inv={} base=({:.2},{:.2}) dir=({:.4},{:.4}) angle={:.1}°",
                x.common.handle.value(),
                x.common.layer,
                color_tag(&x.common.color),
                x.common.invisible,
                x.base_point.x,
                x.base_point.y,
                x.direction.x,
                x.direction.y,
                angle_deg(x.direction.x, x.direction.y),
            );
        }
    }
    eprintln!("total xlines = {n}");

    eprintln!("=== RAYS ===");
    let mut n = 0usize;
    for e in doc.entities() {
        if let EntityType::Ray(r) = e {
            n += 1;
            eprintln!(
                "{:#X} layer={:<22} color={:<9} inv={} base=({:.2},{:.2}) dir=({:.4},{:.4}) angle={:.1}°",
                r.common.handle.value(),
                r.common.layer,
                color_tag(&r.common.color),
                r.common.invisible,
                r.base_point.x,
                r.base_point.y,
                r.direction.x,
                r.direction.y,
                angle_deg(r.direction.x, r.direction.y),
            );
        }
    }
    eprintln!("total rays = {n}");

    eprintln!("=== RED/GREEN/GRAY LINES NEAR ORIGIN (|p|<300) ===");
    let mut n = 0usize;
    for e in doc.entities() {
        let EntityType::Line(l) = e else { continue };
        let near = l.start.x.abs() < 300.0
            && l.start.y.abs() < 300.0
            && l.end.x.abs() < 300.0
            && l.end.y.abs() < 300.0;
        if !near {
            continue;
        }
        let tag = color_tag(&l.common.color);
        if matches!(tag.as_str(), "ACI1" | "ACI2" | "ACI3" | "ACI8" | "ACI9" | "ByLayer") {
            n += 1;
            eprintln!(
                "{:#X} layer={:<22} color={:<9} inv={} ({:.2},{:.2})->({:.2},{:.2}) angle={:.1}° len={:.2}",
                l.common.handle.value(),
                l.common.layer,
                tag,
                l.common.invisible,
                l.start.x,
                l.start.y,
                l.end.x,
                l.end.y,
                angle_deg(l.end.x - l.start.x, l.end.y - l.start.y),
                ((l.end.x - l.start.x).powi(2) + (l.end.y - l.start.y).powi(2)).sqrt(),
            );
        }
    }
    eprintln!("total near-origin lines printed = {n}");

    eprintln!("=== HATCHES ===");
    for e in doc.entities() {
        if let EntityType::Hatch(h) = e {
            let first_line_angle = h.pattern.lines.first().map(|pl| pl.angle);
            eprintln!(
                "{:#X} layer={:<22} color={:<9} inv={} solid={} pattern={:<10} pat_angle={:.1}° scale={:.4} paths={} first_line_angle={:?}",
                h.common.handle.value(),
                h.common.layer,
                color_tag(&h.common.color),
                h.common.invisible,
                h.is_solid,
                h.pattern.name,
                h.pattern_angle.to_degrees(),
                h.pattern_scale,
                h.paths.len(),
                first_line_angle.map(|a| a.to_degrees()),
            );
        }
    }

    eprintln!("=== WIPEOUTS ===");
    for e in doc.entities() {
        if let EntityType::Wipeout(w) = e {
            eprintln!(
                "{:#X} layer={:<22} inv={} at ({:.2},{:.2})",
                w.common.handle.value(),
                w.common.layer,
                w.common.invisible,
                w.insertion_point.x,
                w.insertion_point.y,
            );
        }
    }

    // Silence unused import if Line is only used via pattern

}
