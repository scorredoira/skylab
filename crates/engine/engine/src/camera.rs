use glam::{DVec3, Mat4, Vec3, Vec4};

/// A camera in a world too big for `f32`: its position is `f64`, and everything is drawn relative to it.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub position: DVec3,
    pub forward: Vec3,
    pub up: Vec3,
    pub right: Vec3,
    /// Vertical field of view, radians.
    pub fov_y: f32,
    /// Moves the point the camera looks at up the screen, as a fraction of half its height.
    pub shift_y: f32,
    pub near: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            position: DVec3::ZERO,
            forward: Vec3::NEG_Z,
            up: Vec3::Y,
            right: Vec3::X,
            fov_y: 62f32.to_radians(),
            shift_y: 0.0,
            near: 0.25,
        }
    }
}

impl Camera {
    /// View-projection for camera-relative positions. Depth is reversed (1 at the near plane, 0 at
    /// infinity), which keeps precision over tens of kilometres; clear depth to 0 and test with `GreaterEqual`.
    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let f = 1.0 / (self.fov_y / 2.0).tan();
        let proj = Mat4::from_cols(
            Vec4::new(f / aspect, 0.0, 0.0, 0.0),
            Vec4::new(0.0, f, 0.0, 0.0),
            Vec4::new(0.0, -self.shift_y, 0.0, -1.0),
            Vec4::new(0.0, 0.0, self.near, 0.0),
        );
        let view = Mat4::from_cols(
            Vec4::new(self.right.x, self.up.x, -self.forward.x, 0.0),
            Vec4::new(self.right.y, self.up.y, -self.forward.y, 0.0),
            Vec4::new(self.right.z, self.up.z, -self.forward.z, 0.0),
            Vec4::W,
        );
        proj * view
    }

    /// Tangents of half the field of view, horizontal and vertical.
    pub fn tan_half_fov(&self, aspect: f32) -> (f32, f32) {
        let t = (self.fov_y / 2.0).tan();
        (t * aspect, t)
    }
}
