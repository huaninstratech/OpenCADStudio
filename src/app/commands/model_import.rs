//! ACISIN and WMFIN, and the files IMPORT routes here: ACIS text (SAT),
//! STEP and IGES models and Windows metafiles, read into the current drawing
//! as one undo step.

use super::*;
use std::collections::HashMap;

use crate::io::model_import::{self, ModelFormat};

impl OpenCADStudio {
    /// ACISIN and WMFIN; `None` defers to the other families.
    pub(super) fn dispatch_model_import(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let (verb, rest) = match cmd.trim().split_once(char::is_whitespace) {
            Some((verb, rest)) => (verb.to_ascii_uppercase(), rest.trim().trim_matches('"')),
            None => (cmd.trim().to_ascii_uppercase(), ""),
        };
        match (verb.as_str(), rest.is_empty()) {
            ("ACISIN", true) if crate::sys::file_dia() => {
                let filters = vec![("ACIS (*.sat)".to_string(), vec!["sat".to_string()])];
                let title = crate::t!("Select ACIS File").into_owned();
                Some(Task::perform(super::dgn_import::pick(title, filters), |path| match path {
                    Some(path) => Message::Command(format!("_ACISINFILE {path}")),
                    None => Message::Noop,
                }))
            }
            ("ACISIN", true) => {
                use crate::command::FreeTextValuePromptCommand;
                let command = FreeTextValuePromptCommand::new("_ACISINFILE", "Enter SAT file name:");
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                Some(Task::none())
            }
            ("ACISIN" | "_ACISINFILE", false) => {
                if crate::scene::model::pdf_raster::source_bytes(rest).is_none() {
                    let name = std::path::Path::new(rest)
                        .file_name()
                        .map_or_else(|| rest.to_string(), |n| n.to_string_lossy().into_owned());
                    let here = std::env::current_dir()
                        .map(|d| d.display().to_string())
                        .unwrap_or_default();
                    self.command_line
                        .push_error(&format!("\"{name}\": file not found in the current directory, {here}"));
                    return Some(Task::none());
                }
                Some(self.import_model(i, rest, ModelFormat::Sat))
            }
            ("_ACISINFILE", true) => Some(Task::none()),
            ("WMFIN", true) if crate::sys::file_dia() => {
                let filters = vec![("Metafile (*.wmf)".to_string(), vec!["wmf".to_string()])];
                let title = crate::t!("Import WMF").into_owned();
                Some(Task::perform(super::dgn_import::pick(title, filters), |path| match path {
                    Some(path) => Message::Command(format!("_WMFINFILE {path}")),
                    None => Message::Noop,
                }))
            }
            ("WMFIN", true) => {
                use crate::command::FreeTextValuePromptCommand;
                let command = FreeTextValuePromptCommand::new(
                    "_WMFINFILE",
                    "Enter name of WMF file to import (no extension):",
                );
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                Some(Task::none())
            }
            ("WMFIN" | "_WMFINFILE", false) => {
                let path = if std::path::Path::new(rest).extension().is_some() {
                    rest.to_string()
                } else {
                    format!("{rest}.wmf")
                };
                Some(self.import_wmf(i, &path, true))
            }
            ("_WMFINFILE", true) => Some(Task::none()),
            _ => None,
        }
    }

    /// Reads a model file into drawing `i`.
    pub(in crate::app) fn import_model(&mut self, i: usize, path: &str, format: ModelFormat) -> Task<Message> {
        let Some(bytes) = crate::scene::model::pdf_raster::source_bytes(path) else {
            self.command_line
                .push_error(&format!("File {path} does not exist."));
            return Task::none();
        };
        let translated = format != ModelFormat::Sat;
        let drawing_mm = crate::app::properties::insunits_to_mm(
            self.tabs[i].scene.document.header.insertion_units,
        );
        let Some(imported) = model_import::read(format, &bytes, drawing_mm) else {
            self.command_line
                .push_error(&format!("Import failed: {path} has no model the importer reads."));
            return Task::none();
        };
        self.push_undo_snapshot(i, "IMPORT");
        let scene = &mut self.tabs[i].scene;
        // Each block goes under a name the drawing does not use yet (a
        // product named like a block already there gets a new name, not the
        // old definition), and the inserts are pointed at the names the
        // blocks got. An insert of a block that could not be made is left
        // out.
        fn retarget(entities: Vec<codec::EntityType>, defined: &HashMap<String, String>) -> Vec<codec::EntityType> {
            entities
                .into_iter()
                .filter_map(|entity| match entity {
                    codec::EntityType::Insert(mut insert) => {
                        insert.block_name = defined.get(&insert.block_name)?.clone();
                        Some(codec::EntityType::Insert(insert))
                    }
                    other => Some(other),
                })
                .collect()
        }
        let mut taken: Vec<String> = scene.document.block_records.iter().map(|b| b.name.clone()).collect();
        let mut defined: HashMap<String, String> = HashMap::new();
        for (name, entities) in imported.blocks {
            let unique = crate::modules::insert::xattach::unique_block_name(&name, &taken);
            let entities = retarget(entities, &defined);
            match scene.define_block_from_owned_entities(entities, &unique, glam::DVec3::ZERO) {
                Ok(_) => {
                    taken.push(unique.clone());
                    defined.insert(name, unique);
                }
                Err(error) => self.command_line.push_error(&format!("{name}: {error}")),
            }
        }
        for entity in retarget(imported.entities, &defined) {
            scene.add_entity(entity);
        }
        self.tabs[i].dirty = true;
        if translated {
            self.command_line.push_output("Import Job Complete");
        }
        self.refresh_properties();
        Task::none()
    }

    /// A metafile as a new block `WMF<n>`, fitted to half the current view:
    /// placed at the view's top-left corner (IMPORT), or by the insertion
    /// prompts (WMFIN).
    pub(in crate::app) fn import_wmf(&mut self, i: usize, path: &str, prompt: bool) -> Task<Message> {
        let Some(bytes) = crate::scene::model::pdf_raster::source_bytes(path) else {
            self.command_line
                .push_error(&format!("File {path} does not exist."));
            return Task::none();
        };
        let (centre, size, pixel) = self.tabs[i].scene.view_window();
        let Some(mut picture) =
            crate::io::wmf_import::read(&bytes, [0.5 * size[0], 0.5 * size[1]])
        else {
            self.command_line
                .push_error(&format!("{path} is not a valid Windows metafile."));
            return Task::none();
        };
        self.push_undo_snapshot(i, if prompt { "WMFIN" } else { "IMPORT" });
        let scene = &mut self.tabs[i].scene;
        // A text style per face, `WMF-<face><n>` with `n` counting the
        // face's bold and italic variants; the style's font is the
        // installed file of that face, none when it is not installed. A
        // style the drawing already has is used as it is.
        let mut styles: Vec<String> = Vec::new();
        for (k, face) in picture.fonts.iter().enumerate() {
            let n = picture.fonts[..k].iter().filter(|f| f.name == face.name).count();
            // The face name comes from the file.
            let name = crate::scene::sanitized_table_name(&format!("WMF-{}{n}", face.name), "WMF");
            if !scene.document.text_styles.contains(&name) {
                let mut style = codec::tables::TextStyle::new(name.as_str());
                style.handle = scene.document.allocate_handle();
                style.font_file =
                    crate::scene::text::sysfont::face_file(&face.name, face.bold, face.italic).unwrap_or_default();
                let _ = scene.document.text_styles.add(style);
            }
            styles.push(name);
        }
        for (at, font) in &picture.text_fonts {
            if let Some(codec::EntityType::Text(text)) = picture.entities.get_mut(*at) {
                text.style = styles[*font].clone();
            }
        }
        let mut n = 0;
        let name = loop {
            let name = format!("WMF{n}");
            if scene.document.block_records.get(&name).is_none() {
                break name;
            }
            n += 1;
        };
        if let Err(error) =
            scene.define_block_from_owned_entities(picture.entities, &name, glam::DVec3::ZERO)
        {
            self.command_line.push_error(&error);
            return Task::none();
        }
        self.tabs[i].dirty = true;
        if prompt {
            use crate::command::CadCommand;
            use crate::modules::insert::insert_block::InsertBlockCommand;
            let preview = self.tabs[i].scene.block_preview_wires(&name);
            let command = InsertBlockCommand::classic_for_block(name, preview, String::new());
            self.command_line.push_info(&command.prompt());
            self.tabs[i].active_cmd = Some(Box::new(command));
        } else {
            // The view's top-left corner, a pixel or two in.
            let corner = codec::types::Vector3::new(
                centre[0] - 0.5 * size[0] + pixel,
                centre[1] + 0.5 * size[1] - 2.0 * pixel,
                0.0,
            );
            let insert = codec::entities::Insert::new(&name, corner);
            self.tabs[i].scene.add_entity(codec::EntityType::Insert(insert));
            self.refresh_properties();
        }
        Task::none()
    }
}

impl OpenCADStudio {
    /// The 3D Studio Import window for a file; an error when it is no 3D
    /// Studio file.
    pub(in crate::app) fn open_tds_import_dialog(&mut self, path: &str) -> Result<(), String> {
        let scene = crate::scene::model::pdf_raster::source_bytes(path)
            .and_then(|bytes| crate::io::tds_import::read(&bytes))
            .ok_or_else(|| format!("{path} is not a valid 3D Studio file."))?;
        let mut objects: Vec<(String, &'static str, usize)> = scene
            .objects
            .iter()
            .map(|(name, object)| {
                let materials = match object {
                    crate::io::tds_import::TdsObject::Mesh(mesh) => scene.mesh_materials(mesh).len(),
                    _ => 0,
                };
                (name.clone(), object.kind(), materials)
            })
            .collect();
        objects.sort_by_key(|o| o.0.to_lowercase());
        objects.insert(0, ("Global Settings".to_string(), "Global", 0));
        // The layer and material choices made last stay; nothing starts
        // checked.
        let previous = self.import_dialogs.tds.take();
        self.import_dialogs.tds = Some(crate::ui::window::import_dialogs::TdsImportState {
            path: path.to_string(),
            checked: vec![false; objects.len()],
            objects,
            materials: scene.materials.len(),
            filter: 0,
            search: String::new(),
            layers: previous.as_ref().map_or(0, |s| s.layers),
            multi: previous.as_ref().map_or(0, |s| s.multi),
        });
        self.active_modal = Some(crate::app::ModalKind::TdsImport);
        Ok(())
    }

    /// The window's choices as an import: asked mesh by mesh first when set
    /// to ask and some checked mesh has several materials.
    pub(in crate::app) fn start_tds_import(
        &mut self,
        i: usize,
        state: crate::ui::window::import_dialogs::TdsImportState,
    ) -> Task<Message> {
        use crate::io::tds_import::{LayerMode, MultiMaterial, TdsObject};
        use crate::modules::insert::tds_import::{TdsImportRequest, TdsMaterialCommand};
        let Some(scene) = crate::scene::model::pdf_raster::source_bytes(&state.path)
            .and_then(|bytes| crate::io::tds_import::read(&bytes))
        else {
            return Task::none();
        };
        let selected: Vec<String> = state
            .objects
            .iter()
            .zip(&state.checked)
            .filter(|(_, on)| **on)
            .map(|((name, _, _), _)| name.clone())
            .collect();
        let several: Vec<(String, Vec<String>)> = scene
            .objects
            .iter()
            .filter(|(name, _)| selected.contains(name))
            .filter_map(|(name, object)| match object {
                TdsObject::Mesh(mesh) => {
                    let materials = scene.mesh_materials(mesh);
                    (materials.len() > 1).then(|| (name.clone(), materials))
                }
                _ => None,
            })
            .collect();
        let choice = match state.multi {
            1 => Some(MultiMaterial::Split),
            2 => Some(MultiMaterial::First),
            3 => Some(MultiMaterial::Material(None)),
            _ => None,
        };
        let mut request = TdsImportRequest {
            path: state.path.clone(),
            selected,
            layers: match state.layers {
                1 => LayerMode::ByMaterial,
                2 => LayerMode::ByObjectColor,
                3 => LayerMode::Single,
                _ => LayerMode::ByObject,
            },
            multi: Default::default(),
            compact: matches!(state.multi, 0 | 1),
        };
        match choice {
            Some(choice) => {
                for (name, _) in several {
                    request.multi.insert(name, choice.clone());
                }
            }
            None if !several.is_empty() => {
                use crate::command::CadCommand;
                let command = TdsMaterialCommand::new(request, several);
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                return Task::none();
            }
            None => {}
        }
        self.run_tds_import(i, request);
        Task::none()
    }

    /// The import itself, one undo step: the meshes on their layers with
    /// their materials, the cameras as named views.
    pub(in crate::app) fn run_tds_import(
        &mut self,
        i: usize,
        request: crate::modules::insert::tds_import::TdsImportRequest,
    ) {
        use codec::objects::ObjectType;
        let Some(scene) = crate::scene::model::pdf_raster::source_bytes(&request.path)
            .and_then(|bytes| crate::io::tds_import::read(&bytes))
        else {
            self.command_line
                .push_error(&format!("{} is not a valid 3D Studio file.", request.path));
            return;
        };
        let import = crate::io::tds_import::convert(
            &scene,
            &crate::io::tds_import::TdsOptions {
                selected: request.selected.clone(),
                layers: request.layers,
                multi: request.multi.clone(),
                compact: request.compact,
                single_layer: self.tabs[i].scene.document.header.current_layer_name.clone(),
            },
        );
        // Named views take the current view's proportions.
        let (_, size, _) = self.tabs[i].scene.view_window();
        let aspect = if size[0] > 0.0 && size[1] > 0.0 { size[0] / size[1] } else { 1.0 };
        self.push_undo_snapshot(i, "IMPORT");
        let document = &mut self.tabs[i].scene.document;
        // Materials live in the material dictionary; a drawing without one
        // imports the meshes without materials.
        let dictionary = document.header.acad_material_dict_handle;
        let has_dictionary = matches!(document.objects.get(&dictionary), Some(ObjectType::Dictionary(_)));
        // Materials by name: the drawing's own when it has one, else one
        // made from the file's colours.
        let mut materials: std::collections::HashMap<String, codec::Handle> = Default::default();
        for name in &import.materials {
            let existing = document.objects.iter().find_map(|(h, o)| match o {
                ObjectType::Material(m) if m.name.eq_ignore_ascii_case(name) => Some(*h),
                _ => None,
            });
            let handle = match existing {
                Some(h) => h,
                None if !has_dictionary => continue,
                None => {
                    let source = scene.materials.iter().find(|m| m.name == *name);
                    let mut material = codec::objects::Material::new();
                    material.handle = document.allocate_handle();
                    material.owner = dictionary;
                    material.name = name.clone();
                    let color = |rgb: Option<[u8; 3]>| match rgb {
                        Some([r, g, b]) => codec::objects::MaterialColor {
                            flag: 1,
                            factor: 1.0,
                            rgb: Some(crate::scene::convert::acis_import::packed_true_colour(r, g, b)),
                        },
                        None => codec::objects::MaterialColor::default(),
                    };
                    material.ambient_color = color(source.and_then(|m| m.ambient));
                    material.diffuse_color = color(source.and_then(|m| m.diffuse));
                    material.specular_color = color(source.and_then(|m| m.specular));
                    let handle = material.handle;
                    if let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&dictionary) {
                        dictionary.add_entry(name.clone(), handle);
                    }
                    document.objects.insert(handle, ObjectType::Material(material));
                    handle
                }
            };
            materials.insert(name.clone(), handle);
        }
        for part in &import.parts {
            if !document.layers.contains(&part.layer) {
                let mut layer = codec::tables::Layer::new(part.layer.as_str());
                layer.handle = document.allocate_handle();
                let _ = document.layers.add(layer);
            }
        }
        for view in &import.views {
            let mut named = codec::tables::View::new(view.name.as_str());
            named.target = codec::types::Vector3::new(view.target[0], view.target[1], view.target[2]);
            named.direction =
                codec::types::Vector3::new(view.direction[0], view.direction[1], view.direction[2]);
            named.height = view.height;
            named.width = view.height * aspect;
            named.lens_length = view.lens;
            named.perspective = true;
            named.handle = match document.views.get(&view.name) {
                Some(old) => old.handle,
                None => document.allocate_handle(),
            };
            document.views.add_or_replace(named);
        }
        for part in import.parts {
            let mut mesh = part.mesh;
            mesh.common.layer = part.layer.clone();
            mesh.common.material_handle = part.material.as_ref().and_then(|m| materials.get(m).copied());
            if mesh.common.material_handle.is_some() {
                mesh.common.material_flags = 0b11;
            }
            self.tabs[i].scene.add_entity(codec::EntityType::PolyfaceMesh(mesh));
        }
        for line in &import.messages {
            self.command_line.push_output(line);
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
    }
}
