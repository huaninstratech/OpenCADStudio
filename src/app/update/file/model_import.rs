//! The fork's mesh-model imports (IFC, STEP), the IFC property report and the
//! IFC round-trip export, split out of the original `update/file.rs`.

use super::super::util::*;
use super::*;

impl OpenCADStudio {
    /// Build the entity + resident mesh set for an imported mesh model.
    ///
    /// The entity is a real `PolyfaceMesh` carrying the triangle shell, so
    /// MOVE / ROTATE / SCALE / UNDO and DWG/DXF saves run through the normal
    /// CAD paths (an empty Solid3D would leave the mesh untransformable).
    /// DXF polyface vertex indices are i16, so shells beyond ~32k vertices
    /// stay display-only Solid3Ds.
    ///
    /// IFC properties arrive as XDATA under the "IFC" application — the
    /// properties panel renders them in a dedicated section and they persist
    /// with the drawing.
    pub(in crate::app) fn build_mesh_import(
        mesh: crate::scene::model::mesh_model::MeshModel,
        guid: &str,
        props: &[(String, String, String)],
    ) -> (document::EntityType, crate::scene::MeshLodSet) {
        let [r, g, b, _a] = mesh.color;
        let mut set = crate::scene::MeshLodSet::from_single(mesh.clone());
        // Feature edges make the solid pickable (normal selection tests
        // B-rep-style feature edges) and give wireframe views real edges.
        let (high, low) = crate::io::meshutil::feature_edges(&set.lods[0].verts);
        set.edge_verts = high;
        set.edge_verts_low = low;

        // XDATA: one record per property (set, name, value) plus GlobalId.
        let mut extended_data = document::xdata::ExtendedData::new();
        if !guid.is_empty() {
            let mut record = document::xdata::ExtendedDataRecord::new("IFC");
            record.add_value(document::xdata::XDataValue::String("GlobalId".into()));
            record.add_value(document::xdata::XDataValue::String(guid.to_string()));
            extended_data.add_record(record);
        }
        if !mesh.name.is_empty() {
            let mut record = document::xdata::ExtendedDataRecord::new("IFC");
            record.add_value(document::xdata::XDataValue::String("Element".into()));
            record.add_value(document::xdata::XDataValue::String("Name".into()));
            record.add_value(document::xdata::XDataValue::String(mesh.name.clone()));
            extended_data.add_record(record);
        }
        for (set_name, prop_name, value) in props {
            let mut record = document::xdata::ExtendedDataRecord::new("IFC");
            record.add_value(document::xdata::XDataValue::String(set_name.clone()));
            record.add_value(document::xdata::XDataValue::String(prop_name.clone()));
            record.add_value(document::xdata::XDataValue::String(value.clone()));
            extended_data.add_record(record);
        }

        const POLYFACE_VERTEX_LIMIT: usize = 32_000;
        let entity = if mesh.verts.len() <= POLYFACE_VERTEX_LIMIT && !mesh.indices.is_empty() {
            let mut pm = document::entities::PolyfaceMesh::new();
            pm.common.color = document::types::Color::from_rgb(
                (r * 255.0).round().clamp(0.0, 255.0) as u8,
                (g * 255.0).round().clamp(0.0, 255.0) as u8,
                (b * 255.0).round().clamp(0.0, 255.0) as u8,
            );
            pm.common.extended_data = extended_data;
            for vertex in &mesh.verts {
                pm.add_vertex_xyz(vertex[0] as f64, vertex[1] as f64, vertex[2] as f64);
            }
            for tri in mesh.indices.chunks_exact(3) {
                // Polyface indices are 1-based.
                pm.add_triangle(tri[0] as i16 + 1, tri[1] as i16 + 1, tri[2] as i16 + 1);
            }
            document::EntityType::PolyfaceMesh(pm)
        } else {
            let mut entity = crate::modules::insert::solid3d_cmds::empty_solid3d();
            entity.common_mut().color = document::types::Color::from_rgb(
                (r * 255.0).round().clamp(0.0, 255.0) as u8,
                (g * 255.0).round().clamp(0.0, 255.0) as u8,
                (b * 255.0).round().clamp(0.0, 255.0) as u8,
            );
            entity.common_mut().extended_data = extended_data;
            entity
        };
        (entity, set)
    }

    /// Bounding-box centre of a flat vertex list, for edit anchoring.
    fn ifc_mesh_center(verts: &[[f32; 3]]) -> Option<[f64; 3]> {
        if verts.is_empty() {
            return None;
        }
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for p in verts {
            for k in 0..3 {
                let v = p[k] as f64;
                min[k] = min[k].min(v);
                max[k] = max[k].max(v);
            }
        }
        Some([
            (min[0] + max[0]) / 2.0,
            (min[1] + max[1]) / 2.0,
            (min[2] + max[2]) / 2.0,
        ])
    }

    /// Round-trip export: collect the imported/edited IFC elements (semantic
    /// records + current meshes + XDATA properties) and write IFC4 text.
    pub(in crate::app) fn on_ifc_export(&mut self) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].scene.ifc_elements.is_empty() {
            self.command_line
                .push_error("EXPORTIFC: no IFC model imported in this drawing (IMPORTIFC first).");
            return Task::none();
        }
        Task::perform(
            async {
                crate::sys::file_dialog()
                    .set_title("Export IFC Model")
                    .add_filter("IFC Files", &["ifc", "IFC"])
                    .set_file_name("model-export.ifc")
                    .save_file()
                    .await
                    .map(|h| crate::sys::handle_path(&h))
            },
            Message::IfcExportPath,
        )
    }

    pub(in crate::app) fn on_ifc_export_path_some(&mut self, path: std::path::PathBuf) -> Task<Message> {
        let worker_path = path.clone();
        let i = self.active_tab;
        let mut elements = Vec::new();
        for (handle, record) in &self.tabs[i].scene.ifc_elements {
            let Some(set) = self.tabs[i].scene.meshes.get(handle) else {
                continue;
            };
            let Some(mesh) = set.lods.first() else {
                continue;
            };
            let props: Vec<(String, String, String)> = self.tabs[i]
                .scene
                .document
                .get_entity(*handle)
                .map(|entity| {
                    entity
                        .common()
                        .extended_data
                        .records()
                        .iter()
                        .filter(|rec| rec.application_name == "IFC")
                        .filter_map(|rec| {
                            let text = |idx: usize| -> Option<String> {
                                rec.values.get(idx).and_then(|value| match value {
                                    document::xdata::XDataValue::String(text) => Some(text.clone()),
                                    _ => None,
                                })
                            };
                            match rec.values.len() {
                                3 => match (text(0), text(1), text(2)) {
                                    (Some(set), Some(name), Some(value)) => {
                                        Some((set, name, value))
                                    }
                                    _ => None,
                                },
                                _ => None,
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            let verts: Vec<[f64; 3]> = mesh
                .verts
                .iter()
                .map(|v| [v[0] as f64, v[1] as f64, v[2] as f64])
                .collect();
            let storey = record.storey.rsplit(" / ").next().unwrap_or("").to_string();
            elements.push(crate::io::ifc_write::IfcExportElement {
                guid: record.guid.clone(),
                ifc_class: record.class.clone(),
                name: record.name.clone(),
                storey,
                verts,
                props,
            });
        }
        let model = crate::io::ifc_write::IfcExportModel { elements };
        background_task(
            crate::t!("IFC export"),
            move || {
                let text = crate::io::ifc_write::write_ifc(&model)?;
                std::fs::write(&worker_path, text).map_err(|e| e.to_string())
            },
            move |result| Message::IfcExportFinished(path, result),
        )
    }

    /// Show the shared open-progress overlay for a model import.
    fn begin_model_import_progress(&mut self, path: &std::path::Path, size_bytes: u64) {
        let progress = std::sync::Arc::new(crate::io::OpenProgressState::new(
            crate::app::OPEN_PHASE_READING,
        ));
        self.opening = Some(crate::app::OpenProgress {
            id: self.next_open_id(),
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "model".into()),
            source_path: Some(path.to_path_buf()),
            size_bytes,
            state: progress,
            started: Instant::now(),
            recovery_error: None,
            recovery_read_stats: None,
            #[cfg(target_arch = "wasm32")]
            recovery_bytes: None,
            #[cfg(not(target_arch = "wasm32"))]
            fingerprint: None,
        });
    }

    /// Parametric edit of the selected IFC element: apply `key = value` to
    /// its captured extrusion parameters, regenerate the mesh in place and
    /// rewrite the resident PolyfaceMesh.
    pub(in crate::app) fn on_ifc_edit(&mut self, key: String, value: f64) -> Task<Message> {
        let i = self.active_tab;
        let Some(handle) = self.tabs[i]
            .scene
            .selected_handles_in_order()
            .last()
            .copied()
        else {
            self.command_line
                .push_error("IFCEDIT: select an IFC element first.");
            return Task::none();
        };
        let listing = {
            let scene = &self.tabs[i].scene;
            scene.ifc_elements.get(&handle).and_then(|record| {
                record.params.as_ref().map(|params| {
                    params
                        .editable_rows()
                        .iter()
                        .map(|(k, label, v)| format!("{k} = {label} ({v:.1} mm)"))
                        .collect::<Vec<_>>()
                        .join("; ")
                })
            })
        };
        let Some(record) = self.tabs[i].scene.ifc_elements.get_mut(&handle) else {
            self.command_line
                .push_error("IFCEDIT: the selected object has no editable IFC parameters.");
            return Task::none();
        };
        if key == "?" {
            let text = listing.unwrap_or_else(|| "none".into());
            self.command_line
                .push_info(&format!("IFCEDIT: editable parameters — {text}"));
            return Task::none();
        }
        let Some(params) = record.params.as_mut() else {
            self.command_line
                .push_error("IFCEDIT: the selected object has no editable IFC parameters.");
            return Task::none();
        };
        if !params.set(&key, value) {
            self.command_line
                .push_error(&format!("IFCEDIT: cannot set {key} = {value}."));
            return Task::none();
        }
        let name = record.name.clone();
        let params = record.params.clone();
        // Keep the element where it stands: anchor the rebuilt mesh to the
        // current bounding-box centre and slide the stored placement along.
        let old_center = self.tabs[i]
            .scene
            .meshes
            .get(&handle)
            .and_then(|s| s.lods.first())
            .and_then(|m| Self::ifc_mesh_center(&m.verts));
        let color = self.tabs[i]
            .scene
            .meshes
            .get(&handle)
            .and_then(|s| s.lods.first())
            .map(|m| m.color)
            .unwrap_or([0.72, 0.72, 0.78, 1.0]);
        let Some(mut new_mesh) = crate::io::ifc::rebuild_extrusion(
            params.as_ref().expect("params present"),
            &name,
            color,
            1.0,
        ) else {
            self.command_line
                .push_error("IFCEDIT: this element's shape cannot be regenerated.");
            return Task::none();
        };
        if let (Some(old), Some(new)) = (old_center, Self::ifc_mesh_center(&new_mesh.verts)) {
            let d = [old[0] - new[0], old[1] - new[1], old[2] - new[2]];
            for vertex in new_mesh.verts.iter_mut() {
                vertex[0] += d[0] as f32;
                vertex[1] += d[1] as f32;
                vertex[2] += d[2] as f32;
            }
            if let Some(record) = self.tabs[i].scene.ifc_elements.get_mut(&handle) {
                if let Some(params) = record.params.as_mut() {
                    params.world.w_axis.x += d[0];
                    params.world.w_axis.y += d[1];
                    params.world.w_axis.z += d[2];
                }
            }
        }
        if let Some(document::EntityType::PolyfaceMesh(pm)) =
            self.tabs[i].scene.document.get_entity_mut(handle)
        {
            pm.vertices.clear();
            pm.faces.clear();
            for vertex in &new_mesh.verts {
                pm.add_vertex_xyz(vertex[0] as f64, vertex[1] as f64, vertex[2] as f64);
            }
            for tri in new_mesh.indices.chunks_exact(3) {
                pm.add_triangle(tri[0] as i16 + 1, tri[1] as i16 + 1, tri[2] as i16 + 1);
            }
        }
        let mut set = crate::scene::MeshLodSet::from_single(new_mesh);
        set.lods[0].name = handle.value().to_string();
        let (high, low) = crate::io::meshutil::feature_edges(&set.lods[0].verts);
        set.edge_verts = high;
        set.edge_verts_low = low;
        self.tabs[i].scene.meshes.insert(handle, set);
        self.tabs[i].dirty = true;
        self.refresh_properties();
        self.command_line.push_output(&format!(
            "IFCEDIT: {key} set to {value:.1} mm; element rebuilt."
        ));
        Task::none()
    }

    pub(in crate::app) fn on_ifc_import_path_some(&mut self, path: std::path::PathBuf) -> Task<Message> {
        let tab_id = self.tabs[self.active_tab].id;
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        self.begin_model_import_progress(&path, size);
        let progress = self
            .opening
            .as_ref()
            .map(|p| p.state.clone())
            .unwrap_or_else(|| {
                std::sync::Arc::new(crate::io::OpenProgressState::new(
                    crate::app::OPEN_PHASE_READING,
                ))
            });
        let worker_path = path.clone();
        background_task(
            crate::t!("IFC import"),
            move || {
                let bytes = std::fs::read(&worker_path).map_err(|e| e.to_string())?;
                crate::io::ifc::IfcImportResult::parse_ifc_with_progress(&bytes, Some(&progress))
                    .map(Box::new)
            },
            move |result| Message::IfcImportFinished(tab_id, path, result),
        )
    }

    pub(in crate::app) fn on_ifc_import_finished(
        &mut self,
        tab_id: u64,
        path: std::path::PathBuf,
        result: Result<Box<crate::io::ifc::IfcImportResult>, String>,
    ) -> Task<Message> {
        self.opening = None;
        match result {
            Err(error) => self.command_line.push_error(&format!("IMPORTIFC: {error}")),
            Ok(import) => {
                let Some(i) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
                    self.command_line
                        .push_info("IMPORTIFC: target drawing was closed.");
                    return Task::none();
                };
                // Snapshotting a six-figure entity import would freeze the UI
                // thread; skip undo for very large imports.
                let undo_worthwhile = import.meshes.len() <= 20_000;
                if undo_worthwhile {
                    self.push_undo_snapshot(i, "IMPORTIFC");
                }
                let mut added = 0usize;
                let records = import.records;
                let tree = import.tree;
                // Snapshotting a six-figure entity import would freeze the UI
                // thread; skip undo for very large imports.
                let undo_worthwhile = import.meshes.len() <= 20_000;
                let mut guid_handles: Vec<(String, document::Handle)> = Vec::new();
                // GUID → property rows, attached to each entity as XDATA.
                let props_by_guid: std::collections::HashMap<&str, &[(String, String, String)]> =
                    import
                        .elements
                        .iter()
                        .map(|e| (e.guid.as_str(), e.props.as_slice()))
                        .collect();
                for out in import.meshes {
                    let props = props_by_guid.get(out.guid.as_str()).copied().unwrap_or(&[]);
                    let (entity, mut set) = Self::build_mesh_import(out.mesh, &out.guid, props);
                    set.edge_verts = out.edges.0;
                    set.edge_verts_low = out.edges.1;
                    let handle = self.tabs[i].scene.add_entity(entity);
                    if !handle.is_null() {
                        // The pick pipeline resolves the owning entity by
                        // parsing the resident mesh's name as a handle —
                        // element names live in the IFC XDATA section instead.
                        set.lods[0].name = handle.value().to_string();
                        self.tabs[i].scene.meshes.insert(handle, set);
                        guid_handles.push((out.guid, handle));
                        added += 1;
                    }
                }
                // Semantic layer (mức 3): per-element records + model tree.
                {
                    let scene = &mut self.tabs[i].scene;
                    scene.ifc_elements.clear();
                    scene.ifc_handle_by_guid.clear();
                    for (record, (guid, handle)) in records.iter().zip(guid_handles.iter()) {
                        scene.ifc_elements.insert(*handle, record.clone());
                        scene.ifc_handle_by_guid.insert(guid.clone(), *handle);
                    }
                    scene.ifc_tree = tree;
                }
                self.tabs[i].dirty = true;
                // Mesh imports land wherever the file's coordinates are —
                // bring them into view immediately.
                self.tabs[i].scene.remember_current_view();
                self.tabs[i].scene.fit_all();
                // Surface the Properties panel so the IFC Properties section
                // (XDATA) is visible right after the import.
                self.show_properties = true;
                self.ribbon.set_properties(true);
                self.refresh_properties();
                self.command_line.push_output(&format!(
                    "IMPORTIFC: imported {added} mesh(es) from {} (schema {}; {} elements)",
                    path.display(),
                    import.schema,
                    import.elements.len(),
                ));
                for warning in import.warnings.iter().take(3) {
                    self.command_line
                        .push_info(&format!("IMPORTIFC: note: {warning}"));
                }
                if !import.elements.is_empty() {
                    self.command_line.push_info(
                        "IFCDATA: run this command to export the property report as CSV.",
                    );
                }
            }
        }
        self.drain_pending_open()
    }

    pub(in crate::app) fn on_step_import_path_some(&mut self, path: std::path::PathBuf) -> Task<Message> {
        let tab_id = self.tabs[self.active_tab].id;
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        self.begin_model_import_progress(&path, size);
        let progress = self
            .opening
            .as_ref()
            .map(|p| p.state.clone())
            .unwrap_or_else(|| {
                std::sync::Arc::new(crate::io::OpenProgressState::new(
                    crate::app::OPEN_PHASE_READING,
                ))
            });
        let worker_path = path.clone();
        background_task(
            crate::t!("STEP import"),
            move || {
                let bytes = std::fs::read(&worker_path).map_err(|e| e.to_string())?;
                crate::io::step_read::StepImportResult::parse_step_with_progress(
                    &bytes,
                    Some(&progress),
                )
                .map(Box::new)
            },
            move |result| Message::StepImportFinished(tab_id, path, result),
        )
    }

    pub(in crate::app) fn on_step_import_finished(
        &mut self,
        tab_id: u64,
        path: std::path::PathBuf,
        result: Result<Box<crate::io::step_read::StepImportResult>, String>,
    ) -> Task<Message> {
        self.opening = None;
        match result {
            Err(error) => self
                .command_line
                .push_error(&format!("IMPORTSTEP: {error}")),
            Ok(import) => {
                let Some(i) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
                    self.command_line
                        .push_info("IMPORTSTEP: target drawing was closed.");
                    return Task::none();
                };
                self.push_undo_snapshot(i, "IMPORTSTEP");
                let mut added = 0usize;
                for mesh in import.meshes {
                    let (entity, mut set) = Self::build_mesh_import(mesh, "", &[]);
                    let handle = self.tabs[i].scene.add_entity(entity);
                    if !handle.is_null() {
                        set.lods[0].name = handle.value().to_string();
                        self.tabs[i].scene.meshes.insert(handle, set);
                        added += 1;
                    }
                }
                self.tabs[i].dirty = true;
                // Mesh imports land wherever the file's coordinates are —
                // bring them into view immediately.
                self.tabs[i].scene.remember_current_view();
                self.tabs[i].scene.fit_all();
                self.command_line.push_output(&format!(
                    "IMPORTSTEP: imported {added} solid(s) from {} (schema {}; {} products)",
                    path.display(),
                    import.schema,
                    import.products,
                ));
                for warning in import.warnings.iter().take(3) {
                    self.command_line
                        .push_info(&format!("IMPORTSTEP: note: {warning}"));
                }
            }
        }
        self.drain_pending_open()
    }

    pub(in crate::app) fn on_ifc_data_source_some(&mut self, path: std::path::PathBuf) -> Task<Message> {
        let worker_path = path.clone();
        background_task(
            crate::t!("IFC data export"),
            move || {
                let bytes = std::fs::read(&worker_path).map_err(|e| e.to_string())?;
                crate::io::ifc::properties_csv_string(&bytes)
            },
            Message::IfcDataExportBuilt,
        )
    }

    pub(in crate::app) fn on_ifc_data_built(&mut self, result: Result<String, String>) -> Task<Message> {
        match result {
            Err(error) => {
                self.command_line.push_error(&format!("IFCDATA: {error}"));
                Task::none()
            }
            Ok(csv) => {
                let rows = csv.lines().count().saturating_sub(1);
                self.command_line.push_output(&format!(
                    "IFCDATA: extracted {rows} property rows; choose where to save the CSV."
                ));
                Task::perform(
                    async {
                        crate::sys::file_dialog()
                            .set_title("Save IFC data report")
                            .add_filter("CSV Files", &["csv", "CSV"])
                            .set_file_name("ifc-data.csv")
                            .save_file()
                            .await
                            .map(|h| crate::sys::handle_path(&h))
                    },
                    move |picked| Message::IfcDataExportSaveResult(csv, picked),
                )
            }
        }
    }

    pub(in crate::app) fn on_ifc_data_save(
        &mut self,
        csv: String,
        picked: Option<std::path::PathBuf>,
    ) -> Task<Message> {
        let Some(path) = picked else {
            return Task::none();
        };
        let worker_path = path.clone();
        background_task(
            crate::t!("IFC data save"),
            move || std::fs::write(&worker_path, csv.as_bytes()).map_err(|e| e.to_string()),
            move |result| Message::IfcDataWriteFinished(path, result),
        )
    }
}
