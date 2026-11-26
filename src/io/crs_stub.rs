//! The `mobile` build's stand-in for [`super::crs`].
//!
//! A phone build carries no EPSG registry and no proj4, which together are
//! several megabytes of wasm the phone never needs: a drawing opened on a
//! phone is edited in its own plane, not projected onto the globe. Every
//! function keeps the signature of its real counterpart so the rest of the
//! tree compiles unchanged; the ones that must produce a projection say so
//! instead of pretending to work.
//!
//! Reading and writing a drawing is untouched: GEODATA objects still travel
//! through the document, so a file saved on the phone keeps the coordinate
//! system it arrived with — this module only refuses to *interpret* one.

use document::objects::{GeoData, ObjectType};
use document::{Document, Handle};

pub const GEODATA_DICTIONARY_KEY: &str = "ACAD_GEOGRAPHICDATA";

/// The one line a user sees when they ask for something that needs a CRS.
const UNAVAILABLE: &str =
    "Coordinate systems are not available in this build. Open the drawing in the desktop app \
     to work with georeferenced data.";

/// A coordinate-system definition the mobile build cannot build. It keeps the
/// text a caller supplied so a round trip through `apply_to_geodata` still
/// writes back what it read, but there is no projection behind it.
#[derive(Clone, Debug)]
pub struct CrsDefinition {
    code: Option<u16>,
    name: String,
    proj4: String,
    wkt: Option<String>,
}

impl CrsDefinition {
    pub fn from_epsg(code: u16) -> Result<Self, String> {
        Err(format!("EPSG:{code}: {UNAVAILABLE}"))
    }

    pub fn from_wkt(_wkt: &str) -> Result<Self, String> {
        Err(UNAVAILABLE.to_string())
    }

    pub(crate) fn from_wkt_parameters(_wkt: &str) -> Result<Self, String> {
        Err(UNAVAILABLE.to_string())
    }

    pub fn from_text(_input: &str) -> Result<Self, String> {
        Err(UNAVAILABLE.to_string())
    }

    /// GEODATA written by a desktop build is read for its text only. A drawing
    /// that carries one still opens; converting coordinates is what fails.
    pub fn from_geodata(geo: &GeoData) -> Result<Self, String> {
        let wkt = geo.coordinate_system_wkt.trim();
        if wkt.is_empty() && geo.coordinate_system_definition.trim().is_empty() {
            return Err("The drawing GEODATA has no coordinate-system definition".to_string());
        }
        Ok(Self {
            code: None,
            name: if wkt.is_empty() {
                "GEODATA".to_string()
            } else {
                wkt.to_string()
            },
            proj4: String::new(),
            wkt: (!wkt.is_empty()).then(|| wkt.to_string()),
        })
    }

    /// Writes back the text this definition was read from, so saving a
    /// drawing on a phone does not strip the coordinate system from it.
    pub fn apply_to_geodata(&self, geo: &mut GeoData) -> Result<(), String> {
        let Some(wkt) = self.wkt.as_ref() else {
            return Err(UNAVAILABLE.to_string());
        };
        geo.coordinate_system_wkt = wkt.clone();
        Ok(())
    }

    pub fn apply_to_geodata_for_new(
        &self,
        geo: &mut GeoData,
        insertion_units: i16,
    ) -> Result<(), String> {
        self.apply_to_geodata(geo)?;
        let millimetres = crate::entities::units::insunits_to_mm(insertion_units);
        let valid = millimetres.is_some() && (1..=24).contains(&insertion_units);
        geo.horizontal_unit_scale = millimetres.map(|mm| mm / 1_000.0).unwrap_or(1.0);
        geo.vertical_unit_scale = geo.horizontal_unit_scale;
        geo.horizontal_units = if valid { i32::from(insertion_units) } else { 0 };
        geo.vertical_units = geo.horizontal_units;
        geo.north_direction = document::types::Vector2::new(0.0, 1.0);
        geo.up_direction = document::types::Vector3::UNIT_Z;
        geo.scale_estimation_method = 1;
        geo.user_scale_factor = 1.0;
        Ok(())
    }

    pub fn code(&self) -> Option<u16> {
        self.code
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn proj4(&self) -> &str {
        &self.proj4
    }

    pub fn wkt(&self) -> Option<&str> {
        self.wkt.as_deref()
    }

    pub fn is_geographic(&self) -> bool {
        false
    }

    pub fn equivalent_to(&self, other: &Self) -> bool {
        self.wkt == other.wkt
    }

    pub fn base_geographic_definition(&self) -> Result<Self, String> {
        Err(UNAVAILABLE.to_string())
    }

    pub fn is_normalized_axis(&self) -> bool {
        false
    }

    pub fn axis(&self) -> [u8; 3] {
        *b"enu"
    }

    pub fn units(&self) -> &str {
        "m"
    }

    pub fn to_meter(&self) -> f64 {
        1.0
    }

    pub fn to_base_geographic(&self, _x: f64, _y: f64) -> Result<(f64, f64), String> {
        Err(UNAVAILABLE.to_string())
    }

    pub fn from_base_geographic(&self, _lon: f64, _lat: f64) -> Result<(f64, f64), String> {
        Err(UNAVAILABLE.to_string())
    }
}

/// The GEODATA holder of a drawing, which is plain document data and still
/// readable here — only interpreting it needs the registry.
pub fn model_space_geo_data_handle(document: &Document) -> Option<Handle> {
    let model_space = document.block_records.get("*Model_Space")?.handle;
    let dictionary = document.extension_dictionary_handle(model_space)?;
    let ObjectType::Dictionary(dictionary) = document.objects.get(&dictionary)? else {
        return None;
    };
    dictionary
        .entries
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(GEODATA_DICTIONARY_KEY))
        .map(|(_, handle)| *handle)
        .find(|handle| matches!(document.objects.get(handle), Some(ObjectType::GeoData(_))))
}

pub fn model_space_geo_data(document: &Document) -> Option<&GeoData> {
    match document.objects.get(&model_space_geo_data_handle(document)?)? {
        ObjectType::GeoData(geo) => Some(geo),
        _ => None,
    }
}

pub fn document_crs(document: &Document) -> Result<Option<CrsDefinition>, String> {
    model_space_geo_data(document)
        .map(CrsDefinition::from_geodata)
        .transpose()
}

pub fn set_document_crs(document: &mut Document, crs: &CrsDefinition) -> Result<Handle, String> {
    let model_space = document
        .block_records
        .get("*Model_Space")
        .map(|record| record.handle)
        .ok_or_else(|| "The drawing has no model-space block record".to_string())?;
    if let Some(handle) = model_space_geo_data_handle(document) {
        let Some(ObjectType::GeoData(geo)) = document.objects.get_mut(&handle) else {
            return Err("The model-space GEODATA object is missing".to_string());
        };
        crs.apply_to_geodata(geo)?;
        return Ok(handle);
    }
    let insertion_units = document.header.insertion_units;
    let mut geo = GeoData::default();
    crs.apply_to_geodata_for_new(&mut geo, insertion_units)?;
    let dictionary = document.ensure_extension_dictionary(model_space);
    let handle = document.allocate_handle();
    geo.handle = handle;
    geo.owner = dictionary;
    geo.reactors = vec![dictionary];
    geo.version = 3;
    geo.host_block = model_space;
    document.objects.insert(handle, ObjectType::GeoData(geo));
    if let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&dictionary) {
        dictionary.insert(GEODATA_DICTIONARY_KEY, handle);
    }
    Ok(handle)
}
