// gen_big_ifc.rs — standalone synthetic IFC4 (ISO-10303-21) benchmark generator.
// std-only: no external crates, no dependency on this repository's Rust crate.
//
// Build: rustc --edition 2021 -O gen_big_ifc.rs -o gen_big_ifc.exe
// Usage: gen_big_ifc <output.ifc> <wall_count> [floors]   (floors default 10)
//
// Emits a valid IFC4 file: IfcProject / IfcSite / IfcBuilding, `floors`
// IfcBuildingStorey elements and `wall_count` IfcWall elements distributed
// round-robin over the storeys, positioned on a 500 mm grid so walls never
// overlap. Every wall carries:
//   - IfcLocalPlacement (relative to its storey)
//   - IfcProductRepresentation -> IfcShapeRepresentation('Body','SweptSolid')
//     -> IfcExtrudedAreaSolid (depth 3000 mm) over
//        IfcRectangleProfileDef 200 x 400 mm, or, on every 10th wall,
//        an IfcArbitraryClosedProfileDef over an L-shaped IfcPolyline
//   - IfcRelDefinesByProperties -> IfcPropertySet with 8 IfcPropertySingleValue
//     (IFCLABEL / IFCBOOLEAN / IFCLENGTHMEASURE mix)
//   - IfcRelDefinesByProperties -> IfcElementQuantity with
//     IfcQuantityLength / IfcQuantityArea / IfcQuantityVolume
// Names/descriptions exercise \X2\<utf16>\X0\ unicode escapes, '' quote
// escaping and \\ backslash escaping. Entities are numbered sequentially in
// emission order. A Part 21 comment right after ISO-10303-21; records the
// wall count.

use std::env;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::process::ExitCode;

const IFC_B64: &[u8; 64] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";

/// 22-char IFC GlobalId derived from a counter (uniqueness is what matters).
fn ifc_guid(counter: u64) -> String {
    let v = counter as u128;
    let mut s = String::with_capacity(22);
    s.push(IFC_B64[((v >> 126) & 0x3) as usize] as char);
    for i in 1..22 {
        s.push(IFC_B64[((v >> (126 - 6 * i)) & 0x3F) as usize] as char);
    }
    s
}

/// IFC spf string encoding: ' -> '', \ -> \\, non-ASCII runs -> \X2\<hex utf16>\X0\.
fn ifc_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    let mut run: Vec<u16> = Vec::new();
    for c in s.chars() {
        let cp = c as u32;
        if cp < 0x80 {
            if !run.is_empty() {
                out.push_str("\\X2\\");
                for &u in &run {
                    out.push_str(&format!("{:04X}", u));
                }
                out.push_str("\\X0\\");
                run.clear();
            }
            match c {
                '\'' => out.push_str("''"),
                '\\' => out.push_str("\\\\"),
                _ => out.push(c),
            }
        } else if cp <= 0xFFFF {
            run.push(cp as u16);
        } else {
            let v = cp - 0x10000;
            run.push(0xD800 | ((v >> 10) as u16));
            run.push(0xDC00 | ((v & 0x3FF) as u16));
        }
    }
    if !run.is_empty() {
        out.push_str("\\X2\\");
        for &u in &run {
            out.push_str(&format!("{:04X}", u));
        }
        out.push_str("\\X0\\");
    }
    out
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 || args.len() > 4 {
        eprintln!("usage: gen_big_ifc <output.ifc> <wall_count> [floors]  (floors default 10)");
        return ExitCode::from(2);
    }
    let out_path = args[1].clone();
    let wall_count: u64 = match args[2].parse() {
        Ok(n) if n > 0 => n,
        _ => {
            eprintln!("invalid wall_count: {:?}", args[2]);
            return ExitCode::from(2);
        }
    };
    let floors: u64 = if args.len() == 4 {
        match args[3].parse() {
            Ok(n) if n > 0 => n,
            _ => {
                eprintln!("invalid floors: {:?}", args[3]);
                return ExitCode::from(2);
            }
        }
    } else {
        10
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
    let mut gseq: u64 = 0;
    macro_rules! nid {
        () => {{
            id += 1;
            id
        }};
    }
    macro_rules! ng {
        () => {{
            gseq += 1;
            ifc_guid(gseq)
        }};
    }
    macro_rules! out {
        ($($a:tt)*) => {
            write!(w, $($a)*).unwrap()
        };
    }

    // ---------------- header ----------------
    out!("ISO-10303-21;\n");
    out!("/* gen_big_ifc: synthetic IFC4 benchmark file; wall_count={}; floors={}; entities per wall: 23 (rectangle) / 31 (L-profile on every 10th) */\n", wall_count, floors);
    out!("HEADER;\n");
    out!("FILE_DESCRIPTION(('ViewDefinition [CoordinationView]'),'2;1');\n");
    out!(
        "FILE_NAME('{}','2026-09-23T00:00:00',('gen_big_ifc'),('OpenCADStudio benchmark'),'gen_big_ifc 1.0','gen_big_ifc','');\n",
        out_path.replace('\'', "''")
    );
    out!("FILE_SCHEMA(('IFC4'));\n");
    out!("ENDSEC;\n");
    out!("DATA;\n");

    // ---------------- units, context, world placement ----------------
    let u_len = nid!();
    out!("#{}=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);\n", u_len);
    let u_area = nid!();
    out!("#{}=IFCSIUNIT(*,.AREAUNIT.,$,.SQUARE_METRE.);\n", u_area);
    let u_vol = nid!();
    out!("#{}=IFCSIUNIT(*,.VOLUMEUNIT.,$,.CUBIC_METRE.);\n", u_vol);
    let u_ang = nid!();
    out!("#{}=IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.);\n", u_ang);
    let units = nid!();
    out!("#{}=IFCUNITASSIGNMENT((#{},#{},#{},#{}));\n", units, u_len, u_area, u_vol, u_ang);

    let origin = nid!();
    out!("#{}=IFCCARTESIANPOINT((0.,0.,0.));\n", origin);
    let dirz = nid!();
    out!("#{}=IFCDIRECTION((0.,0.,1.));\n", dirz);
    let dirx = nid!();
    out!("#{}=IFCDIRECTION((1.,0.,0.));\n", dirx);
    let world = nid!();
    out!("#{}=IFCAXIS2PLACEMENT3D(#{},#{},#{});\n", world, origin, dirz, dirx);
    let ctx = nid!();
    out!("#{}=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#{},$);\n", ctx, world);
    let bodyctx = nid!();
    out!("#{}=IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Body','Model',*,*,*,*,#{},$,.MODEL_VIEW.,$);\n", bodyctx, ctx);
    let origin2 = nid!();
    out!("#{}=IFCCARTESIANPOINT((0.,0.));\n", origin2);
    let pos2d = nid!();
    out!("#{}=IFCAXIS2PLACEMENT2D(#{},$);\n", pos2d, origin2);

    // ---------------- spatial structure ----------------
    let project = nid!();
    out!(
        "#{}=IFCPROJECT('{}',$,'Big IFC parse benchmark ({} walls)',$,$,$,$,(#{}),#{});\n",
        project,
        ng!(),
        wall_count,
        ctx,
        units
    );
    let site_plc = nid!();
    out!("#{}=IFCLOCALPLACEMENT($,#{});\n", site_plc, world);
    let site = nid!();
    out!(
        "#{}=IFCSITE('{}',$,'Benchmark site',$,$,#{},$,$,.ELEMENT.,$,$,$,$,$);\n",
        site,
        ng!(),
        site_plc
    );
    let bld_plc = nid!();
    out!("#{}=IFCLOCALPLACEMENT(#{},#{});\n", bld_plc, site_plc, world);
    let building = nid!();
    out!(
        "#{}=IFCBUILDING('{}',$,'Benchmark building',$,$,#{},$,$,.ELEMENT.,$,$,$);\n",
        building,
        ng!(),
        bld_plc
    );

    let mut storey_ids: Vec<u64> = Vec::with_capacity(floors as usize);
    let mut storey_plcs: Vec<u64> = Vec::with_capacity(floors as usize);
    for f in 0..floors {
        let z = f * 3000;
        let sp = nid!();
        out!("#{}=IFCCARTESIANPOINT((0.,0.,{}.));\n", sp, z);
        let sa = nid!();
        out!("#{}=IFCAXIS2PLACEMENT3D(#{},$,$);\n", sa, sp);
        let plc = nid!();
        out!("#{}=IFCLOCALPLACEMENT(#{},#{});\n", plc, bld_plc, sa);
        let name = format!("Storey {} - T\u{1ea7}ng", f + 1);
        let st = nid!();
        out!(
            "#{}=IFCBUILDINGSTOREY('{}',$,'{}',$,$,#{},$,$,.ELEMENT.,{}.);\n",
            st,
            ng!(),
            ifc_string(&name),
            plc,
            z
        );
        storey_ids.push(st);
        storey_plcs.push(plc);
    }

    // project -> site -> building -> storeys
    let rel = nid!();
    out!("#{}=IFCRELAGGREGATES('{}',$,$,$,(#{}),(#{}));\n", rel, ng!(), project, site);
    let rel = nid!();
    out!("#{}=IFCRELAGGREGATES('{}',$,$,$,(#{}),(#{}));\n", rel, ng!(), site, building);
    let kids: Vec<String> = storey_ids.iter().map(|s| format!("#{}", s)).collect();
    let rel = nid!();
    out!(
        "#{}=IFCRELAGGREGATES('{}',$,$,$,(#{}),({}));\n",
        rel,
        ng!(),
        building,
        kids.join(",")
    );

    // ---------------- walls ----------------
    let mut walls_per_storey: Vec<Vec<u64>> = vec![Vec::new(); floors as usize];
    // L-shaped closed profile: 200 x 400 outer, 100 x 250 notch (area 0.055 m^2).
    let l_pts: [(u64, u64); 7] = [(0, 0), (200, 0), (200, 150), (100, 150), (100, 400), (0, 400), (0, 0)];

    for i in 0..wall_count {
        let floor = (i % floors) as usize;
        let k = i / floors;
        let gx = (k % 200) * 500; // 200 columns x 500 mm per strip -> no overlaps
        let gy = (k / 200) * 500;
        let is_l = (i + 1) % 10 == 0;

        // placement
        let wpt = nid!();
        out!("#{}=IFCCARTESIANPOINT(({}.,{}.,0.));\n", wpt, gx, gy);
        let wax = nid!();
        out!("#{}=IFCAXIS2PLACEMENT3D(#{},$,$);\n", wax, wpt);
        let wplc = nid!();
        out!("#{}=IFCLOCALPLACEMENT(#{},#{});\n", wplc, storey_plcs[floor], wax);

        // profile
        let (prof, area, volume): (u64, &str, &str) = if is_l {
            let mut pids = [0u64; 7];
            for (j, (px, py)) in l_pts.iter().enumerate() {
                pids[j] = nid!();
                out!("#{}=IFCCARTESIANPOINT(({}.,{}.));\n", pids[j], px, py);
            }
            let list: Vec<String> = pids.iter().map(|p| format!("#{}", p)).collect();
            let poly = nid!();
            out!("#{}=IFCPOLYLINE(({}));\n", poly, list.join(","));
            let pr = nid!();
            out!("#{}=IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,'L200x400x100',#{});\n", pr, poly);
            (pr, "0.055", "0.165")
        } else {
            let pr = nid!();
            out!("#{}=IFCRECTANGLEPROFILEDEF(.AREA.,'W200x400',#{},200.,400.);\n", pr, pos2d);
            (pr, "0.08", "0.24")
        };

        // body geometry
        let eas = nid!();
        out!("#{}=IFCEXTRUDEDAREASOLID(#{},#{},#{},3000.);\n", eas, prof, world, dirz);
        let sr = nid!();
        out!("#{}=IFCSHAPEREPRESENTATION(#{},'Body','SweptSolid',(#{}));\n", sr, bodyctx, eas);
        let pr = nid!();
        out!("#{}=IFCPRODUCTREPRESENTATION($,$,(#{}));\n", pr, sr);

        // the wall itself
        let name = if is_l {
            format!("\u{58c1}\u{30c6}\u{30b9}\u{30c8} L-Wall #{}", i)
        } else {
            format!("T\u{01b0}\u{1edd}ng Wall #{}", i)
        };
        let desc = format!(
            "Synthetic benchmark wall #{} for large-file parsing tests; quotes: 'single' and \"double\"; path C:\\walls\\{}; generated by gen_big_ifc for the OpenCADStudio IFC reader benchmark suite",
            i, i
        );
        let wall = nid!();
        out!(
            "#{}=IFCWALL('{}',$,'{}','{}',$,#{},#{},$,$);\n",
            wall,
            ng!(),
            ifc_string(&name),
            ifc_string(&desc),
            wplc,
            pr
        );

        // property set: 8 single values (labels, booleans, length measures)
        let note = format!("T\u{01b0}\u{1edd}ng property #{}, value 'x'; from the synthetic load case table", i);
        let p1 = nid!();
        out!("#{}=IFCPROPERTYSINGLEVALUE('Reference','Nominal thickness class of the benchmark wall',IFCLABEL('W-200-{}'),$);\n", p1, i);
        let p2 = nid!();
        out!("#{}=IFCPROPERTYSINGLEVALUE('LoadBearing','True when the wall carries structural loads in the benchmark model',IFCBOOLEAN(.T.),$);\n", p2);
        let p3 = nid!();
        out!("#{}=IFCPROPERTYSINGLEVALUE('IsExternal','True when the wall is exposed to the exterior environment',IFCBOOLEAN(.F.),$);\n", p3);
        let p4 = nid!();
        out!("#{}=IFCPROPERTYSINGLEVALUE('FireRating','Fire resistance classification of the wall assembly',IFCLABEL('REI30'),$);\n", p4);
        let p5 = nid!();
        out!("#{}=IFCPROPERTYSINGLEVALUE('AcousticRating','Airborne sound insulation rating of the wall assembly',IFCLABEL('54 dB'),$);\n", p5);
        let p6 = nid!();
        out!("#{}=IFCPROPERTYSINGLEVALUE('Height','Design height of the wall measured from the storey level',IFCLENGTHMEASURE(3000.),$);\n", p6);
        let p7 = nid!();
        out!("#{}=IFCPROPERTYSINGLEVALUE('Thickness','Nominal wall thickness measured in millimetres',IFCLENGTHMEASURE(200.),$);\n", p7);
        let p8 = nid!();
        out!("#{}=IFCPROPERTYSINGLEVALUE('Note','Free-form benchmark annotation with encoded characters',IFCLABEL('{}'),$);\n", p8, ifc_string(&note));
        let pset = nid!();
        out!(
            "#{}=IFCPROPERTYSET('{}',$,'Pset_WallCommon_Bench',$,(#{},#{},#{},#{},#{},#{},#{},#{}));\n",
            pset,
            ng!(),
            p1, p2, p3, p4, p5, p6, p7, p8
        );
        let relp = nid!();
        out!(
            "#{}=IFCRELDEFINESBYPROPERTIES('{}',$,$,$,(#{}),#{});\n",
            relp,
            ng!(),
            wall,
            pset
        );

        // element quantities (length in mm, area in m^2, volume in m^3)
        let q1 = nid!();
        out!("#{}=IFCQUANTITYLENGTH('Height',$,$,3000.,$);\n", q1);
        let q2 = nid!();
        out!("#{}=IFCQUANTITYAREA('GrossArea',$,$,{},$);\n", q2, area);
        let q3 = nid!();
        out!("#{}=IFCQUANTITYVOLUME('GrossVolume',$,$,{},$);\n", q3, volume);
        let eq = nid!();
        out!(
            "#{}=IFCELEMENTQUANTITY('{}',$,'BaseQuantities',$,$,(#{},#{},#{}));\n",
            eq,
            ng!(),
            q1, q2, q3
        );
        let relq = nid!();
        out!(
            "#{}=IFCRELDEFINESBYPROPERTIES('{}',$,$,$,(#{}),#{});\n",
            relq,
            ng!(),
            wall,
            eq
        );

        walls_per_storey[floor].push(wall);
    }

    // ---------------- containment ----------------
    for (f, list) in walls_per_storey.iter().enumerate() {
        for chunk in list.chunks(500) {
            let refs: Vec<String> = chunk.iter().map(|p| format!("#{}", p)).collect();
            let rel = nid!();
            out!(
                "#{}=IFCRELCONTAINEDINSPATIALSTRUCTURE('{}',$,$,$,({}),#{});\n",
                rel,
                ng!(),
                refs.join(","),
                storey_ids[f]
            );
        }
    }

    // ---------------- footer ----------------
    out!("ENDSEC;\n");
    out!("END-ISO-10303-21;\n");
    w.flush().unwrap();

    let size = fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
    println!(
        "wrote {}: {} walls, {} floors, {} entities, {} bytes ({:.3} MB, {:.0} bytes/wall)",
        out_path,
        wall_count,
        floors,
        id,
        size,
        size as f64 / (1024.0 * 1024.0),
        size as f64 / wall_count as f64
    );
    ExitCode::SUCCESS
}
