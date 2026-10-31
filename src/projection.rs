//! Off-axis ("window") projection.
//!
//! World units are meters. The window (the visible canvas) lies in the `z = 0` plane, centered
//! on the origin, with `x` to the right, `y` up and `+z` pointing out of the screen toward the
//! viewer. The room behind it extends into `-z`.
//!
//! For an eye at `e` the frustum is the pyramid from `e` through the window rectangle: an
//! asymmetric frustum whose near-plane extents are the window edges scaled by `near / e.z`.
//! This is Kooima's generalized perspective projection, simplified because the window is
//! axis-aligned, so the view matrix is a pure translation.

use game_toolkit::prelude::{Mat4, transform};

pub type Vec3 = [f32; 3];

/// Closest the eye may get to the window plane. Below this the frustum degenerates.
pub const MIN_EYE_DISTANCE: f32 = 0.05;

/// View-projection for an eye looking through a `2 * half_w` by `2 * half_h` window in the
/// `z = 0` plane. Depth maps to wgpu's `[0, 1]`.
pub fn off_axis(eye: Vec3, half_w: f32, half_h: f32, near: f32, far: f32) -> Mat4 {
    let d = eye[2].max(MIN_EYE_DISTANCE);
    let s = near / d;
    let left = (-half_w - eye[0]) * s;
    let right = (half_w - eye[0]) * s;
    let bottom = (-half_h - eye[1]) * s;
    let top = (half_h - eye[1]) * s;
    transform::mul(
        &frustum(left, right, bottom, top, near, far),
        &transform::translation([-eye[0], -eye[1], -d]),
    )
}

/// Right-handed asymmetric frustum (OpenGL `glFrustum`) with a `[0, 1]` depth range.
pub fn frustum(l: f32, r: f32, b: f32, t: f32, n: f32, f: f32) -> Mat4 {
    [
        [2.0 * n / (r - l), 0.0, 0.0, 0.0],
        [0.0, 2.0 * n / (t - b), 0.0, 0.0],
        [(r + l) / (r - l), (t + b) / (t - b), f / (n - f), -1.0],
        [0.0, 0.0, n * f / (n - f), 0.0],
    ]
}

/// Apply `m` to the point `p` and divide by `w`.
pub fn project(m: &Mat4, p: Vec3) -> Vec3 {
    let mut out = [0.0f32; 4];
    for (row, o) in out.iter_mut().enumerate() {
        *o = m[0][row] * p[0] + m[1][row] * p[1] + m[2][row] * p[2] + m[3][row];
    }
    [out[0] / out[3], out[1] / out[3], out[2] / out[3]]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    #[test]
    fn window_corners_hit_ndc_corners_from_any_eye() {
        let (hw, hh) = (0.17, 0.095);
        for eye in [[0.0, 0.0, 0.6], [0.2, -0.1, 0.4], [-0.5, 0.3, 1.2]] {
            let m = off_axis(eye, hw, hh, 0.05, 10.0);
            let tr = project(&m, [hw, hh, 0.0]);
            let bl = project(&m, [-hw, -hh, 0.0]);
            assert!(
                close([tr[0], tr[1], 0.0], [1.0, 1.0, 0.0]),
                "{eye:?}: {tr:?}"
            );
            assert!(
                close([bl[0], bl[1], 0.0], [-1.0, -1.0, 0.0]),
                "{eye:?}: {bl:?}"
            );
        }
    }

    #[test]
    fn depth_range_is_zero_to_one() {
        let eye = [0.1, 0.05, 0.5];
        let (n, f) = (0.05, 5.0);
        let m = off_axis(eye, 0.2, 0.1, n, f);
        // Points straight ahead of the eye at the near and far plane.
        let near = project(&m, [eye[0], eye[1], eye[2] - n]);
        let far = project(&m, [eye[0], eye[1], eye[2] - f]);
        assert!(near[2].abs() < 1e-4, "{near:?}");
        assert!((far[2] - 1.0).abs() < 1e-4, "{far:?}");
    }

    #[test]
    fn points_behind_the_window_follow_the_eye() {
        // Parallax: relative to the window frame, a point behind it moves with the eye.
        let p = [0.0, 0.0, -0.3];
        let a = project(&off_axis([0.0, 0.0, 0.6], 0.2, 0.1, 0.05, 5.0), p);
        let b = project(&off_axis([0.1, 0.0, 0.6], 0.2, 0.1, 0.05, 5.0), p);
        assert!(b[0] > a[0]);
        // A point in front of the window moves against it.
        let q = [0.0, 0.0, 0.2];
        let a = project(&off_axis([0.0, 0.0, 0.6], 0.2, 0.1, 0.05, 5.0), q);
        let b = project(&off_axis([0.1, 0.0, 0.6], 0.2, 0.1, 0.05, 5.0), q);
        assert!(b[0] < a[0]);
    }
}
