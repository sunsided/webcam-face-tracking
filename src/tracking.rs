//! From two eye positions in a webcam frame to the viewer's eye point in window space.
//!
//! The camera is modelled as a pinhole looking straight out of the screen (along `+z`). Depth
//! comes from the eyes' pixel distance against an assumed interpupillary distance, which is
//! good to a few centimeters and needs no second camera.

use crate::projection::Vec3;

/// Physical setup of the screen and the webcam. All lengths are meters.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Calibration {
    /// Width and height of the visible canvas (the "window").
    pub window_size: [f32; 2],
    /// Horizontal field of view of the webcam, in degrees.
    pub camera_hfov_deg: f32,
    /// Position of the webcam lens relative to the window center.
    pub camera_pos: Vec3,
    /// Distance between the pupils. 63 mm is the adult average.
    pub ipd: f32,
}

impl Default for Calibration {
    fn default() -> Self {
        // A 15.6" 16:9 laptop with the webcam in the top bezel.
        let window_size = [0.345, 0.194];
        Self {
            window_size,
            camera_hfov_deg: 60.0,
            camera_pos: [0.0, window_size[1] * 0.5 + 0.01, 0.0],
            ipd: 0.063,
        }
    }
}

/// Eye midpoint in window space from the left and right eye in image pixels (image `y` down,
/// unmirrored camera frame) of an `image_size` frame. `None` for degenerate input.
pub fn eye_point(
    left: [f32; 2],
    right: [f32; 2],
    image_size: [f32; 2],
    cal: &Calibration,
) -> Option<Vec3> {
    let dx = right[0] - left[0];
    let dy = right[1] - left[1];
    let d_px = (dx * dx + dy * dy).sqrt();
    if d_px < 1.0 || image_size[0] <= 0.0 || image_size[1] <= 0.0 {
        return None;
    }
    let f_px = image_size[0] * 0.5 / (cal.camera_hfov_deg.to_radians() * 0.5).tan();
    let z = f_px * cal.ipd / d_px;
    let mid = [(left[0] + right[0]) * 0.5, (left[1] + right[1]) * 0.5];
    // The camera faces the viewer: moving to your right moves you left in its raw image, and
    // image y grows downward.
    let x = -(mid[0] - image_size[0] * 0.5) * z / f_px;
    let y = -(mid[1] - image_size[1] * 0.5) * z / f_px;
    Some([
        cal.camera_pos[0] + x,
        cal.camera_pos[1] + y,
        cal.camera_pos[2] + z,
    ])
}

/// One Euro filter (Casiez et al. 2012): low jitter when still, low lag when moving.
#[derive(Copy, Clone, Debug)]
pub struct OneEuro {
    /// Cutoff in Hz at zero speed. Lower is smoother.
    pub min_cutoff: f32,
    /// How fast the cutoff rises with speed. Higher is less laggy.
    pub beta: f32,
    /// Cutoff for the derivative estimate, in Hz.
    pub d_cutoff: f32,
    state: Option<(Vec3, Vec3)>,
}

impl OneEuro {
    pub fn new(min_cutoff: f32, beta: f32) -> Self {
        Self {
            min_cutoff,
            beta,
            d_cutoff: 1.0,
            state: None,
        }
    }

    pub fn reset(&mut self) {
        self.state = None;
    }

    /// Feed a sample taken `dt` seconds after the previous one; returns the filtered value.
    pub fn filter(&mut self, x: Vec3, dt: f32) -> Vec3 {
        let Some((prev, prev_dx)) = self.state else {
            self.state = Some((x, [0.0; 3]));
            return x;
        };
        let dt = dt.max(1e-4);
        let mut out = [0.0; 3];
        let mut dx = [0.0; 3];
        for i in 0..3 {
            let raw_dx = (x[i] - prev[i]) / dt;
            dx[i] = lerp(prev_dx[i], raw_dx, alpha(self.d_cutoff, dt));
            let cutoff = self.min_cutoff + self.beta * dx[i].abs();
            out[i] = lerp(prev[i], x[i], alpha(cutoff, dt));
        }
        self.state = Some((out, dx));
        out
    }
}

fn alpha(cutoff: f32, dt: f32) -> f32 {
    let tau = 1.0 / (std::f32::consts::TAU * cutoff);
    1.0 / (1.0 + tau / dt)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cal() -> Calibration {
        Calibration {
            camera_pos: [0.0, 0.1, 0.0],
            ..Default::default()
        }
    }

    #[test]
    fn centered_face_is_on_the_camera_axis() {
        let img = [640.0, 480.0];
        let p = eye_point([300.0, 240.0], [340.0, 240.0], img, &cal()).unwrap();
        assert!(p[0].abs() < 1e-6 && (p[1] - 0.1).abs() < 1e-6);
        // 40 px apart at f = 320 / tan(30°) ≈ 554 px puts 63 mm at ≈ 0.87 m.
        assert!((p[2] - 0.8730).abs() < 1e-3, "{p:?}");
    }

    #[test]
    fn closer_face_has_larger_eye_distance() {
        let img = [640.0, 480.0];
        let far = eye_point([300.0, 240.0], [340.0, 240.0], img, &cal()).unwrap();
        let near = eye_point([280.0, 240.0], [360.0, 240.0], img, &cal()).unwrap();
        assert!((near[2] - far[2] * 0.5).abs() < 1e-5);
    }

    #[test]
    fn image_left_is_viewer_right_and_image_up_is_up() {
        let img = [640.0, 480.0];
        let p = eye_point([100.0, 100.0], [140.0, 100.0], img, &cal()).unwrap();
        assert!(p[0] > 0.0, "{p:?}");
        assert!(p[1] > 0.1, "{p:?}");
    }

    #[test]
    fn degenerate_input_is_rejected() {
        assert!(eye_point([10.0, 10.0], [10.0, 10.0], [640.0, 480.0], &cal()).is_none());
    }

    #[test]
    fn one_euro_converges_on_a_constant() {
        let mut f = OneEuro::new(1.0, 0.0);
        f.filter([0.0; 3], 1.0 / 30.0);
        let mut v = [0.0; 3];
        for _ in 0..120 {
            v = f.filter([1.0, 2.0, 3.0], 1.0 / 30.0);
        }
        assert!(
            (v[0] - 1.0).abs() < 1e-3 && (v[2] - 3.0).abs() < 1e-3,
            "{v:?}"
        );
    }
}
