// PDFIMPORT — turn the vector content of a PDF underlay into drawing objects.
//
//   Select PDF underlay or [File] <File>:
//   Specify first corner of area to import or [Polygonal/All/Settings] <All>:
//   Specify opposite corner:
//   Keep, Detach or Unload PDF underlay? [Keep/Detach/Unload] <Unload>:
//   Binding PDF file <path>, page <n> ...
//
// Content of a PDF layer goes to PDF_<layer>; the rest to PDF_Geometry,
// PDF_Solid Fills, PDF_Text and PDF_Images (or those alone, or the current
// layer, per the settings). Straight strokes that meet become polylines, a
// Bézier that is a circular arc an arc segment of them (with "Join" on) or
// an ARC (off), a closed four-arc Bézier loop that is a circle a CIRCLE,
// other curves splines, filled triangles and quadrilaterals SOLIDs at 50% transparency
// (other fills solid hatches), text runs MTEXT in a "PDF <font>" style and
// raster images PNG files referenced by IMAGE objects.

use codec::entities::{
    Arc, AttachmentPoint, BoundaryEdge, BoundaryPath, Circle, Hatch, LwPolyline, MText,
    PolylineEdge, Solid, Spline, Underlay, UnderlayType,
};
use codec::types::{Color, Handle, LineWeight, Vector2, Vector3};
use codec::EntityType;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::scene::model::pdf_vector::{
    bezier, circle_of as as_circle, PageVectors, PdfPath, Segment, SubPath,
};
use crate::scene::model::wire_model::WireModel;

/// Which layers imported objects go to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportLayers {
    /// The PDF's own layers ("PDF_<name>"); content outside them to
    /// PDF_Geometry / PDF_Text.
    Pdf,
    /// One layer per kind of object.
    Object,
    /// The current layer.
    Current,
}

/// The PDF Import Settings, kept for the session. The options below the
/// data section are PDFIMPORTMODE's bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PdfImportSettings {
    pub vector: bool,
    pub fills: bool,
    pub text: bool,
    pub raster: bool,
    pub layers: ImportLayers,
    pub as_block: bool,
    pub join: bool,
    pub hatches: bool,
    pub lineweights: bool,
    pub linetypes: bool,
}

impl Default for PdfImportSettings {
    fn default() -> Self {
        Self {
            vector: true,
            fills: true,
            text: true,
            raster: false,
            layers: ImportLayers::Pdf,
            as_block: false,
            join: true,
            hatches: false,
            lineweights: true,
            linetypes: false,
        }
    }
}

impl PdfImportSettings {
    /// PDFIMPORTMODE: 1 import as block, 2 apply lineweights, 4 join
    /// segments, 8 fills as hatches, 16 infer linetypes (default 6).
    pub fn mode(&self) -> i16 {
        i16::from(self.as_block)
            | i16::from(self.lineweights) << 1
            | i16::from(self.join) << 2
            | i16::from(self.hatches) << 3
            | i16::from(self.linetypes) << 4
    }

    /// PDFIMPORTFILTER: the kinds left out — 1 vector geometry, 2 text,
    /// 4 solid fills, 8 raster images (default 8).
    pub fn filter(&self) -> i16 {
        i16::from(!self.vector)
            | i16::from(!self.text) << 1
            | i16::from(!self.fills) << 2
            | i16::from(!self.raster) << 3
    }

    pub fn set_filter(&mut self, filter: i16) {
        self.vector = filter & 1 == 0;
        self.text = filter & 2 == 0;
        self.fills = filter & 4 == 0;
        self.raster = filter & 8 == 0;
    }

    /// PDFIMPORTLAYERS: 0 PDF layers, 1 object layers, 2 current layer.
    pub fn layers_value(&self) -> i16 {
        match self.layers {
            ImportLayers::Pdf => 0,
            ImportLayers::Object => 1,
            ImportLayers::Current => 2,
        }
    }

    pub fn set_mode(&mut self, mode: i16) {
        self.as_block = mode & 1 != 0;
        self.lineweights = mode & 2 != 0;
        self.join = mode & 4 != 0;
        self.hatches = mode & 8 != 0;
        self.linetypes = mode & 16 != 0;
    }
}

static SETTINGS: std::sync::Mutex<Option<PdfImportSettings>> = std::sync::Mutex::new(None);

pub fn import_settings() -> PdfImportSettings {
    SETTINGS
        .lock()
        .map(|s| s.unwrap_or_default())
        .unwrap_or_default()
}

pub fn set_import_settings(settings: PdfImportSettings) {
    if let Ok(mut s) = SETTINGS.lock() {
        *s = Some(settings);
    }
}

static IMAGE_PATH: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// PDFIMPORTIMAGEPATH: the folder raster images are extracted to — relative
/// to the folder of the PDF, absolute as given, empty for that folder itself.
pub fn image_path() -> String {
    IMAGE_PATH
        .lock()
        .ok()
        .and_then(|p| p.clone())
        .unwrap_or_else(|| "PDF Images".to_string())
}

pub fn set_image_path(path: String) {
    if let Ok(mut p) = IMAGE_PATH.lock() {
        *p = Some(path);
    }
}

/// The extraction folder for the PDF at `pdf`.
pub fn image_dir(pdf: &str) -> Option<std::path::PathBuf> {
    let setting = image_path();
    let folder = std::path::Path::new(&setting);
    if folder.is_absolute() {
        return Some(folder.to_path_buf());
    }
    let parent = std::path::Path::new(pdf).parent()?;
    Some(if setting.is_empty() {
        parent.to_path_buf()
    } else {
        parent.join(folder)
    })
}

#[derive(Clone, Debug, PartialEq)]
pub enum ImportArea {
    All,
    Polygon(Vec<DVec3>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnderlayMode {
    Keep,
    Detach,
    Unload,
}

#[derive(Clone, Debug)]
pub struct PdfImportRequest {
    pub underlay: Handle,
    pub area: ImportArea,
    pub mode: UnderlayMode,
}

/// A page of a file imported without an underlay (the File option): placed
/// at `insertion` with a scale and rotation.
#[derive(Clone, Debug)]
pub struct PdfFileImport {
    pub path: String,
    pub page: String,
    pub scale: f64,
    pub rotation: f64,
    pub insertion: DVec3,
}

impl PdfFileImport {
    /// Where the page's content goes: one drawing unit per page inch times
    /// the scale, or, on a page with a viewport measure (a plot), its
    /// measured units (a 1:1 millimetre plot comes in at its millimetres).
    pub fn placement(&self) -> Underlay {
        let mut u = Underlay::pdf();
        u.insertion_point = Vector3::new(self.insertion.x, self.insertion.y, self.insertion.z);
        let units = crate::scene::model::pdf_raster::page_units_per_inch(&self.path, &self.page)
            .unwrap_or(1.0);
        u.set_scale(self.scale * units);
        u.rotation = self.rotation;
        u
    }
}

/// Import PDF with the insertion point asked on screen.
pub struct PdfImportPointCommand {
    import: PdfFileImport,
}

impl PdfImportPointCommand {
    pub fn new(import: PdfFileImport) -> Self {
        Self { import }
    }
}

impl CadCommand for PdfImportPointCommand {
    fn name(&self) -> &'static str {
        "PDFIMPORT"
    }
    fn prompt(&self) -> String {
        "Specify insertion point:".to_string()
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        self.import.insertion = pt;
        CmdResult::PdfImportFile(self.import.clone())
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Select,
    Area,
    Opposite,
    Polygon,
    Mode,
    // The File option on the command line (-PDFIMPORT, or FILEDIA 0).
    FileName,
    Page,
    PageList,
    Insertion,
    Scale,
    Rotation,
}

pub struct PdfImportCommand {
    step: Step,
    handle: Handle,
    picked: Option<EntityType>,
    points: Vec<DVec3>,
    area: ImportArea,
    /// -PDFIMPORT: the file is always asked on the command line.
    dash: bool,
    file: PdfFileImport,
    pages: usize,
}

impl PdfImportCommand {
    /// Import from an underlay already chosen (the underlay tab's button):
    /// starts at the area prompt.
    pub fn for_underlay(handle: Handle) -> Self {
        Self {
            step: Step::Area,
            handle,
            ..Self::new()
        }
    }

    pub fn new() -> Self {
        Self {
            step: Step::Select,
            handle: Handle::NULL,
            picked: None,
            points: Vec::new(),
            area: ImportArea::All,
            dash: false,
            file: PdfFileImport {
                path: String::new(),
                page: "1".to_string(),
                scale: 1.0,
                rotation: 0.0,
                insertion: DVec3::ZERO,
            },
            pages: 0,
        }
    }

    /// -PDFIMPORT.
    pub fn command_line() -> Self {
        Self {
            dash: true,
            ..Self::new()
        }
    }

    /// The File option: the file dialog while FILEDIA is on (PDFIMPORT
    /// only), otherwise the file name prompt.
    fn file_option(&mut self) -> CmdResult {
        if !self.dash && crate::sys::file_dia() {
            return CmdResult::Dispatch("_PDFIMPORTFILE".to_string());
        }
        self.step = Step::FileName;
        CmdResult::NeedPoint
    }

    fn file_name(&mut self, text: &str) -> CmdResult {
        let typed = text.trim().trim_matches('"');
        if typed.is_empty() {
            return CmdResult::ReportError("Filename cannot be blank".to_string());
        }
        // A name without an extension is a .pdf file.
        let path = if std::path::Path::new(typed).extension().is_none() {
            format!("{typed}.pdf")
        } else {
            typed.to_string()
        };
        let shown = crate::entities::underlay::display_path(&path);
        let Ok(bytes) = std::fs::read(&path) else {
            return CmdResult::ReportError(format!("File {shown} does not exist."));
        };
        crate::scene::model::pdf_raster::register_source(&path, std::sync::Arc::new(bytes));
        let Some(pages) = crate::scene::model::pdf_raster::page_count(&path) else {
            // ponytail: wording for an unreadable PDF not measured.
            return CmdResult::ReportError(format!("{shown} is not a valid PDF file."));
        };
        self.pages = pages;
        self.file.path = path;
        self.step = Step::Page;
        CmdResult::NeedPoint
    }

    fn page(&mut self, text: &str) -> CmdResult {
        let token = text.trim();
        if token == "?" {
            self.step = Step::PageList;
            return CmdResult::NeedPoint;
        }
        let page = if token.is_empty() {
            1
        } else {
            match token.parse::<i64>() {
                Ok(n) if n > 0 => n as usize,
                // Zero is asked again without a message.
                Ok(0) => return CmdResult::NeedPoint,
                _ => {
                    return CmdResult::ReportError("Please enter page number or '?'.".to_string())
                }
            }
        };
        // A page past the last one is asked again without a message.
        if page > self.pages {
            return CmdResult::NeedPoint;
        }
        self.file.page = page.to_string();
        self.step = Step::Insertion;
        CmdResult::NeedPoint
    }

    fn page_list(&mut self, text: &str) -> CmdResult {
        let token = text.trim();
        self.step = Step::Page;
        if token.is_empty() || token == "*" {
            let all: Vec<String> = (1..=self.pages).map(|n| n.to_string()).collect();
            return CmdResult::ReportMeasurement(all.join("\n"));
        }
        match token.parse::<usize>() {
            Ok(n) if (1..=self.pages).contains(&n) => {
                CmdResult::ReportMeasurement(format!("Found page {n}."))
            }
            _ => CmdResult::ReportMeasurement("No matching page found.".to_string()),
        }
    }

    fn scale(&mut self, value: f64) -> CmdResult {
        if value <= 0.0 {
            return CmdResult::ReportError("Value must be positive and nonzero.".to_string());
        }
        self.file.scale = value;
        self.step = Step::Rotation;
        CmdResult::NeedPoint
    }

    fn rotation(&mut self, radians: f64) -> CmdResult {
        self.file.rotation = radians;
        CmdResult::PdfImportFile(self.file.clone())
    }

    fn area_choice(&mut self, text: &str) -> CmdResult {
        match text.trim().to_ascii_uppercase().as_str() {
            "" | "A" | "ALL" => {
                self.area = ImportArea::All;
                self.step = Step::Mode;
                CmdResult::NeedPoint
            }
            "P" | "POLYGONAL" => {
                self.points.clear();
                self.step = Step::Polygon;
                CmdResult::NeedPoint
            }
            "S" | "SETTINGS" => CmdResult::OpenPdfImportSettings,
            _ => CmdResult::ReportError("Invalid option keyword.".to_string()),
        }
    }

    fn mode_choice(&mut self, text: &str) -> CmdResult {
        let mode = match text.trim().to_ascii_uppercase().as_str() {
            "" | "U" | "UNLOAD" => UnderlayMode::Unload,
            "K" | "KEEP" => UnderlayMode::Keep,
            "D" | "DETACH" => UnderlayMode::Detach,
            _ => return CmdResult::ReportError("Invalid option keyword.".to_string()),
        };
        CmdResult::PdfImport(PdfImportRequest {
            underlay: self.handle,
            area: self.area.clone(),
            mode,
        })
    }
}

impl CadCommand for PdfImportCommand {
    fn name(&self) -> &'static str {
        if self.dash {
            "-PDFIMPORT"
        } else {
            "PDFIMPORT"
        }
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::Select => "Select PDF underlay or [File] <File>:".to_string(),
            Step::Area => {
                "Specify first corner of area to import or [Polygonal/All/Settings] <All>:"
                    .to_string()
            }
            Step::Opposite => "Specify opposite corner:".to_string(),
            Step::Polygon if self.points.is_empty() => "Specify first point:".to_string(),
            Step::Polygon => "Specify next point or [Undo]:".to_string(),
            Step::Mode => {
                "Keep, Detach or Unload PDF underlay? [Keep/Detach/Unload] <Unload>:".to_string()
            }
            Step::FileName => "Enter filename for PDF import:".to_string(),
            Step::Page => "Enter page number or [?] <1>:".to_string(),
            Step::PageList => "Enter page to list <*>:".to_string(),
            Step::Insertion => "Specify insertion point <0,0>:".to_string(),
            Step::Scale => "Specify scale factor <1.0>:".to_string(),
            Step::Rotation => "Specify rotation <0>:".to_string(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Select => vec![CmdOption::new("File", "F")],
            Step::Area => vec![
                CmdOption::new("Polygonal", "P"),
                CmdOption::new("All", "A"),
                CmdOption::new("Settings", "S"),
            ],
            Step::Polygon if !self.points.is_empty() => vec![CmdOption::new("Undo", "U")],
            Step::Mode => vec![
                CmdOption::new("Keep", "K"),
                CmdOption::new("Detach", "D"),
                CmdOption::new("Unload", "U"),
            ],
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Mode | Step::Page | Step::PageList => InputKind::SingleToken,
            Step::FileName => InputKind::FreeText,
            _ => InputKind::Point,
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(
            self.step,
            Step::Select | Step::Area | Step::Polygon | Step::Scale | Step::Rotation
        )
    }

    fn needs_entity_pick(&self) -> bool {
        self.step == Step::Select
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        match self.picked.take() {
            Some(EntityType::Underlay(u)) if u.underlay_type == UnderlayType::Pdf => {
                self.handle = handle;
                self.step = Step::Area;
                CmdResult::NeedPoint
            }
            _ => CmdResult::ReportError("Object selected was not a PDF underlay.".to_string()),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::Area => {
                self.points = vec![pt];
                self.step = Step::Opposite;
                CmdResult::NeedPoint
            }
            Step::Opposite => {
                let a = self.points[0];
                self.area = ImportArea::Polygon(vec![
                    a,
                    DVec3::new(pt.x, a.y, a.z),
                    pt,
                    DVec3::new(a.x, pt.y, a.z),
                ]);
                self.step = Step::Mode;
                CmdResult::NeedPoint
            }
            Step::Polygon => {
                self.points.push(pt);
                CmdResult::NeedPoint
            }
            Step::Insertion => {
                self.file.insertion = pt;
                self.step = Step::Scale;
                CmdResult::NeedPoint
            }
            // A second point gives the scale, or the angle, from the
            // insertion point.
            Step::Scale => {
                let d = (pt - self.file.insertion).truncate().length();
                self.scale(d)
            }
            Step::Rotation => {
                let d = (pt - self.file.insertion).truncate();
                self.rotation(d.y.atan2(d.x))
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let token = text.trim().to_ascii_uppercase();
        Some(match self.step {
            Step::Select if matches!(token.as_str(), "F" | "FILE" | "_F" | "_FILE") => {
                self.file_option()
            }
            Step::Area => self.area_choice(&token),
            Step::Polygon if token == "U" || token == "UNDO" => {
                self.points.pop();
                CmdResult::NeedPoint
            }
            Step::Mode => self.mode_choice(&token),
            Step::FileName => self.file_name(text),
            Step::Page => self.page(text),
            Step::PageList => self.page_list(text),
            Step::Scale => match token.parse::<f64>() {
                Ok(v) => self.scale(v),
                Err(_) if token.is_empty() => self.scale(1.0),
                // ponytail: wording for a non-number scale not measured.
                Err(_) => CmdResult::ReportError(
                    "Requires numeric distance or second point.".to_string(),
                ),
            },
            Step::Rotation => match token.parse::<f64>() {
                Ok(v) => self.rotation(v.to_radians()),
                Err(_) if token.is_empty() => self.rotation(0.0),
                Err(_) => CmdResult::ReportError(
                    "Requires valid numeric angle or second point.".to_string(),
                ),
            },
            _ => return None,
        })
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Select => self.file_option(),
            Step::Area => self.area_choice(""),
            Step::Polygon if self.points.len() >= 3 => {
                self.area = ImportArea::Polygon(self.points.clone());
                self.step = Step::Mode;
                CmdResult::NeedPoint
            }
            Step::Polygon => CmdResult::NeedPoint,
            Step::Mode => self.mode_choice(""),
            Step::Opposite => CmdResult::NeedPoint,
            Step::FileName => self.file_name(""),
            Step::Page => self.page(""),
            Step::PageList => self.page_list(""),
            Step::Insertion => self.on_point(DVec3::ZERO),
            Step::Scale => self.scale(1.0),
            Step::Rotation => self.rotation(0.0),
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let ring: Vec<DVec3> = match self.step {
            Step::Opposite => {
                let a = self.points[0];
                vec![
                    a,
                    DVec3::new(pt.x, a.y, a.z),
                    pt,
                    DVec3::new(a.x, pt.y, a.z),
                ]
            }
            Step::Polygon if !self.points.is_empty() => {
                let mut ring = self.points.clone();
                ring.push(pt);
                ring
            }
            _ => return None,
        };
        let mut points: Vec<[f64; 3]> = ring.iter().map(|p| p.to_array()).collect();
        points.push(points[0]);
        Some(WireModel::solid_f64(
            "pdf_import_area".into(),
            points,
            WireModel::CYAN,
            false,
        ))
    }
}

// ── Conversion ───────────────────────────────────────────────────────────────

/// What an import adds: objects, and the layers and text styles they use
/// (name, colour / font file), created when missing.
#[derive(Default)]
pub struct ImportResult {
    pub entities: Vec<EntityType>,
    pub layers: Vec<(String, Color)>,
    pub text_styles: Vec<(String, String)>,
    pub images: Vec<ImageImport>,
    /// Some line uses the dash linetype.
    pub dashed: bool,
}

/// A PDF colour as an index colour when it is one of the basic seven,
/// otherwise as a true colour.
fn color_of(rgb: [u8; 3]) -> Color {
    match rgb {
        [255, 0, 0] => Color::Index(1),
        [255, 255, 0] => Color::Index(2),
        [0, 255, 0] => Color::Index(3),
        [0, 255, 255] => Color::Index(4),
        [0, 0, 255] => Color::Index(5),
        [255, 0, 255] => Color::Index(6),
        [0, 0, 0] | [255, 255, 255] => Color::Index(7),
        [r, g, b] => Color::Rgb { r, g, b },
    }
}

/// Stroke width in points → the nearest standard lineweight.
fn lineweight_of(width_pt: f64) -> LineWeight {
    const STANDARD: [i16; 24] = [
        0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158,
        200, 211,
    ];
    if width_pt <= 0.0 {
        return LineWeight::ByLayer;
    }
    let hundredths = width_pt * 25.4 / 72.0 * 100.0;
    let best = STANDARD
        .iter()
        .copied()
        .min_by(|a, b| {
            (*a as f64 - hundredths)
                .abs()
                .total_cmp(&(*b as f64 - hundredths).abs())
        })
        .unwrap_or(0);
    LineWeight::Value(best)
}

fn font_file_for(font: &str) -> String {
    let f = font.to_ascii_lowercase();
    if f.contains("times") {
        "times.ttf"
    } else if f.contains("courier") {
        "cour.ttf"
    } else {
        "arial.ttf"
    }
    .to_string()
}

fn inside(poly: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut odd = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            odd = !odd;
        }
        j = i;
    }
    odd
}

/// Whether a path reaches into the area: any sampled point inside it.
fn path_in_area(path: &PdfPath, area: Option<&[[f64; 2]]>) -> bool {
    let Some(area) = area else { return true };
    path.subpaths.iter().any(|sp| {
        sp.segments
            .iter()
            .any(|s| [0.0, 0.5, 1.0].iter().any(|t| inside(area, bezier(s, *t))))
    })
}

/// Where imported objects of each kind go.
pub struct LayerNaming {
    pub settings: PdfImportSettings,
    /// "PDF_", or "PDF2_", "PDF3_" … for the next files imported.
    pub prefix: String,
    /// The current layer, for [`ImportLayers::Current`].
    pub current: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Geometry,
    Fill,
    Text,
    Image,
}

impl LayerNaming {
    fn layer(&self, pdf_layer: Option<&str>, kind: Kind) -> String {
        let object = |kind| match kind {
            Kind::Geometry => "Geometry",
            Kind::Fill => "Solid Fills",
            Kind::Text => "Text",
            Kind::Image => "Images",
        };
        match (self.settings.layers, pdf_layer) {
            (ImportLayers::Current, _) => self.current.clone(),
            (ImportLayers::Pdf, Some(name)) => format!("{}{name}", self.prefix),
            _ => format!("{}{}", self.prefix, object(kind)),
        }
    }
}

/// An image the import places: its pixels go to a PNG file named by the
/// caller, then a raster image at `insertion` spanning `u` × `v` per pixel.
pub struct ImageImport {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub insertion: Vector3,
    pub u: Vector3,
    pub v: Vector3,
    pub layer: String,
}

/// The linetype collinear dashes become ("Infer linetypes"): dash 0.8,
/// space 0.2, scaled per object to the dash length.
pub const DASH_LINETYPE: &str = "PDF_IMPORT";

/// A stroke before it becomes an object: chains can still be joined or read
/// as dashes. `bulges[i]` bends the segment from point i to the next (the
/// closing one on a closed stroke; 0 after an open stroke's last point).
struct Stroke {
    layer: String,
    color: Color,
    weight: LineWeight,
    points: Vec<[f64; 2]>,
    bulges: Vec<f64>,
    closed: bool,
}

impl Stroke {
    fn straight(&self) -> bool {
        self.bulges.iter().all(|b| *b == 0.0)
    }

    fn reverse(&mut self) {
        self.points.reverse();
        let n = self.bulges.len();
        let mut bulges: Vec<f64> = self.bulges[..n - 1].iter().rev().map(|b| -b).collect();
        bulges.push(0.0);
        self.bulges = bulges;
    }
}

/// The bulge of a Bézier that is a circular arc (its quarter, half and
/// three-quarter points on the circle through its ends and midpoint), from
/// its midpoint's height over the chord; positive counter-clockwise.
fn arc_bulge(seg: &Segment) -> Option<f64> {
    let Segment::Cubic(p0, _, _, p3) = seg else {
        return None;
    };
    let m = bezier(seg, 0.5);
    let (ax, ay) = (m[0] - p0[0], m[1] - p0[1]);
    let (bx, by) = (p3[0] - p0[0], p3[1] - p0[1]);
    let d = 2.0 * (ax * by - ay * bx);
    let chord = bx.hypot(by);
    if d.abs() < 1e-12 || chord < 1e-9 {
        return None;
    }
    // Circle through p0, m, p3 (relative to p0).
    let (a2, b2) = (ax * ax + ay * ay, bx * bx + by * by);
    let c = [(by * a2 - ay * b2) / d, (ax * b2 - bx * a2) / d];
    let r = c[0].hypot(c[1]);
    let tol = r * 2e-3;
    let on = [0.25, 0.75].iter().all(|t| {
        let q = bezier(seg, *t);
        ((q[0] - p0[0] - c[0]).hypot(q[1] - p0[1] - c[1]) - r).abs() <= tol
    });
    if !on {
        return None;
    }
    let h = (ax * by - ay * bx) / chord;
    Some(2.0 * h / chord)
}

/// A subpath cut where its curves are not circular arcs.
enum Piece {
    /// Points, bulges (as in [`Stroke`]) and whether it closes.
    Run(Vec<[f64; 2]>, Vec<f64>, bool),
    /// A cubic that is no circular arc.
    Curve(Segment),
}

/// The straight-and-arc runs of a subpath between its other curves. A
/// subpath without other curves is one run (closed when it closes).
fn run_strokes(segments: &[Segment], closed: bool) -> Vec<Piece> {
    let whole = !segments
        .iter()
        .any(|s| matches!(s, Segment::Cubic(..)) && arc_bulge(s).is_none());
    let mut out = Vec::new();
    let mut points: Vec<[f64; 2]> = Vec::new();
    let mut bulges: Vec<f64> = Vec::new();
    let flush = |points: &mut Vec<[f64; 2]>, bulges: &mut Vec<f64>, out: &mut Vec<Piece>| {
        if points.len() > 1 {
            bulges.push(0.0);
            out.push(Piece::Run(
                std::mem::take(points),
                std::mem::take(bulges),
                false,
            ));
        }
        points.clear();
        bulges.clear();
    };
    for seg in segments {
        let bulge = match seg {
            Segment::Line(..) => Some(0.0),
            cubic => arc_bulge(cubic),
        };
        match bulge {
            Some(b) => {
                if points.is_empty() {
                    points.push(seg.start());
                }
                points.push(seg.end());
                bulges.push(b);
            }
            None => {
                flush(&mut points, &mut bulges, &mut out);
                out.push(Piece::Curve(seg.clone()));
            }
        }
    }
    if whole && closed && points.len() > 2 {
        // The closing point is the first again.
        points.pop();
        out.push(Piece::Run(points, bulges, true));
    } else {
        flush(&mut points, &mut bulges, &mut out);
    }
    out
}

/// Circle through three points: centre and radius.
fn circle_through(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<([f64; 2], f64)> {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1e-12 {
        return None;
    }
    let (a2, b2, c2) = (
        a[0] * a[0] + a[1] * a[1],
        b[0] * b[0] + b[1] * b[1],
        c[0] * c[0] + c[1] * c[1],
    );
    let x = (a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d;
    let y = (a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d;
    Some(([x, y], (a[0] - x).hypot(a[1] - y)))
}

/// An ellipse: centre, unit major direction, semi-axes.
struct Conic {
    center: [f64; 2],
    major: [f64; 2],
    a: f64,
    b: f64,
}

impl Conic {
    /// The ellipse through five points (none at their centroid).
    fn through(points: &[[f64; 2]; 5]) -> Option<Self> {
        let o = [
            points.iter().map(|p| p[0]).sum::<f64>() / 5.0,
            points.iter().map(|p| p[1]).sum::<f64>() / 5.0,
        ];
        // A x² + B xy + C y² + D x + E y = 1 about the centroid.
        let mut m = [[0.0f64; 6]; 5];
        for (row, p) in m.iter_mut().zip(points) {
            let (x, y) = (p[0] - o[0], p[1] - o[1]);
            *row = [x * x, x * y, y * y, x, y, 1.0];
        }
        for col in 0..5 {
            let pivot = (col..5).max_by(|i, j| m[*i][col].abs().total_cmp(&m[*j][col].abs()))?;
            m.swap(col, pivot);
            if m[col][col].abs() < 1e-18 {
                return None;
            }
            for row in 0..5 {
                if row != col {
                    let f = m[row][col] / m[col][col];
                    for k in col..6 {
                        m[row][k] -= f * m[col][k];
                    }
                }
            }
        }
        let s: Vec<f64> = (0..5).map(|i| m[i][5] / m[i][i]).collect();
        let (a, b, c, d, e) = (s[0], s[1], s[2], s[3], s[4]);
        let det = 4.0 * a * c - b * b;
        if det <= 0.0 {
            return None;
        }
        let x0 = (b * e - 2.0 * c * d) / det;
        let y0 = (b * d - 2.0 * a * e) / det;
        let f = a * x0 * x0 + b * x0 * y0 + c * y0 * y0 + d * x0 + e * y0 - 1.0;
        if f >= 0.0 {
            return None;
        }
        // Eigen-directions of the quadratic part; the smaller value is the
        // major axis.
        let theta = 0.5 * b.atan2(a - c);
        let (ct, st) = (theta.cos(), theta.sin());
        let q1 = a * ct * ct + b * ct * st + c * st * st;
        let q2 = a * st * st - b * ct * st + c * ct * ct;
        let (r1, r2) = ((-f / q1).sqrt(), (-f / q2).sqrt());
        let (major, ra, rb) = if r1 >= r2 {
            ([ct, st], r1, r2)
        } else {
            ([-st, ct], r2, r1)
        };
        Some(Self {
            center: [x0 + o[0], y0 + o[1]],
            major,
            a: ra,
            b: rb,
        })
    }

    /// (u, v) of a point in the ellipse's own frame, scaled to the unit
    /// circle.
    fn unit(&self, p: [f64; 2]) -> (f64, f64) {
        let (dx, dy) = (p[0] - self.center[0], p[1] - self.center[1]);
        let u = dx * self.major[0] + dy * self.major[1];
        let v = -dx * self.major[1] + dy * self.major[0];
        (u / self.a, v / self.b)
    }
}

/// Turning of a run of points about a centre: positive counter-clockwise.
fn sweep_sign(points: &[[f64; 2]], c: [f64; 2]) -> f64 {
    points
        .windows(2)
        .map(|w| (w[0][0] - c[0]) * (w[1][1] - c[1]) - (w[0][1] - c[1]) * (w[1][0] - c[0]))
        .sum::<f64>()
        .signum()
}

/// Minimum segments of a straight polyline read as a curve.
const CURVE_MIN_SEGMENTS: usize = 9;
/// Share of the vertices that must lie within tolerance of the curve.
const CURVE_INLIERS: f64 = 0.85;
/// Radial tolerance of an arc through the first, middle and last vertex.
const ARC_TOLERANCE: f64 = 0.07;
/// Tolerance of an ellipse through five vertices (in its unit circle).
const ELLIPSE_TOLERANCE: f64 = 0.015;
/// An ellipse at least this round is a circle.
const CIRCLE_RATIO: f64 = 0.96;

/// A joined stroke read as a curve, in drawing coordinates (`points`):
/// a run of arcs on one circle is an ARC (a CIRCLE when it closes); a
/// straight polyline of nine or more segments whose vertices lie on a
/// circular arc an ARC, on an ellipse a CIRCLE (round) or an ELLIPSE (an
/// elliptical arc when open).
// ponytail: tolerances fitted to measured samples (glyph outlines, noisy
// and exact polygons); vertices off the curve are allowed for 15%.
fn curve_of(points: &[[f64; 2]], bulges: &[f64], closed: bool, z: f64) -> Option<EntityType> {
    use std::f64::consts::TAU;
    let n = points.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    if segs == 0 {
        return None;
    }
    let arc = |c: [f64; 2], r: f64, first: [f64; 2], last: [f64; 2], ccw: bool| {
        let angle = |p: [f64; 2]| (p[1] - c[1]).atan2(p[0] - c[0]).rem_euclid(TAU);
        let (start, end) = if ccw {
            (angle(first), angle(last))
        } else {
            (angle(last), angle(first))
        };
        EntityType::Arc(Arc::from_center_radius_angles(
            Vector3::new(c[0], c[1], z),
            r,
            start,
            end,
        ))
    };
    // Arcs of one circle, all turning the same way.
    if bulges[..segs].iter().all(|b| *b != 0.0) {
        let turn = bulges[0].signum();
        let arcs: Vec<_> = (0..segs)
            .map(|i| {
                crate::entities::common::BulgeArc::from_bulge(
                    points[i],
                    points[(i + 1) % n],
                    bulges[i],
                )
            })
            .collect::<Option<_>>()?;
        let first = &arcs[0];
        let same = arcs.iter().zip(&bulges[..segs]).all(|(a, b)| {
            b.signum() == turn
                && (a.radius - first.radius).abs() <= first.radius * 1e-4
                && (a.center[0] - first.center[0]).hypot(a.center[1] - first.center[1])
                    <= first.radius * 1e-4
        });
        if !same || segs < 2 {
            return None;
        }
        return Some(if closed {
            EntityType::Circle(Circle::from_center_radius(
                Vector3::new(first.center[0], first.center[1], z),
                first.radius,
            ))
        } else {
            arc(first.center, first.radius, points[0], points[n - 1], turn > 0.0)
        });
    }
    if bulges.iter().any(|b| *b != 0.0) || segs < CURVE_MIN_SEGMENTS {
        return None;
    }
    let enough = |inliers: usize| inliers as f64 >= CURVE_INLIERS * n as f64;
    if !closed {
        if let Some((c, r)) = circle_through(points[0], points[n / 2], points[n - 1]) {
            let inliers = points
                .iter()
                .filter(|p| ((p[0] - c[0]).hypot(p[1] - c[1]) - r).abs() <= ARC_TOLERANCE * r)
                .count();
            if enough(inliers) {
                return Some(arc(c, r, points[0], points[n - 1], sweep_sign(points, c) > 0.0));
            }
        }
    }
    // Five vertices spread along the run: a fifth of a closed loop apart,
    // or the ends and quarters of an open one.
    let step = if closed {
        (n as f64 / 5.0).round()
    } else {
        (n - 1) as f64 / 4.0
    };
    let idx: Vec<usize> = (0..5)
        .map(|k| ((k as f64 * step).round() as usize) % n)
        .collect();
    let five = [
        points[idx[0]],
        points[idx[1]],
        points[idx[2]],
        points[idx[3]],
        points[idx[4]],
    ];
    let e = Conic::through(&five)?;
    let inliers = points
        .iter()
        .filter(|p| {
            let (u, v) = e.unit(**p);
            (u.hypot(v) - 1.0).abs() <= ELLIPSE_TOLERANCE
        })
        .count();
    if !enough(inliers) {
        return None;
    }
    if closed && e.b / e.a >= CIRCLE_RATIO {
        let (c, r) = circle_through(five[0], five[2], five[4])?;
        return Some(EntityType::Circle(Circle::from_center_radius(
            Vector3::new(c[0], c[1], z),
            r,
        )));
    }
    let param = |p: [f64; 2]| {
        let (u, v) = e.unit(p);
        v.atan2(u).rem_euclid(TAU)
    };
    let (start, mut end) = if closed {
        let s = param(points[0]);
        (s, s + TAU)
    } else if sweep_sign(points, e.center) > 0.0 {
        (param(points[0]), param(points[n - 1]))
    } else {
        (param(points[n - 1]), param(points[0]))
    };
    if end <= start {
        end += TAU;
    }
    let mut ellipse = codec::entities::Ellipse::from_center_axes(
        Vector3::new(e.center[0], e.center[1], z),
        Vector3::new(e.major[0] * e.a, e.major[1] * e.a, 0.0),
        e.b / e.a,
    );
    ellipse.start_parameter = start;
    ellipse.end_parameter = end;
    Some(EntityType::Ellipse(ellipse))
}

fn same_point(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-7 && (a[1] - b[1]).abs() < 1e-7
}

/// Open strokes of one layer, colour and weight that meet end to end become
/// one polyline (closed when the chain returns to its start). Straight ones
/// always do; ones with arcs only with `join` (off, their arcs stay apart).
fn join_strokes(strokes: Vec<Stroke>, join: bool) -> Vec<Stroke> {
    let mut out: Vec<Stroke> = Vec::new();
    let mut open: Vec<Stroke> = Vec::new();
    for s in strokes {
        if s.closed || (!join && !s.straight()) {
            out.push(s);
        } else {
            open.push(s);
        }
    }
    // Drawing order is kept: dashes read as a run only in the order drawn.
    while !open.is_empty() {
        let mut chain = open.remove(0);
        loop {
            let end = *chain.points.last().expect("strokes have points");
            let start = chain.points[0];
            let fits = |o: &Stroke| {
                o.layer == chain.layer && o.color == chain.color && o.weight == chain.weight
            };
            if let Some(i) = open.iter().position(|o| {
                fits(o)
                    && (same_point(o.points[0], end) || same_point(*o.points.last().unwrap(), end))
            }) {
                let mut next = open.remove(i);
                if !same_point(next.points[0], end) {
                    next.reverse();
                }
                chain.points.extend(next.points.into_iter().skip(1));
                chain.bulges.pop();
                chain.bulges.extend(next.bulges);
            } else if let Some(i) = open.iter().position(|o| {
                fits(o)
                    && (same_point(*o.points.last().unwrap(), start)
                        || same_point(o.points[0], start))
            }) {
                let mut prev = open.remove(i);
                if !same_point(*prev.points.last().unwrap(), start) {
                    prev.reverse();
                }
                prev.points.pop();
                prev.points.extend(chain.points);
                chain.points = prev.points;
                prev.bulges.pop();
                prev.bulges.extend(chain.bulges);
                chain.bulges = prev.bulges;
            } else {
                break;
            }
        }
        if chain.points.len() > 3 && same_point(chain.points[0], *chain.points.last().unwrap()) {
            chain.points.pop();
            chain.bulges.pop();
            chain.closed = true;
        }
        out.push(chain);
    }
    out
}

/// An open stroke cut into its straight runs and one stroke per arc.
fn split_at_arcs(stroke: Stroke) -> Vec<Stroke> {
    let mut out = Vec::new();
    let piece = |points: Vec<[f64; 2]>, bulges: Vec<f64>| Stroke {
        layer: stroke.layer.clone(),
        color: stroke.color,
        weight: stroke.weight,
        points,
        bulges,
        closed: false,
    };
    let mut run: Vec<[f64; 2]> = vec![stroke.points[0]];
    for i in 0..stroke.points.len() - 1 {
        let (a, b, bulge) = (stroke.points[i], stroke.points[i + 1], stroke.bulges[i]);
        if bulge == 0.0 {
            run.push(b);
            continue;
        }
        if run.len() > 1 {
            let n = run.len();
            out.push(piece(std::mem::take(&mut run), vec![0.0; n]));
        }
        out.push(piece(vec![a, b], vec![bulge, 0.0]));
        run = vec![b];
    }
    if run.len() > 1 {
        let n = run.len();
        out.push(piece(run, vec![0.0; n]));
    }
    out
}

/// Runs of three or more equal collinear dashes with equal gaps become one
/// line from the first dash's start to the last's end, with the dash length
/// (the linetype scale).
// ponytail: dashes are matched along one direction at a time in drawing
// order; patterns of mixed dash lengths stay separate lines.
fn infer_dashes(strokes: Vec<Stroke>) -> Vec<(Stroke, Option<f64>)> {
    let is_dash = |s: &Stroke| !s.closed && s.points.len() == 2 && s.straight();
    let len = |s: &Stroke| {
        let (a, b) = (s.points[0], s.points[1]);
        (b[0] - a[0]).hypot(b[1] - a[1])
    };
    let mut out: Vec<(Stroke, Option<f64>)> = Vec::new();
    let mut run: Vec<Stroke> = Vec::new();
    let flush = |run: &mut Vec<Stroke>, out: &mut Vec<(Stroke, Option<f64>)>| {
        if run.len() >= 3 {
            let dash = len(&run[0]);
            let mut line = run.remove(0);
            let last = run.pop().expect("three or more");
            line.points[1] = last.points[1];
            run.clear();
            out.push((line, Some(dash)));
        } else {
            out.extend(run.drain(..).map(|s| (s, None)));
        }
    };
    for s in strokes {
        if !is_dash(&s) {
            flush(&mut run, &mut out);
            out.push((s, None));
            continue;
        }
        let continues = run.last().is_some_and(|prev| {
            let first = &run[0];
            let (a, b) = (prev.points[0], prev.points[1]);
            let dir = [(b[0] - a[0]) / len(prev), (b[1] - a[1]) / len(prev)];
            let (c, d) = (s.points[0], s.points[1]);
            let gap = [c[0] - b[0], c[1] - b[1]];
            let along = gap[0] * dir[0] + gap[1] * dir[1];
            let across = (gap[0] * dir[1] - gap[1] * dir[0]).abs();
            let seg = [(d[0] - c[0]) / len(&s), (d[1] - c[1]) / len(&s)];
            let parallel = (seg[0] * dir[1] - seg[1] * dir[0]).abs() < 1e-6
                && seg[0] * dir[0] + seg[1] * dir[1] > 0.0;
            let first_gap = run.get(1).map(|second| {
                let (e, f) = (first.points[1], second.points[0]);
                (f[0] - e[0]).hypot(f[1] - e[1])
            });
            s.layer == prev.layer
                && s.color == prev.color
                && s.weight == prev.weight
                && parallel
                && across < 1e-6
                && along > 1e-9
                && (len(&s) - len(first)).abs() < 1e-6
                && first_gap.is_none_or(|g| (g - along).abs() < 1e-6)
        });
        if !continues {
            flush(&mut run, &mut out);
        }
        run.push(s);
    }
    flush(&mut run, &mut out);
    out
}

/// The objects an import creates from a page's content split by PDF layer
/// (see `page_content_by_layer`), placed as `underlay` places the page.
pub fn convert(
    content: &[(Option<String>, PageVectors)],
    underlay: &Underlay,
    area: &ImportArea,
    naming: &LayerNaming,
) -> ImportResult {
    let settings = &naming.settings;
    let world = |p: [f64; 2]| crate::entities::underlay::local_to_world(underlay, p);
    let world2 = |p: [f64; 2]| {
        let w = world(p);
        Vector2::new(w[0], w[1])
    };
    let world3 = |p: [f64; 2]| {
        let w = world(p);
        Vector3::new(w[0], w[1], w[2])
    };
    let z = underlay.insertion_point.z;
    let scale = underlay.x_scale.abs();
    // The area is tested in page space, where the page content lives.
    let area_local: Option<Vec<[f64; 2]>> = match area {
        ImportArea::All => None,
        ImportArea::Polygon(points) => Some(
            points
                .iter()
                .map(|p| {
                    let (c, s) = (underlay.rotation.cos(), underlay.rotation.sin());
                    let dx = p.x - underlay.insertion_point.x;
                    let dy = p.y - underlay.insertion_point.y;
                    [
                        (dx * c + dy * s) / underlay.x_scale,
                        (-dx * s + dy * c) / underlay.y_scale,
                    ]
                })
                .collect(),
        ),
    };
    let in_area = |p: [f64; 2]| area_local.as_deref().is_none_or(|a| inside(a, p));

    let mut result = ImportResult::default();
    let mut layer_colors: Vec<(String, Color)> = Vec::new();
    // A layer takes the colour of its first object; objects of that colour
    // then follow the layer.
    let mut color_on = |layer: &str, color: Color| -> Color {
        let layer_color = match layer_colors.iter().find(|(n, _)| n == layer) {
            Some((_, c)) => *c,
            None => {
                layer_colors.push((layer.to_string(), color));
                color
            }
        };
        if color == layer_color {
            Color::ByLayer
        } else {
            color
        }
    };
    let fill_transparency = codec::types::Transparency::from_alpha_value(0x0200_007F);
    let mut strokes: Vec<Stroke> = Vec::new();
    let mut curves: Vec<EntityType> = Vec::new();

    for (pdf_layer, page) in content {
        let pdf_layer = pdf_layer.as_deref();
        if settings.vector {
            for path in &page.paths {
                if !path_in_area(path, area_local.as_deref()) {
                    continue;
                }
                match (path.stroke, path.fill) {
                    (Some((rgb, width)), _) => {
                        let layer = naming.layer(pdf_layer, Kind::Geometry);
                        let color = color_on(&layer, color_of(rgb));
                        let weight = if settings.lineweights {
                            lineweight_of(width)
                        } else {
                            LineWeight::ByLayer
                        };
                        for sp in &path.subpaths {
                            if let Some((c, r)) = as_circle(sp) {
                                let w = world(c);
                                let mut circle = EntityType::Circle(Circle::from_center_radius(
                                    Vector3::new(w[0], w[1], z),
                                    r * scale,
                                ));
                                let common = circle.common_mut();
                                (common.layer, common.color, common.line_weight) =
                                    (layer.clone(), color, weight);
                                curves.push(circle);
                            } else {
                                // Straight and circular runs become polylines;
                                // each other curve its own one-span SPLINE.
                                let all_runs = run_strokes(&sp.segments, sp.closed);
                                for piece in all_runs {
                                    match piece {
                                        Piece::Run(points, bulges, closed) => {
                                            let stroke = Stroke {
                                                layer: layer.clone(),
                                                color,
                                                weight,
                                                points,
                                                bulges,
                                                closed,
                                            };
                                            if settings.join || stroke.straight() || stroke.closed
                                            {
                                                strokes.push(stroke);
                                            } else {
                                                // Join off: each arc apart, straight runs
                                                // between them.
                                                strokes.extend(split_at_arcs(stroke));
                                            }
                                        }
                                        Piece::Curve(seg) => {
                                            let one = SubPath {
                                                segments: vec![seg],
                                                closed: false,
                                            };
                                            let mut spline =
                                                EntityType::Spline(spline_of(&one, &world3));
                                            let common = spline.common_mut();
                                            (common.layer, common.color, common.line_weight) =
                                                (layer.clone(), color, weight);
                                            curves.push(spline);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    (None, Some(rgb)) if settings.fills => {
                        let layer = naming.layer(pdf_layer, Kind::Fill);
                        let color = color_on(&layer, color_of(rgb));
                        for mut entity in fill_entities(path, settings.hatches, &world2, &world3, z)
                        {
                            let common = entity.common_mut();
                            (common.layer, common.color, common.transparency) =
                                (layer.clone(), color, fill_transparency);
                            result.entities.push(entity);
                        }
                    }
                    _ => {}
                }
            }
        }

        if settings.text {
            for text in &page.texts {
                let cap = text.cap_height.max(0.0);
                let probe = [
                    text.origin[0] + text.width / 2.0,
                    text.origin[1] + cap / 2.0,
                ];
                if !in_area(probe) && !in_area(text.origin) {
                    continue;
                }
                let style = format!("PDF {}", text.font);
                if !result.text_styles.iter().any(|(name, _)| *name == style) {
                    result
                        .text_styles
                        .push((style.clone(), font_file_for(&text.font)));
                }
                let (dx, dy) = (-text.rotation.sin(), text.rotation.cos());
                let mid = [
                    text.origin[0] + dx * cap / 2.0,
                    text.origin[1] + dy * cap / 2.0,
                ];
                let mut mtext = MText::new();
                mtext.value = text.text.trim_end().to_string();
                mtext.insertion_point = world3(mid);
                mtext.height = cap * scale;
                mtext.rectangle_width = 0.0;
                mtext.rotation = text.rotation + underlay.rotation;
                mtext.style = style;
                mtext.attachment_point = AttachmentPoint::MiddleLeft;
                let layer = naming.layer(pdf_layer, Kind::Text);
                mtext.common.color = color_on(&layer, color_of(text.color));
                mtext.common.layer = layer;
                result.entities.push(EntityType::MText(mtext));
            }
        }

        if settings.raster {
            let (c, s) = (underlay.rotation.cos(), underlay.rotation.sin());
            let turn = |d: [f64; 2]| {
                Vector3::new(
                    (d[0] * c - d[1] * s) * underlay.x_scale,
                    (d[0] * s + d[1] * c) * underlay.y_scale,
                    0.0,
                )
            };
            for image in &page.images {
                let mid = [
                    image.origin[0]
                        + (image.u[0] * image.width as f64 + image.v[0] * image.height as f64)
                            / 2.0,
                    image.origin[1]
                        + (image.u[1] * image.width as f64 + image.v[1] * image.height as f64)
                            / 2.0,
                ];
                if !in_area(mid) {
                    continue;
                }
                result.images.push(ImageImport {
                    rgba: image.rgba.clone(),
                    width: image.width,
                    height: image.height,
                    insertion: world3(image.origin),
                    u: turn(image.u),
                    v: turn(image.v),
                    layer: naming.layer(pdf_layer, Kind::Image),
                });
            }
        }
    }

    // The joined strokes, then (when asked) dashes read as one dashed line.
    let strokes = join_strokes(strokes, settings.join);
    // A mirrored placement turns arcs the other way.
    let turn = (underlay.x_scale * underlay.y_scale).signum();
    // Joined strokes that trace circles, arcs and ellipses become those.
    let strokes: Vec<Stroke> = strokes
        .into_iter()
        .filter_map(|stroke| {
            let points: Vec<[f64; 2]> = stroke
                .points
                .iter()
                .map(|p| {
                    let w = world2(*p);
                    [w.x, w.y]
                })
                .collect();
            let bulges: Vec<f64> = stroke.bulges.iter().map(|b| b * turn).collect();
            match curve_of(&points, &bulges, stroke.closed, z) {
                Some(mut curve) => {
                    let common = curve.common_mut();
                    (common.layer, common.color, common.line_weight) =
                        (stroke.layer, stroke.color, stroke.weight);
                    result.entities.push(curve);
                    None
                }
                None => Some(stroke),
            }
        })
        .collect();
    let strokes: Vec<(Stroke, Option<f64>)> = if settings.linetypes {
        infer_dashes(strokes)
    } else {
        strokes.into_iter().map(|s| (s, None)).collect()
    };
    for (stroke, dash) in strokes {
        // A lone arc is an ARC.
        if !stroke.closed && stroke.points.len() == 2 && stroke.bulges[0] != 0.0 {
            let (a, b) = (world2(stroke.points[0]), world2(stroke.points[1]));
            let bulge = stroke.bulges[0] * turn;
            if let Some(ba) =
                crate::entities::common::BulgeArc::from_bulge([a.x, a.y], [b.x, b.y], bulge)
            {
                let (start, end) = if bulge > 0.0 {
                    (ba.start_angle, ba.end_angle)
                } else {
                    (ba.end_angle, ba.start_angle)
                };
                let tau = std::f64::consts::TAU;
                let mut arc = Arc::from_center_radius_angles(
                    Vector3::new(ba.center[0], ba.center[1], z),
                    ba.radius,
                    start.rem_euclid(tau),
                    end.rem_euclid(tau),
                );
                (arc.common.layer, arc.common.color, arc.common.line_weight) =
                    (stroke.layer, stroke.color, stroke.weight);
                result.entities.push(EntityType::Arc(arc));
                continue;
            }
        }
        let mut pl = LwPolyline::from_points(stroke.points.iter().map(|p| world2(*p)).collect());
        for (vertex, bulge) in pl.vertices.iter_mut().zip(&stroke.bulges) {
            vertex.bulge = bulge * turn;
        }
        pl.is_closed = stroke.closed;
        pl.elevation = z;
        (pl.common.layer, pl.common.color, pl.common.line_weight) =
            (stroke.layer, stroke.color, stroke.weight);
        if let Some(dash) = dash {
            pl.common.linetype = DASH_LINETYPE.to_string();
            pl.common.linetype_scale = dash * scale;
            result.dashed = true;
        }
        result.entities.push(EntityType::LwPolyline(pl));
    }
    result.entities.extend(curves);
    result.layers = layer_colors;
    for image in &result.images {
        if !result.layers.iter().any(|(n, _)| *n == image.layer) {
            result.layers.push((image.layer.clone(), Color::Index(7)));
        }
    }
    result
}

/// One cubic B-spline through a subpath's Bézier control points (lines as
/// degree-elevated cubics).
fn spline_of(sp: &SubPath, world3: &dyn Fn([f64; 2]) -> Vector3) -> Spline {
    let mut spline = Spline::new();
    spline.degree = 3;
    let mut controls: Vec<Vector3> = Vec::new();
    for (i, seg) in sp.segments.iter().enumerate() {
        let (p0, p1, p2, p3) = match seg {
            Segment::Cubic(p0, p1, p2, p3) => (*p0, *p1, *p2, *p3),
            Segment::Line(a, b) => (
                *a,
                [a[0] + (b[0] - a[0]) / 3.0, a[1] + (b[1] - a[1]) / 3.0],
                [
                    a[0] + (b[0] - a[0]) * 2.0 / 3.0,
                    a[1] + (b[1] - a[1]) * 2.0 / 3.0,
                ],
                *b,
            ),
        };
        let pts = if i == 0 {
            vec![p0, p1, p2, p3]
        } else {
            vec![p1, p2, p3]
        };
        controls.extend(pts.into_iter().map(world3));
    }
    let spans = sp.segments.len();
    let mut knots = vec![0.0; 4];
    for k in 1..spans {
        knots.extend([k as f64; 3]);
    }
    knots.extend([spans as f64; 4]);
    spline.control_points = controls;
    spline.weights = Vec::new();
    spline.knots = knots;
    spline.flags.closed = sp.closed;
    spline
}

/// A filled area: a triangle or four-cornered straight loop is a SOLID; any
/// other loop, and every loop but a triangle when fills become hatches, one
/// solid HATCH.
fn fill_entities(
    path: &PdfPath,
    hatches: bool,
    world2: &dyn Fn([f64; 2]) -> Vector2,
    world3: &dyn Fn([f64; 2]) -> Vector3,
    z: f64,
) -> Vec<EntityType> {
    let straight = |sp: &SubPath| sp.segments.iter().all(|s| matches!(s, Segment::Line(..)));
    let corners = |sp: &SubPath| {
        let mut pts: Vec<[f64; 2]> = sp.segments.iter().map(|s| s.start()).collect();
        pts.dedup_by(|a, b| same_point(*a, *b));
        if pts.len() > 1 && same_point(pts[0], *pts.last().unwrap()) {
            pts.pop();
        }
        pts
    };
    if path.subpaths.len() == 1 && straight(&path.subpaths[0]) {
        let pts = corners(&path.subpaths[0]);
        match pts.len() {
            3 => {
                return vec![EntityType::Solid(Solid::triangle(
                    world3(pts[0]),
                    world3(pts[1]),
                    world3(pts[2]),
                ))]
            }
            4 if !hatches => {
                // SOLID corners run 1-2-4-3 (the second edge crosses).
                return vec![EntityType::Solid(Solid::new(
                    world3(pts[0]),
                    world3(pts[1]),
                    world3(pts[3]),
                    world3(pts[2]),
                ))];
            }
            _ => {}
        }
    }
    let mut hatch = Hatch::solid();
    for sp in &path.subpaths {
        let mut ring: Vec<Vector2> = Vec::new();
        for seg in &sp.segments {
            let steps = if matches!(seg, Segment::Cubic(..)) {
                8
            } else {
                1
            };
            for k in 0..steps {
                ring.push(world2(bezier(seg, k as f64 / steps as f64)));
            }
        }
        if ring.len() < 3 {
            continue;
        }
        let mut boundary = BoundaryPath::external();
        boundary.flags.set_polyline(true);
        boundary
            .edges
            .push(BoundaryEdge::Polyline(PolylineEdge::new(ring, true)));
        hatch.add_path(boundary);
    }
    if hatch.paths.is_empty() {
        return Vec::new();
    }
    hatch.elevation = z;
    vec![EntityType::Hatch(hatch)]
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["PDFIMPORT", "-PDFIMPORT"]
});
