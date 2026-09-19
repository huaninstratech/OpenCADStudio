use super::*;

impl Scene {
    fn model_limits(&self) -> Option<(glam::DVec2, glam::DVec2)> {
        let min = self.document.header.model_space_limits_min;
        let max = self.document.header.model_space_limits_max;
        Self::valid_limits(
            glam::DVec2::new(min.x, min.y),
            glam::DVec2::new(max.x, max.y),
        )
    }

    fn paper_layout_limits(&self) -> Option<(glam::DVec2, glam::DVec2)> {
        self.document.objects.values().find_map(|object| {
            let ObjectType::Layout(layout) = object else {
                return None;
            };
            (layout.name == self.current_layout).then(|| {
                Self::valid_limits(
                    glam::DVec2::new(layout.min_limits.0, layout.min_limits.1),
                    glam::DVec2::new(layout.max_limits.0, layout.max_limits.1),
                )
            })?
        })
    }

    fn valid_limits(min: glam::DVec2, max: glam::DVec2) -> Option<(glam::DVec2, glam::DVec2)> {
        const SANE_LIMIT: f64 = 1.0e16;
        (min.is_finite()
            && max.is_finite()
            && min.x < max.x
            && min.y < max.y
            && min.abs().max_element() < SANE_LIMIT
            && max.abs().max_element() < SANE_LIMIT)
            .then_some((min, max))
    }

    /// The active input space is model space on the Model tab and while editing
    /// through a floating paper-space viewport (MSPACE).
    pub fn input_uses_model_space(&self) -> bool {
        self.current_layout == "Model" || self.active_viewport.is_some()
    }

    /// LIMITS rectangle for the active point-input space.
    pub fn current_drawing_limits(&self) -> Option<(glam::DVec2, glam::DVec2)> {
        if self.input_uses_model_space() {
            self.model_limits()
        } else {
            self.paper_layout_limits().or_else(|| {
                let min = self.document.header.paper_space_limits_min;
                let max = self.document.header.paper_space_limits_max;
                Self::valid_limits(
                    glam::DVec2::new(min.x, min.y),
                    glam::DVec2::new(max.x, max.y),
                )
            })
        }
    }

    /// LIMITS rectangle belonging to a rendered grid viewport. Floating
    /// viewports display model space; the sheet viewport displays paper space.
    pub fn grid_limits_for_viewport(&self, viewport: Handle) -> Option<(glam::DVec2, glam::DVec2)> {
        if self.current_layout == "Model" {
            return self.model_limits();
        }
        let sheet = self.current_layout_sheet_viewport_handle();
        if viewport.is_valid() && viewport != sheet {
            self.model_limits()
        } else {
            self.paper_layout_limits()
        }
    }

    pub fn drawing_limit_check_enabled(&self) -> bool {
        if self.input_uses_model_space() {
            self.document.header.limit_check
        } else {
            self.document.header.paper_space_limit_check
        }
    }

    pub fn point_inside_drawing_limits(&self, point: glam::DVec3) -> bool {
        let Some((min, max)) = self.current_drawing_limits() else {
            return true;
        };
        point.x >= min.x && point.x <= max.x && point.y >= min.y && point.y <= max.y
    }

    pub fn set_drawing_limit_check(&mut self, enabled: bool) {
        if self.input_uses_model_space() {
            self.document.header.limit_check = enabled;
        } else {
            self.document.header.paper_space_limit_check = enabled;
            self.persist_current_layout_state();
        }
    }

    pub fn set_current_drawing_limits(&mut self, min: glam::DVec2, max: glam::DVec2) {
        if self.input_uses_model_space() {
            self.document.header.model_space_limits_min =
                acadrust::types::Vector2::new(min.x, min.y);
            self.document.header.model_space_limits_max =
                acadrust::types::Vector2::new(max.x, max.y);
        } else {
            self.document.header.paper_space_limits_min =
                acadrust::types::Vector2::new(min.x, min.y);
            self.document.header.paper_space_limits_max =
                acadrust::types::Vector2::new(max.x, max.y);
        }

        // Keep the current Layout object synchronized with the header values.
        // DWG stores per-layout limits here as well as the current-space header.
        for object in self.document.objects.values_mut() {
            if let ObjectType::Layout(layout) = object {
                if layout.name == self.current_layout {
                    layout.min_limits = (min.x, min.y);
                    layout.max_limits = (max.x, max.y);
                    break;
                }
            }
        }
        self.paper_viewport_cache
            .borrow_mut()
            .remove(&self.current_layout);
    }

    /// ZOOM All frames the configured drawing limits, extended to include
    /// the drawing extents whenever entities reach beyond them (AutoCAD
    /// parity): a drawing inside the limits still shows the whole limits
    /// rectangle, a drawing that spills out is framed in full. Object-only
    /// framing without the limits remains the responsibility of ZOOM Extents.
    pub fn fit_all_with_limits(&mut self) {
        let Some((limit_min, limit_max)) = self.current_drawing_limits() else {
            self.fit_all();
            return;
        };

        let mut min = glam::Vec3::new(limit_min.x as f32, limit_min.y as f32, 0.0);
        let mut max = glam::Vec3::new(limit_max.x as f32, limit_max.y as f32, 0.0);

        // Model-space content counts toward the frame — drawings exported by
        // nesting/CAM tools routinely sit far outside a template's default
        // LIMITS. Paper space keeps the sheet rectangle: the sheet is the
        // frame there.
        if self.current_layout == "Model" || self.active_viewport.is_some() {
            if let Some((extents_min, extents_max)) = self.model_space_extents() {
                if extents_min.is_finite() && extents_max.is_finite() {
                    min = min.min(extents_min);
                    max = max.max(extents_max);
                }
            }
        }

        // MSPACE owns a camera encoded on the active viewport entity.
        if self.active_viewport.is_some() {
            self.fit_active_viewport_to_bounds(min, max);
            return;
        }

        let aspect = self.active_camera_aspect();
        self.camera.borrow_mut().fit_to_bounds(min, max, aspect);
        self.camera_generation += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drawings exported by nesting/CAM tools sit far outside a template's
    /// default LIMITS: ZOOM All must frame the union of limits and extents
    /// (the U311 nest-file case), while a drawing inside the limits keeps
    /// the limits rectangle as its frame.
    #[test]
    fn zoom_all_frames_entities_that_spill_past_the_limits() {
        let frame = |entity: EntityType| {
            let mut scene = Scene::new();
            scene.add_entity(entity);
            scene.set_current_drawing_limits(glam::DVec2::ZERO, glam::DVec2::new(12.0, 9.0));
            scene.fit_all_with_limits();
            let target = scene.camera.borrow().target;
            target
        };

        // Template-default limits in inches while the geometry sits at
        // millimetre nesting coordinates: the frame centres on the union of
        // limits and extents, not on the 12×9 limits rectangle.
        let mut line = acadrust::entities::Line::new();
        line.start = acadrust::types::Vector3::new(1000.0, 1000.0, 0.0);
        line.end = acadrust::types::Vector3::new(2000.0, 2000.0, 0.0);
        let target = frame(EntityType::Line(line));
        assert!((target.x - 1000.0).abs() < 1.0, "target: {target:?}");
        assert!((target.y - 1000.0).abs() < 1.0, "target: {target:?}");

        // A drawing fully inside the limits still frames the whole limits
        // rectangle (AutoCAD keeps the limits as the frame).
        let mut line = acadrust::entities::Line::new();
        line.start = acadrust::types::Vector3::new(3.0, 3.0, 0.0);
        line.end = acadrust::types::Vector3::new(5.0, 5.0, 0.0);
        let target = frame(EntityType::Line(line));
        assert!((target.x - 6.0).abs() < 1.0, "target: {target:?}");
        assert!((target.y - 4.5).abs() < 1.0, "target: {target:?}");
    }
}
