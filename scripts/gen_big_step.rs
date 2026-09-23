// gen_big_step.rs — standalone synthetic AP214 (ISO-10303-21) benchmark generator.
// std-only: no external crates, no dependency on this repository's Rust crate.
//
// Build: rustc --edition 2021 -O gen_big_step.rs -o gen_big_step.exe
// Usage: gen_big_step <output.step> <solid_count>
//
// Emits a valid AUTOMOTIVE_DESIGN (AP214) file whose data section contains
// `solid_count` MANIFOLD_SOLID_BREP solids positioned on a 200 mm grid via
// their own vertex coordinates (no assembly transforms):
//   - box solids: CLOSED_SHELL of 6 planar ADVANCED_FACEs bounded by POLY_LOOPs
//     (simplest valid topology; 100 x 100 x 100 mm boxes)
//   - every 5th solid additionally carries a cylindrical boss on top: one
//     CYLINDRICAL_SURFACE face covering the full 360 deg (trimmed by two
//     closed full-circle seam edges), plus its planar top disc and a circular
//     hole bound in the box top face (8-face watertight shell)
// One PRODUCT / PRODUCT_DEFINITION / SHAPE_DEFINITION_REPRESENTATION chain
// names the first solid only; remaining solids are unnamed so readers fall
// back to their own naming. All entity forms mirror real OCCT/Datakit AP214
// exports. Entities are numbered sequentially in emission order.

use std::env;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: gen_big_step <output.step> <solid_count>");
        return ExitCode::from(2);
    }
    let out_path = args[1].clone();
    let solid_count: u64 = match args[2].parse() {
        Ok(n) if n > 0 => n,
        _ => {
            eprintln!("invalid solid_count: {:?}", args[2]);
            return ExitCode::from(2);
        }
    };

    let file = match File::create(&out_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cannot create {}: {}", out_path, e);
            return ExitCode::from(1);
        }
    };
    let mut w = BufWriter::with_capacity(8 << 20, file);

    let mut id: u64 = 0;
    macro_rules! nid {
        () => {{
            id += 1;
            id
        }};
    }
    macro_rules! out {
        ($($a:tt)*) => {
            write!(w, $($a)*).unwrap()
        };
    }

    // ---------------- header ----------------
    out!("ISO-10303-21;\n");
    out!("/* gen_big_step: synthetic AP214 benchmark file; solid_count={}; entities per solid: 40 (box) / 64 (box+cylindrical boss on every 5th) */\n", solid_count);
    out!("HEADER;\n");
    out!("FILE_DESCRIPTION(('AP214 synthetic benchmark'),'2;1');\n");
    out!(
        "FILE_NAME('{}','2026-09-23T00:00:00',('gen_big_step'),('OpenCADStudio benchmark'),'gen_big_step 1.0','gen_big_step','');\n",
        out_path.replace('\'', "''")
    );
    out!("FILE_SCHEMA(('AUTOMOTIVE_DESIGN {{ 1 0 10303 214 1 1 1 1 }}'));\n");
    out!("ENDSEC;\n");
    out!("DATA;\n");

    // ---------------- units, geometric context ----------------
    let u_len = nid!();
    out!("#{}=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));\n", u_len);
    let u_ang = nid!();
    out!("#{}=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));\n", u_ang);
    let u_sol = nid!();
    out!("#{}=(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT());\n", u_sol);
    let unc = nid!();
    out!(
        "#{}=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-07),#{},'DISTANCE_ACCURACY_VALUE','');\n",
        unc, u_len
    );
    let gctx = nid!();
    out!(
        "#{}=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{}))GLOBAL_UNIT_ASSIGNED_CONTEXT((#{},#{},#{}))REPRESENTATION_CONTEXT('','3D Context with UNIT and UNCERTAINTY'));\n",
        gctx, unc, u_len, u_ang, u_sol
    );

    // ---------------- product / shape-definition chain ----------------
    let app = nid!();
    out!("#{}=APPLICATION_CONTEXT('core data for automotive mechanical design processes');\n", app);
    let pctx = nid!();
    out!("#{}=PRODUCT_CONTEXT('',#{},'mechanical');\n", pctx, app);
    let prod = nid!();
    out!("#{}=PRODUCT('benchmark-part','benchmark-part','',(#{}));\n", prod, pctx);
    let pdf = nid!();
    out!("#{}=PRODUCT_DEFINITION_FORMATION('','',#{});\n", pdf, prod);
    let pdctx = nid!();
    out!("#{}=PRODUCT_DEFINITION_CONTEXT('part definition',#{},'design');\n", pdctx, app);
    let pdef = nid!();
    out!("#{}=PRODUCT_DEFINITION('design','',#{},#{});\n", pdef, pdf, pdctx);
    let pds = nid!();
    out!("#{}=PRODUCT_DEFINITION_SHAPE('','',#{});\n", pds, pdef);

    // ---------------- shared axis directions ----------------
    let dz = nid!();
    out!("#{}=DIRECTION('',(0.,0.,1.));\n", dz);
    let dx = nid!();
    out!("#{}=DIRECTION('',(1.,0.,0.));\n", dx);
    let dnx = nid!();
    out!("#{}=DIRECTION('',(-1.,0.,0.));\n", dnx);
    let dy = nid!();
    out!("#{}=DIRECTION('',(0.,1.,0.));\n", dy);
    let dny = nid!();
    out!("#{}=DIRECTION('',(0.,-1.,0.));\n", dny);
    let dnz = nid!();
    out!("#{}=DIRECTION('',(0.,0.,-1.));\n", dnz);

    // planar face: POLY_LOOP bound on a PLANE; returns the ADVANCED_FACE id
    macro_rules! planar_face {
        ($pid:ident, $lp:expr, $nd:expr, $rd:expr, $loc:expr) => {{
            let ax = nid!();
            out!("#{}=AXIS2_PLACEMENT_3D('',#{},#{},#{});\n", ax, $pid[$loc], $nd, $rd);
            let pl = nid!();
            out!("#{}=PLANE('',#{});\n", pl, ax);
            let pts: Vec<String> = $lp.iter().map(|&c| format!("#{}", $pid[c])).collect();
            let lo = nid!();
            out!("#{}=POLY_LOOP('',({}));\n", lo, pts.join(","));
            let bo = nid!();
            out!("#{}=FACE_OUTER_BOUND('',#{},.T.);\n", bo, lo);
            let fa = nid!();
            out!("#{}=ADVANCED_FACE('',(#{}),#{},.T.);\n", fa, bo, pl);
            fa
        }};
    }

    let mut first_solid: u64 = 0;
    for s in 0..solid_count {
        // 100 mm box on a 200 mm grid -> adjacent boxes never touch
        let x0 = ((s % 50) * 200) as i64;
        let y0 = ((s / 50) * 200) as i64;
        let zt: i64 = 100;
        let is_boss = s % 5 == 4;

        // corners 0..3 bottom (CCW from +Z), 4..7 top
        let corners: [(i64, i64, i64); 8] = [
            (x0, y0, 0),
            (x0 + 100, y0, 0),
            (x0 + 100, y0 + 100, 0),
            (x0, y0 + 100, 0),
            (x0, y0, zt),
            (x0 + 100, y0, zt),
            (x0 + 100, y0 + 100, zt),
            (x0, y0 + 100, zt),
        ];
        let mut pid = [0u64; 8];
        for (j, (cx, cy, cz)) in corners.iter().enumerate() {
            pid[j] = nid!();
            out!("#{}=CARTESIAN_POINT('',({}.,{}.,{}.));\n", pid[j], cx, cy, cz);
        }

        let f_bot = planar_face!(pid, [0, 3, 2, 1], dnz, dx, 0);
        let f_fro = planar_face!(pid, [0, 1, 5, 4], dny, dx, 0);
        let f_rig = planar_face!(pid, [1, 2, 6, 5], dx, dy, 1);
        let f_bak = planar_face!(pid, [2, 3, 7, 6], dy, dnx, 2);
        let f_lef = planar_face!(pid, [3, 0, 4, 7], dnx, dny, 3);

        if !is_boss {
            let f_top = planar_face!(pid, [4, 5, 6, 7], dz, dx, 4);
            let shell = nid!();
            out!(
                "#{}=CLOSED_SHELL('',(#{},#{},#{},#{},#{},#{}));\n",
                shell, f_bot, f_fro, f_rig, f_bak, f_lef, f_top
            );
            let name = if s == 0 { "benchmark-solid-1" } else { "" };
            let solid = nid!();
            out!("#{}=MANIFOLD_SOLID_BREP('{}',#{});\n", solid, name, shell);
            if s == 0 {
                first_solid = solid;
            }
            continue;
        }

        // ---- cylindrical boss (full 360 deg, seam-trimmed) on the top face ----
        let cx = x0 + 50;
        let cy = y0 + 50;
        let cp1 = nid!();
        out!("#{}=CARTESIAN_POINT('',({}.,{}.,{}.));\n", cp1, cx, cy, zt);
        let cp2 = nid!();
        out!("#{}=CARTESIAN_POINT('',({}.,{}.,{}.));\n", cp2, cx, cy, zt + 60);
        let ax1 = nid!();
        out!("#{}=AXIS2_PLACEMENT_3D('',#{},#{},#{});\n", ax1, cp1, dz, dx);
        let ax2 = nid!();
        out!("#{}=AXIS2_PLACEMENT_3D('',#{},#{},#{});\n", ax2, cp2, dz, dx);
        let c1 = nid!();
        out!("#{}=CIRCLE('',#{},30.);\n", c1, ax1);
        let c2 = nid!();
        out!("#{}=CIRCLE('',#{},30.);\n", c2, ax2);
        let v1 = nid!();
        out!("#{}=VERTEX_POINT('',#{});\n", v1, cp1);
        let v2 = nid!();
        out!("#{}=VERTEX_POINT('',#{});\n", v2, cp2);
        // closed seam edges (start vertex == end vertex), as written by OCCT
        let e1 = nid!();
        out!("#{}=EDGE_CURVE('',#{},#{},#{},.T.);\n", e1, v1, v1, c1);
        let e2 = nid!();
        out!("#{}=EDGE_CURVE('',#{},#{},#{},.T.);\n", e2, v2, v2, c2);

        // hole bound in the box top face (shares the bottom seam edge)
        let oh = nid!();
        out!("#{}=ORIENTED_EDGE('',*,*,#{},.T.);\n", oh, e1);
        let lh = nid!();
        out!("#{}=EDGE_LOOP('',(#{}));\n", lh, oh);
        let bh = nid!();
        out!("#{}=FACE_BOUND('',#{},.F.);\n", bh, lh);

        // top disc of the boss (shares the top seam edge)
        let od = nid!();
        out!("#{}=ORIENTED_EDGE('',*,*,#{},.T.);\n", od, e2);
        let ld = nid!();
        out!("#{}=EDGE_LOOP('',(#{}));\n", ld, od);
        let bd = nid!();
        out!("#{}=FACE_OUTER_BOUND('',#{},.T.);\n", bd, ld);

        // cylindrical side: outer bound = bottom seam circle, second bound = top seam circle
        let oc1 = nid!();
        out!("#{}=ORIENTED_EDGE('',*,*,#{},.T.);\n", oc1, e1);
        let lc1 = nid!();
        out!("#{}=EDGE_LOOP('',(#{}));\n", lc1, oc1);
        let bc1 = nid!();
        out!("#{}=FACE_OUTER_BOUND('',#{},.T.);\n", bc1, lc1);
        let oc2 = nid!();
        out!("#{}=ORIENTED_EDGE('',*,*,#{},.F.);\n", oc2, e2);
        let lc2 = nid!();
        out!("#{}=EDGE_LOOP('',(#{}));\n", lc2, oc2);
        let bc2 = nid!();
        out!("#{}=FACE_BOUND('',#{},.T.);\n", bc2, lc2);
        let cyl = nid!();
        out!("#{}=CYLINDRICAL_SURFACE('',#{},30.);\n", cyl, ax1);

        // box top face: outer square poly loop + circular hole bound
        let axt = nid!();
        out!("#{}=AXIS2_PLACEMENT_3D('',#{},#{},#{});\n", axt, pid[4], dz, dx);
        let plt = nid!();
        out!("#{}=PLANE('',#{});\n", plt, axt);
        let topts: Vec<String> = [4usize, 5, 6, 7].iter().map(|&c| format!("#{}", pid[c])).collect();
        let lot = nid!();
        out!("#{}=POLY_LOOP('',({}));\n", lot, topts.join(","));
        let bot = nid!();
        out!("#{}=FACE_OUTER_BOUND('',#{},.T.);\n", bot, lot);
        let f_top = nid!();
        out!("#{}=ADVANCED_FACE('',(#{},#{}),#{},.T.);\n", f_top, bot, bh, plt);

        // boss top disc and cylindrical face
        let pld = nid!();
        out!("#{}=PLANE('',#{});\n", pld, ax2);
        let f_disc = nid!();
        out!("#{}=ADVANCED_FACE('',(#{}),#{},.T.);\n", f_disc, bd, pld);
        let f_cyl = nid!();
        out!("#{}=ADVANCED_FACE('',(#{},#{}),#{},.T.);\n", f_cyl, bc1, bc2, cyl);

        let shell = nid!();
        out!(
            "#{}=CLOSED_SHELL('',(#{},#{},#{},#{},#{},#{},#{},#{}));\n",
            shell, f_bot, f_fro, f_rig, f_bak, f_lef, f_top, f_disc, f_cyl
        );
        let name = if s == 0 { "benchmark-solid-1" } else { "" };
        let solid = nid!();
        out!("#{}=MANIFOLD_SOLID_BREP('{}',#{});\n", solid, name, shell);
        if s == 0 {
            first_solid = solid;
        }
    }

    // ---------------- shape definition naming the first solid ----------------
    let abr = nid!();
    out!(
        "#{}=ADVANCED_BREP_SHAPE_REPRESENTATION('benchmark shape',(#{}),#{});\n",
        abr, first_solid, gctx
    );
    let sdr = nid!();
    out!("#{}=SHAPE_DEFINITION_REPRESENTATION(#{},#{});\n", sdr, pds, abr);

    // ---------------- footer ----------------
    out!("ENDSEC;\n");
    out!("END-ISO-10303-21;\n");
    w.flush().unwrap();

    let size = fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
    println!(
        "wrote {}: {} solids, {} entities, {} bytes ({:.3} MB, {:.0} bytes/solid)",
        out_path,
        solid_count,
        id,
        size,
        size as f64 / (1024.0 * 1024.0),
        size as f64 / solid_count as f64
    );
    ExitCode::SUCCESS
}
