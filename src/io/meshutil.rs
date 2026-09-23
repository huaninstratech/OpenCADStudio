// Shared mesh-building helpers for the IFC and STEP readers.
//
// Both formats hand us boundary polygons (profiles, face loops) that must
// become triangles. Ear clipping with hole bridging covers planar caps and
// profile faces; everything else here is small linear-algebra glue.

/// Signed area of a closed polygon (positive = counter-clockwise).
pub fn signed_area(pts: &[[f64; 2]]) -> f64 {
    let n = pts.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..n {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        sum += a[0] * b[1] - b[0] * a[1];
    }
    sum * 0.5
}

fn point_in_polygon(pt: [f64; 2], poly: &[[f64; 2]]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[j], poly[i]);
        if (a[1] > pt[1]) != (b[1] > pt[1]) {
            let x = a[0] + (pt[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if pt[0] < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Even-odd containment over several loops (outer + holes), so a point inside
/// an odd number of loops counts as inside.
pub fn point_in_loops(pt: [f64; 2], loops: &[&[[f64; 2]]]) -> bool {
    let mut inside = false;
    for poly in loops {
        if point_in_polygon(pt, poly) {
            inside = !inside;
        }
    }
    inside
}

/// Triangulate a polygon (possibly with holes) given in 2D.
///
/// Returns triangles as coordinate triples. Hole bridging follows the
/// classic max-x ray cast; when bridging or clipping degenerates the routine
/// degrades to a plain outer fan rather than failing the whole import.
pub fn ear_clip(outer: &[[f64; 2]], holes: &[&[[f64; 2]]]) -> Vec<[[f64; 2]; 3]> {
    if outer.len() < 3 {
        return Vec::new();
    }

    // Build the combined ring: outer (CCW) with each hole (CW) spliced in.
    let mut ring: Vec<[f64; 2]> = if signed_area(outer) < 0.0 {
        outer.iter().rev().copied().collect()
    } else {
        outer.to_vec()
    };
    let mut hole_ranges: Vec<(usize, usize)> = Vec::new(); // [start, end) of hole copies
    let mut holes_sorted: Vec<&[[f64; 2]]> = holes.iter().copied().collect();
    holes_sorted.sort_by(|a, b| {
        let max_x = |p: &[[f64; 2]]| p.iter().map(|q| q[0]).fold(f64::NEG_INFINITY, f64::max);
        max_x(b).partial_cmp(&max_x(a)).unwrap_or(std::cmp::Ordering::Equal)
    });
    for hole in holes_sorted {
        if hole.len() < 3 {
            continue;
        }
        let hole_ring: Vec<[f64; 2]> = if signed_area(hole) > 0.0 {
            hole.iter().rev().copied().collect()
        } else {
            hole.to_vec()
        };
        // Rightmost hole vertex, cast a ray +x, bridge to the closest edge hit.
        let anchor = hole_ring
            .iter()
            .copied()
            .fold([f64::NEG_INFINITY, 0.0], |best, p| {
                if p[0] > best[0] {
                    p
                } else {
                    best
                }
            });
        let (hx, hy) = (anchor[0], anchor[1]);
        let n = ring.len();
        let mut best: Option<(f64, usize)> = None; // (intersection x, edge index)
        for i in 0..n {
            let a = ring[i];
            let b = ring[(i + 1) % n];
            let (y0, y1) = (a[1], b[1]);
            if (y0 > hy) == (y1 > hy) {
                continue;
            }
            let x = a[0] + (hy - y0) / (y1 - y0) * (b[0] - a[0]);
            if x >= hx {
                if best.is_none() || x < best.unwrap().0 {
                    best = Some((x, i));
                }
            }
        }
        let Some((_, edge)) = best else {
            continue; // unbridgeable hole: dropped, the cap stays mostly right
        };
        // The bridge target: the visible endpoint of that edge (the vertex
        // with the larger y of the crossing edge, per standard treatment).
        let a = ring[edge];
        let b = ring[(edge + 1) % n];
        let target = if a[1] > b[1] { a } else { b };
        let target_idx = ring
            .iter()
            .position(|p| p[0] == target[0] && p[1] == target[1])
            .unwrap_or(edge);
        let splice_at = target_idx + 1;
        let hole_len = hole_ring.len();
        let mut spliced: Vec<[f64; 2]> = Vec::with_capacity(n + hole_len + 2);
        spliced.extend_from_slice(&ring[..splice_at]);
        spliced.push(target);
        spliced.extend_from_slice(&hole_ring);
        spliced.push(hole_ring[0]);
        spliced.push(target);
        spliced.extend_from_slice(&ring[splice_at..]);
        hole_ranges.push((splice_at + 1, splice_at + 1 + hole_len + 2));
        ring = spliced;
    }

    let tris = clip_ring(&ring);
    if tris.is_empty() {
        // Degraded fallback: fan the outer loop (holes ignored).
        let m = outer.len();
        return (1..m - 1)
            .map(|i| [outer[0], outer[i], outer[i + 1]])
            .collect();
    }
    tris
        .into_iter()
        .map(|tri| [ring[tri[0]], ring[tri[1]], ring[tri[2]]])
        .collect()
}

fn clip_ring(ring: &[[f64; 2]]) -> Vec<[usize; 3]> {
    let n = ring.len();
    if n < 3 {
        return Vec::new();
    }
    let ccw = signed_area(ring) > 0.0;
    let mut index: Vec<usize> = (0..n).collect();
    if !ccw {
        index.reverse();
    }
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let close = |p: [f64; 2], q: [f64; 2]| {
        (p[0] - q[0]).abs() < 1e-9 && (p[1] - q[1]).abs() < 1e-9
    };
    let eps = 1e-12;
    let mut tris = Vec::with_capacity(n.saturating_sub(2));
    let mut guard = 0usize;
    while index.len() > 3 && guard < n * 4 {
        guard += 1;
        let mut clipped = false;
        'outer: for k in 0..index.len() {
            let i0 = index[(k + index.len() - 1) % index.len()];
            let i1 = index[k];
            let i2 = index[(k + 1) % index.len()];
            let (a, b, c) = (ring[i0], ring[i1], ring[i2]);
            let area = cross(a, b, c);
            if area <= eps {
                // Degenerate ear: collapse duplicate/collinear vertices so the
                // clip cannot stall on the bridge duplicates.
                if close(a, b) || close(b, c) || close(a, c) {
                    index.remove(k);
                    clipped = true;
                    break;
                }
                continue; // reflex or collinear
            }
            // No other vertex inside the candidate ear.
            for &m in &index {
                if m == i0 || m == i1 || m == i2 {
                    continue;
                }
                let p = ring[m];
                if close(p, a) || close(p, b) || close(p, c) {
                    continue; // duplicates sit on the ear, not inside it
                }
                let d1 = cross(a, b, p);
                let d2 = cross(b, c, p);
                let d3 = cross(c, a, p);
                if d1 >= -eps && d2 >= -eps && d3 >= -eps {
                    continue 'outer;
                }
            }
            tris.push([i0, i1, i2]);
            index.remove(k);
            clipped = true;
            break;
        }
        if !clipped {
            break;
        }
    }
    if index.len() == 3 {
        tris.push([index[0], index[1], index[2]]);
    }
    tris
}

/// Newell normal of a 3D polygon.
pub fn poly_normal(pts: &[[f64; 3]]) -> [f64; 3] {
    let mut n = [0.0f64; 3];
    let len = pts.len();
    for i in 0..len {
        let a = pts[i];
        let b = pts[(i + 1) % len];
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l < 1e-12 {
        [0.0, 0.0, 1.0]
    } else {
        [n[0] / l, n[1] / l, n[2] / l]
    }
}

/// Accumulated triangle soup; both readers push into one of these and
/// convert to a `MeshModel` at the end.
#[derive(Default)]
pub struct TriSink {
    pub tris: Vec<[[f64; 3]; 3]>,
}

/// Maximum feature-edge segments extracted per mesh — bounds memory and the
/// pick scan on pathological triangle soups.
const MAX_FEATURE_EDGES: usize = 300_000;

/// Feature edges of a flat triangle list (`verts` in triangle order):
/// boundary edges plus shared edges whose two face normals deviate by more
/// than 30°. Positions are quantised against the mesh diagonal so triangle-
/// soup vertices merge. Returns (high, low_residual) pair lists ready for
/// `MeshLodSet::edge_verts` / `edge_verts_low`.
pub fn feature_edges(verts: &[[f32; 3]]) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for p in verts {
        for k in 0..3 {
            let v = p[k] as f64;
            min[k] = min[k].min(v);
            max[k] = max[k].max(v);
        }
    }
    let diag = ((max[0] - min[0]).powi(2)
        + (max[1] - min[1]).powi(2)
        + (max[2] - min[2]).powi(2))
    .sqrt();
    let cell = (diag / 1e7).max(1e-9);
    let key = |p: [f32; 3]| -> [i64; 3] {
        [
            ((p[0] as f64 - min[0]) / cell).round() as i64,
            ((p[1] as f64 - min[1]) / cell).round() as i64,
            ((p[2] as f64 - min[2]) / cell).round() as i64,
        ]
    };

    struct EdgeInfo {
        count: u8,
        normal_a: [f64; 3],
        normal_b: [f64; 3],
        a: [f32; 3],
        b: [f32; 3],
    }
    let mut table: std::collections::HashMap<([i64; 3], [i64; 3]), EdgeInfo> =
        std::collections::HashMap::new();

    let mut normals: Vec<[f64; 3]> = Vec::with_capacity(verts.len() / 3);
    for tri in verts.chunks_exact(3) {
        let ab = [
            tri[1][0] as f64 - tri[0][0] as f64,
            tri[1][1] as f64 - tri[0][1] as f64,
            tri[1][2] as f64 - tri[0][2] as f64,
        ];
        let ac = [
            tri[2][0] as f64 - tri[0][0] as f64,
            tri[2][1] as f64 - tri[0][1] as f64,
            tri[2][2] as f64 - tri[0][2] as f64,
        ];
        let n = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        normals.push(if len < 1e-12 {
            [0.0; 3]
        } else {
            [n[0] / len, n[1] / len, n[2] / len]
        });
    }

    for (t, tri) in verts.chunks_exact(3).enumerate() {
        let normal = normals[t];
        for k in 0..3 {
            let pa = tri[k];
            let pb = tri[(k + 1) % 3];
            let (ka, kb) = (key(pa), key(pb));
            let edge_key = if ka <= kb { (ka, kb) } else { (kb, ka) };
            let entry = table.entry(edge_key).or_insert(EdgeInfo {
                count: 0,
                normal_a: [0.0; 3],
                normal_b: [0.0; 3],
                a: pa,
                b: pb,
            });
            match entry.count {
                0 => {
                    entry.count = 1;
                    entry.normal_a = normal;
                    entry.a = pa;
                    entry.b = pb;
                }
                1 => {
                    entry.count = 2;
                    entry.normal_b = normal;
                }
                _ => entry.count = 3, // non-manifold
            }
        }
    }

    let cos_threshold = 30f64.to_radians().cos();
    let mut high: Vec<[f32; 3]> = Vec::new();
    let mut low: Vec<[f32; 3]> = Vec::new();
    for (_, edge) in table {
        let feature = match edge.count {
            1 => true, // boundary
            2 => dot(edge.normal_a, edge.normal_b) < cos_threshold,
            _ => true, // non-manifold: show it
        };
        if !feature {
            continue;
        }
        if high.len() >= MAX_FEATURE_EDGES * 2 {
            break;
        }
        high.push(edge.a);
        high.push(edge.b);
        low.push([0.0; 3]);
        low.push([0.0; 3]);
    }
    (high, low)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

impl TriSink {
    pub fn push(&mut self, a: [f64; 3], b: [f64; 3], c: [f64; 3]) {
        self.tris.push([a, b, c]);
    }

    /// Fan-triangulate a 3D loop.
    pub fn push_loop(&mut self, pts: &[[f64; 3]]) {
        for k in 1..pts.len().saturating_sub(1) {
            self.push(pts[0], pts[k], pts[k + 1]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_a_square() {
        let sq = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        let tris = ear_clip(&sq, &[]);
        assert_eq!(tris.len(), 2);
        assert!(tris.iter().flatten().all(|p| sq.contains(p)));
    }

    #[test]
    fn bridges_a_hole() {
        let outer = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let hole = [[4.0, 4.0], [6.0, 4.0], [6.0, 6.0], [4.0, 6.0]];
        let tris = ear_clip(&outer, &[&hole]);
        // A square with a square hole triangulates to 8 real triangles.
        assert_eq!(tris.len(), 8);
        for tri in &tris {
            let centroid = [
                (tri[0][0] + tri[1][0] + tri[2][0]) / 3.0,
                (tri[0][1] + tri[1][1] + tri[2][1]) / 3.0,
            ];
            assert!(point_in_loops(centroid, &[&outer, &hole]), "centroid {centroid:?}");
            assert!(!point_in_polygon(centroid, &hole), "centroid {centroid:?} inside hole");
        }
    }

    #[test]
    fn even_odd_loop_test_handles_holes() {
        let outer = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        let hole = [[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0]];
        assert!(point_in_loops([0.5, 0.5], &[&outer, &hole]));
        assert!(!point_in_loops([2.0, 2.0], &[&outer, &hole]));
    }
}
