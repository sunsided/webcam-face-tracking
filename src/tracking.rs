//! From two eye positions in a webcam frame to the viewer's eye point in window space.
//!
//! The camera is modelled as a pinhole looking straight out of the screen (along `+z`). Depth
//! comes from the eyes' pixel distance against an assumed interpupillary distance, which is
//! good to a few centimeters and needs no second camera.

use minikalman::models::motion;
use minikalman::prelude::*;
use minikalman::regular;

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

/// Per-axis constant-velocity Kalman filter over the eye point.
///
/// Besides smoothing, the velocity estimate lets the renderer extrapolate the head to the time
/// a frame is shown, which hides most of the camera and detection latency. Latency, more than
/// precision, is what breaks the window illusion.
pub struct HeadFilter {
    axes: [Axis; 3],
    /// Variance of the head's acceleration per axis, in (m/s²)². Higher follows faster.
    pub acceleration_variance: Vec3,
    /// Variance of a single measurement per axis, in m². Depth is the noisiest.
    pub measurement_variance: Vec3,
    /// Time of the last measurement, in seconds, on the caller's clock.
    last: Option<f64>,
}

struct Axis {
    filter: regular::builder::KalmanFilterType<2, f32>,
    measurement: regular::builder::KalmanFilterObservationType<2, 1, f32>,
}

impl Axis {
    fn new() -> Self {
        let builder = regular::builder::KalmanFilterBuilder::<2, f32>::default();
        let filter = builder.build();
        let mut measurement = builder.observations().build::<1>();
        measurement.observation_matrix_mut().apply(|h| {
            h.set_at(0, 0, 1.0);
            h.set_at(0, 1, 0.0);
        });
        Self {
            filter,
            measurement,
        }
    }

    fn reset(&mut self, position: f32, measurement_variance: f32) {
        self.filter.state_vector_mut().apply(|x| {
            x[0] = position;
            x[1] = 0.0;
        });
        self.filter.estimate_covariance_mut().apply(|p| {
            p.make_identity();
            p.set_at(0, 0, measurement_variance);
        });
    }

    fn update(&mut self, z: f32, dt: f32, acceleration_variance: f32, measurement_variance: f32) {
        let model = motion::constant_velocity(dt, acceleration_variance);
        self.filter
            .state_transition_mut()
            .apply(|a| model.fill_transition(a));
        self.filter
            .direct_process_noise_mut()
            .apply(|q| model.fill_process_noise(q));
        self.filter.predict();
        self.measurement
            .measurement_noise_covariance_mut()
            .apply(|r| r.set_at(0, 0, measurement_variance));
        self.measurement
            .measurement_vector_mut()
            .apply(|v| v[0] = z);
        self.filter.correct(&mut self.measurement);
    }

    fn state(&self) -> (f32, f32) {
        let x = self.filter.state_vector();
        (x[0], x[1])
    }
}

impl Default for HeadFilter {
    fn default() -> Self {
        Self {
            axes: [Axis::new(), Axis::new(), Axis::new()],
            // Smooth over follow: at 15 to 30 Hz tracking, a noisy velocity estimate turns
            // into visible jitter once the renderer extrapolates with it.
            acceleration_variance: [1.0, 1.0, 0.5],
            // Roughly 5 mm (x, y) and 3 cm (depth from eye spacing) of noise per frame.
            measurement_variance: [2.5e-5, 2.5e-5, 9e-4],
            last: None,
        }
    }
}

impl HeadFilter {
    /// Forget the track; the next measurement starts a new one.
    pub fn reset(&mut self) {
        self.last = None;
    }

    /// Feed a measured eye point taken at `time` seconds.
    pub fn update(&mut self, p: Vec3, time: f64) {
        let dt = self.last.map(|t| (time - t) as f32);
        self.last = Some(time);
        let axes = self.axes.iter_mut().zip(p);
        for ((axis, z), (q, r)) in axes.zip(
            self.acceleration_variance
                .into_iter()
                .zip(self.measurement_variance),
        ) {
            match dt {
                // A gap this long is a new track, not motion.
                Some(dt) if dt > 0.0 && dt < 0.5 => axis.update(z, dt, q, r),
                _ => axis.reset(z, r),
            }
        }
    }

    /// The eye point extrapolated to `time` seconds, at most `max_lead` past the last
    /// measurement. `None` before the first measurement.
    pub fn predict(&self, time: f64, max_lead: f32) -> Option<Vec3> {
        let last = self.last?;
        let lead = ((time - last) as f32).clamp(0.0, max_lead);
        Some(self.axes.each_ref().map(|a| {
            let (p, v) = a.state();
            p + v * lead
        }))
    }
}

/// The webcam's horizontal field of view (degrees) that puts eyes `eye_px` apart in an
/// `image_w` wide frame at `distance` meters, for eyes `ipd` apart. The inverse of the depth
/// estimate in [`eye_point`]: calibrates the camera from one known viewing distance.
pub fn fov_from_distance(eye_px: f32, image_w: f32, distance: f32, ipd: f32) -> Option<f32> {
    if eye_px < 1.0 || distance <= 0.0 || ipd <= 0.0 {
        return None;
    }
    let f_px = distance * eye_px / ipd;
    Some((image_w * 0.5 / f_px).atan().to_degrees() * 2.0)
}

/// Eyeball radius over eye width (corner to corner): about 12 mm over 28 mm. Turns an iris
/// offset measured in eye widths into the sine of the gaze angle.
pub const GAZE_GAIN: f32 = 2.33;

/// Where a gaze ray from `eye` meets the plane `z = plane_z`. `gaze` is the iris offset from
/// the eye center in eye widths, in world orientation (`x` right, `y` up), with the neutral
/// (looking straight at the screen) already subtracted. `gain` scales it to `sin(angle)`.
pub fn gaze_hit(eye: Vec3, gaze: [f32; 2], gain: f32, plane_z: f32) -> Vec3 {
    let tan = |r: f32| {
        let s = (r * gain).clamp(-0.95, 0.95);
        s / (1.0 - s * s).sqrt()
    };
    let dist = eye[2] - plane_z;
    [
        eye[0] + tan(gaze[0]) * dist,
        eye[1] + tan(gaze[1]) * dist,
        plane_z,
    ]
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
    fn fov_calibration_inverts_the_depth_estimate() {
        let cal = cal();
        let img = [640.0, 480.0];
        let p = eye_point([300.0, 240.0], [340.0, 240.0], img, &cal).unwrap();
        let fov = fov_from_distance(40.0, img[0], p[2], cal.ipd).unwrap();
        assert!((fov - cal.camera_hfov_deg).abs() < 1e-3, "{fov}");
    }

    #[test]
    fn gaze_hits_straight_ahead_and_turns_with_the_eye() {
        let eye = [0.1, 0.05, 0.6];
        let ahead = gaze_hit(eye, [0.0, 0.0], GAZE_GAIN, -0.4);
        assert!((ahead[0] - 0.1).abs() < 1e-6 && (ahead[1] - 0.05).abs() < 1e-6);
        // 30° to the right: sin = 0.5, so the offset is 0.5 / gain eye widths.
        let right = gaze_hit(eye, [0.5 / GAZE_GAIN, 0.0], GAZE_GAIN, -0.4);
        let expected = 0.1 + (30f32).to_radians().tan() * 1.0;
        assert!((right[0] - expected).abs() < 1e-4, "{right:?}");
        assert_eq!(right[2], -0.4);
    }

    #[test]
    fn head_filter_settles_and_extrapolates_motion() {
        let mut f = HeadFilter::default();
        assert!(f.predict(0.0, 0.1).is_none());
        // Moving right at 0.2 m/s, sampled at 30 Hz.
        let mut t = 0.0;
        for _ in 0..60 {
            f.update([0.2 * t as f32, 0.0, 0.6], t);
            t += 1.0 / 30.0;
        }
        let last = t - 1.0 / 30.0;
        let now = f.predict(last, 0.1).unwrap();
        assert!((now[0] - 0.2 * last as f32).abs() < 2e-3, "{now:?}");
        assert!((now[2] - 0.6).abs() < 1e-3, "{now:?}");
        // 50 ms later the head should have moved on by about 1 cm.
        let ahead = f.predict(last + 0.05, 0.1).unwrap();
        assert!(
            (ahead[0] - now[0] - 0.01).abs() < 2e-3,
            "{ahead:?} vs {now:?}"
        );
        // Extrapolation is capped.
        let far = f.predict(last + 10.0, 0.1).unwrap();
        assert!((far[0] - now[0] - 0.02).abs() < 2e-3, "{far:?}");
    }
}
