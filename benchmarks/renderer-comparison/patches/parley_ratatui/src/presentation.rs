//! Helpers for presenting terminal render textures without accidental resampling.
//!
//! These helpers are independent of Bevy so they can be shared by Bevy examples,
//! Ratty, and other texture-based integrations. A direct Vello-to-Bevy-GPU
//! target is preferable, but Bevy 0.18 currently uses `wgpu` 27 while Vello 0.9
//! uses `wgpu` 29, so the two device/texture types cannot be passed across the
//! public APIs safely.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PresentationScale {
    pub logical_size: [f32; 2],
    pub physical_size: [u32; 2],
    pub reported_scale_factor: f32,
    pub base_scale_factor: f32,
}

impl PresentationScale {
    pub fn new(
        logical_size: [f32; 2],
        physical_size: [u32; 2],
        reported_scale_factor: f32,
        base_scale_factor: f32,
    ) -> Self {
        Self {
            logical_size,
            physical_size,
            reported_scale_factor,
            base_scale_factor,
        }
    }

    pub fn render_scale(self) -> f32 {
        let logical_width = self.logical_size[0].max(1.0);
        let logical_height = self.logical_size[1].max(1.0);
        let physical_width = self.physical_size[0] as f32;
        let physical_height = self.physical_size[1] as f32;
        let physical_scale = (physical_width / logical_width)
            .min(physical_height / logical_height)
            .max(1.0);

        physical_scale
            .max(self.reported_scale_factor)
            .max(self.base_scale_factor)
            .max(1.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TexturePresentation {
    pub texture_size: [u32; 2],
    pub render_scale: f32,
}

impl TexturePresentation {
    pub fn new(texture_size: [u32; 2], render_scale: f32) -> Self {
        Self {
            texture_size,
            render_scale: render_scale.max(1.0),
        }
    }

    pub fn logical_size(self) -> [f32; 2] {
        [
            self.texture_size[0] as f32 / self.render_scale,
            self.texture_size[1] as f32 / self.render_scale,
        ]
    }

    pub fn expected_physical_size(self) -> [f32; 2] {
        let logical_size = self.logical_size();
        [
            logical_size[0] * self.render_scale,
            logical_size[1] * self.render_scale,
        ]
    }

    pub fn physical_delta(self) -> [f32; 2] {
        let expected = self.expected_physical_size();
        [
            expected[0] - self.texture_size[0] as f32,
            expected[1] - self.texture_size[1] as f32,
        ]
    }

    pub fn is_exact(self) -> bool {
        let delta = self.physical_delta();
        delta[0].abs() <= 0.01 && delta[1].abs() <= 0.01
    }
}

pub fn snap_logical_to_physical_pixel(value: f32, render_scale: f32) -> f32 {
    let render_scale = render_scale.max(1.0);
    (value * render_scale).round() / render_scale
}

pub fn snap_logical_position_to_physical_pixel(position: [f32; 2], render_scale: f32) -> [f32; 2] {
    [
        snap_logical_to_physical_pixel(position[0], render_scale),
        snap_logical_to_physical_pixel(position[1], render_scale),
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        PresentationScale, TexturePresentation, snap_logical_position_to_physical_pixel,
        snap_logical_to_physical_pixel,
    };

    #[test]
    fn render_scale_uses_actual_physical_ratio() {
        let scale = PresentationScale::new([800.0, 600.0], [1600, 1200], 1.0, 1.0);

        assert_eq!(scale.render_scale(), 2.0);
    }

    #[test]
    fn render_scale_respects_reported_and_base_scale_factors() {
        let scale = PresentationScale::new([800.0, 600.0], [800, 600], 1.5, 2.0);

        assert_eq!(scale.render_scale(), 2.0);
    }

    #[test]
    fn texture_presentation_maps_physical_texture_to_logical_size() {
        let presentation = TexturePresentation::new([1600, 900], 2.0);

        assert_eq!(presentation.logical_size(), [800.0, 450.0]);
        assert!(presentation.is_exact());
    }

    #[test]
    fn logical_values_snap_to_physical_pixel_grid() {
        assert_eq!(snap_logical_to_physical_pixel(0.26, 2.0), 0.5);
        assert_eq!(
            snap_logical_position_to_physical_pixel([0.24, 0.76], 2.0),
            [0.0, 1.0]
        );
    }
}
