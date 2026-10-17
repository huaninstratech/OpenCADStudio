//! Model files IMPORT reads: ACIS text (SAT), STEP and IGES, as the drawing
//! objects they become.

use codec::entities::{Entity as _, Insert};
use codec::types::{Color, Matrix4, Transform, Vector3};
use codec::EntityType;

use crate::scene::convert::acis_import::{painted_body_entity, sat_entities};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFormat {
    Sat,
    Step,
    Iges,
}

/// The objects a file brings in.
pub struct Imported {
    /// Block definitions, each before any block that inserts it.
    pub blocks: Vec<(String, Vec<EntityType>)>,
    /// The objects for the current space.
    pub entities: Vec<EntityType>,
}

/// File units over drawing units; one where the drawing is unitless.
fn ratio(file_mm: f64, drawing_mm: Option<f64>) -> f64 {
    drawing_mm.filter(|mm| *mm > 0.0).map_or(1.0, |mm| file_mm / mm)
}

/// A block name from a product's, with the characters names cannot hold
/// replaced.
fn block_name(name: &str) -> String {
    crate::scene::sanitized_table_name(name, "Part")
}

/// A STEP presentation colour as the drawing colour it imports as: the
/// standard index colour it is close to — red, yellow, green, cyan, blue,
/// magenta, white, or the dark grey 250 (which black also becomes) — and
/// otherwise the true colour.
// ponytail: the closeness bands are fitted to sample files (saturation
// at least 0.75, value at least 0.825 and hue within 7.5 degrees for the six
// hues; greys from 0.16 to 0.39 for 250 and from 0.935 for white); refit
// them if a colour on a band edge comes out differently.
pub fn step_colour(rgb: crate::io::step_import::Rgb) -> Color {
    let [r, g, b] = rgb.map(|c| c.clamp(0.0, 1.0));
    let true_colour = Color::Rgb {
        r: (r * 255.0).round() as u8,
        g: (g * 255.0).round() as u8,
        b: (b * 255.0).round() as u8,
    };
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max == 0.0 {
        return Color::Index(250);
    }
    let saturation = (max - min) / max;
    if saturation <= 0.1 {
        return if max >= 0.935 {
            Color::Index(7)
        } else if (0.16..0.395).contains(&max) {
            Color::Index(250)
        } else {
            true_colour
        };
    }
    if saturation < 0.75 || max < 0.825 {
        return true_colour;
    }
    let delta = max - min;
    let hue = if max == r {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    let sector = (hue / 60.0).round();
    if (hue - 60.0 * sector).abs() > 7.5 {
        return true_colour;
    }
    // Red, yellow, green, cyan, blue, magenta.
    Color::Index([1, 2, 3, 4, 5, 6][sector as usize % 6])
}

/// An insert of `block` placed by `placement` (columns: axes, origin).
fn placed_insert(block: &str, placement: &[[f64; 4]; 3]) -> EntityType {
    let mut insert = Insert::new(block, Vector3::ZERO);
    let mut m = Matrix4::identity();
    for r in 0..3 {
        m.m[r] = placement[r];
    }
    insert.apply_transform(&Transform::from_matrix(m));
    EntityType::Insert(insert)
}

/// STEP: a block per product — its solids, and inserts of its components —
/// with each top product inserted at the origin, scaled from the file's
/// units to the drawing's. The solids take their colour from the block.
fn step_blocks(model: crate::io::step_import::StepModel, scale: f64) -> Imported {
    // Unique names, in product order.
    let mut names: Vec<String> = Vec::new();
    for product in &model.products {
        let base = block_name(&product.name);
        let mut name = base.clone();
        let mut n = 1;
        while names.iter().any(|taken| taken.eq_ignore_ascii_case(&name)) {
            name = format!("{base}_{n}");
            n += 1;
        }
        names.push(name);
    }
    // Children before parents. A link that closes a loop in the file's
    // assembly (a product that would contain itself) is dropped, so no
    // block ends up inserting itself.
    let mut order = Vec::new();
    let mut dropped: std::collections::HashSet<(usize, usize)> = Default::default();
    let mut state = vec![0u8; model.products.len()];
    for &root in &model.roots {
        if state[root] != 0 {
            continue;
        }
        state[root] = 1;
        // Each product being walked and the next of its children to visit.
        let mut stack = vec![(root, 0usize)];
        while let Some((k, next)) = stack.last_mut() {
            let k = *k;
            match model.products[k].children.get(*next) {
                Some(&(child, _)) => {
                    let link = (k, *next);
                    *next += 1;
                    match state[child] {
                        0 => {
                            state[child] = 1;
                            stack.push((child, 0));
                        }
                        1 => {
                            dropped.insert(link);
                        }
                        _ => {}
                    }
                }
                None => {
                    state[k] = 2;
                    order.push(k);
                    stack.pop();
                }
            }
        }
    }
    let blocks = order
        .iter()
        .map(|&k| {
            let product = &model.products[k];
            let mut entities: Vec<EntityType> = product
                .bodies
                .iter()
                .filter_map(|body| {
                    let faces: Vec<Option<Color>> = body.faces.iter().map(|c| c.map(step_colour)).collect();
                    painted_body_entity(&body.body, body.colour.map(step_colour), &faces, false)
                })
                .collect();
            entities.extend(
                product
                    .children
                    .iter()
                    .enumerate()
                    .filter(|(n, _)| !dropped.contains(&(k, *n)))
                    .map(|(_, (child, placement))| {
                        let mut insert = placed_insert(&names[*child], placement);
                        insert.common_mut().color = Color::ByBlock;
                        insert
                    }),
            );
            (names[k].clone(), entities)
        })
        .collect();
    let identity = [[scale, 0.0, 0.0, 0.0], [0.0, scale, 0.0, 0.0], [0.0, 0.0, scale, 0.0]];
    Imported {
        blocks,
        entities: model
            .roots
            .iter()
            .map(|&k| placed_insert(&names[k], &identity))
            .collect(),
    }
}

/// `None` when the file is not of `format` or holds nothing it can read.
pub fn read(format: ModelFormat, bytes: &[u8], drawing_mm: Option<f64>) -> Option<Imported> {
    let imported = match format {
        ModelFormat::Sat => Imported {
            blocks: Vec::new(),
            entities: sat_entities(&String::from_utf8_lossy(bytes))?,
        },
        ModelFormat::Step => {
            let model = crate::io::step_import::read(bytes)?;
            let scale = ratio(model.unit_mm, drawing_mm);
            step_blocks(model, scale)
        }
        ModelFormat::Iges => {
            // Solids, then the surfaces as surfaces however flat; faces and
            // edges carry the object's colour.
            let model = crate::io::iges_import::read(bytes, drawing_mm)?;
            let solids = model.bodies.iter().map(|body| (body, false));
            let sheets = model.sheets.iter().map(|body| (body, true));
            Imported {
                blocks: Vec::new(),
                entities: solids
                    .chain(sheets)
                    .filter_map(|((body, color), sheet)| painted_body_entity(body, Some(*color), &[], sheet))
                    .collect(),
            }
        }
    };
    let solids = imported
        .blocks
        .iter()
        .flat_map(|(_, entities)| entities)
        .chain(&imported.entities)
        .any(|entity| !matches!(entity, EntityType::Insert(_)));
    solids.then_some(imported)
}
