//! The JavaScript-facing API. The page owns the webcam and the face tracker and pushes eye
//! positions in; the renderer turns them into a head position.

use wasm_bindgen::prelude::*;

use crate::{Observation, with_shared};

#[wasm_bindgen(start)]
fn start() {
    if let Err(e) = crate::run() {
        log::error!("failed to start: {e:#}");
    }
}

/// A tracked frame: the left and right eye centers in pixels of an unmirrored
/// `image_w` x `image_h` camera frame, the iris offset from the eye centers in eye widths
/// (image orientation), captured at `time_ms` (e.g. `performance.now()`).
#[wasm_bindgen]
// Flat numbers keep the per-frame call cheap; a JS object would need serde or reflection.
#[allow(clippy::too_many_arguments)]
pub fn push_eyes(
    left_x: f32,
    left_y: f32,
    right_x: f32,
    right_y: f32,
    image_w: f32,
    image_h: f32,
    gaze_x: f32,
    gaze_y: f32,
    time_ms: f64,
) {
    let o = Observation {
        left: [left_x, left_y],
        right: [right_x, right_y],
        image_size: [image_w, image_h],
        gaze: [gaze_x, gaze_y],
        time_ms,
    };
    with_shared(|s| {
        s.observation = Some(o);
        s.last_observation = Some(o);
    });
}

/// The tracker lost the face; the next detection starts a fresh filter.
#[wasm_bindgen]
pub fn face_lost() {
    with_shared(|s| s.face_lost = true);
}

/// Physical size of the canvas in meters.
#[wasm_bindgen]
pub fn set_window_size(width_m: f32, height_m: f32) {
    with_shared(|s| {
        s.window_size = (width_m > 0.0 && height_m > 0.0).then_some([width_m, height_m]);
    });
}

/// Webcam horizontal field of view and its lens position relative to the canvas center, in
/// meters (`y` up, `z` toward the viewer).
#[wasm_bindgen]
pub fn set_camera(hfov_deg: f32, x: f32, y: f32, z: f32) {
    with_shared(|s| {
        s.calibration.camera_hfov_deg = hfov_deg.clamp(10.0, 170.0);
        s.calibration.camera_pos = [x, y, z];
    });
}

/// Interpupillary distance in meters.
#[wasm_bindgen]
pub fn set_ipd(ipd_m: f32) {
    with_shared(|s| s.calibration.ipd = ipd_m.clamp(0.04, 0.09));
}

/// Light position relative to the eye, in meters. All zeros puts it in the eye, which hides
/// every shadow behind the object casting it.
#[wasm_bindgen]
pub fn set_light_offset(x: f32, y: f32, z: f32) {
    with_shared(|s| s.light_offset = [x, y, z]);
}

/// Drive the head with the mouse (and the wheel for distance) instead of the tracker.
#[wasm_bindgen]
pub fn set_mouse_mode(on: bool) {
    with_shared(|s| s.mouse_mode = on);
}

/// The eye point of the last frame as `[x, y, z, tracked]`, `tracked` being 1 or 0.
#[wasm_bindgen]
pub fn head() -> Vec<f32> {
    with_shared(|s| {
        let (p, tracked) = s.head;
        vec![p[0], p[1], p[2], f32::from(u8::from(tracked))]
    })
}

/// Make the current tracked eye position the straight-on view, and the current gaze
/// "looking straight at the screen". Returns `[eye x, eye y, gaze x, gaze y]` for the page to
/// store, or nothing when no face has been tracked yet.
#[wasm_bindgen]
pub fn center_view() -> Option<Vec<f32>> {
    with_shared(|s| {
        let p = s.raw_eye?;
        s.neutral = [p[0], p[1]];
        if let Some(g) = s.raw_gaze {
            s.gaze_neutral = g;
        }
        Some(vec![
            s.neutral[0],
            s.neutral[1],
            s.gaze_neutral[0],
            s.gaze_neutral[1],
        ])
    })
}

/// Set the straight-on eye position and the straight-ahead gaze (see [`center_view`]).
#[wasm_bindgen]
pub fn set_neutral(x: f32, y: f32, gaze_x: f32, gaze_y: f32) {
    with_shared(|s| {
        s.neutral = [x, y];
        s.gaze_neutral = [gaze_x, gaze_y];
    });
}

/// Turn the light into a spotlight that points where the viewer looks. `gain` scales the
/// measured iris offset; 1 is the anatomical estimate.
#[wasm_bindgen]
pub fn set_flashlight(on: bool, gain: f32) {
    with_shared(|s| {
        s.flashlight = on;
        s.gaze_gain = crate::tracking::GAZE_GAIN * gain.clamp(0.1, 5.0);
    });
}

/// GPU backend and adapter name, empty until the renderer is up.
#[wasm_bindgen]
pub fn renderer_info() -> String {
    with_shared(|s| s.renderer_info.clone())
}

/// `false` keeps the view fixed straight in front of the window and moves only the light
/// with the head; `true` (default) re-projects the room from the eye.
#[wasm_bindgen]
pub fn set_view_follows(on: bool) {
    with_shared(|s| s.view_follows = on);
}

/// Tracking feel: `smoothing` from 0 (raw, jittery) to 1 (smooth, laggy), and how far past
/// the last measurement the head may be extrapolated, in milliseconds (0 = no prediction).
#[wasm_bindgen]
pub fn set_tracking(smoothing: f32, prediction_ms: f32) {
    with_shared(|s| {
        s.smoothing = smoothing.clamp(0.0, 1.0);
        s.max_lead = (prediction_ms / 1000.0).clamp(0.0, 0.3);
    });
}

/// Mirror the view's parallax left/right and up/down. Only the view: the light keeps
/// following the real head position.
#[wasm_bindgen]
pub fn set_view_invert(x: bool, y: bool) {
    with_shared(|s| s.view_invert = [x, y]);
}

/// Calibrate the webcam's field of view from the viewer sitting `distance_m` from the screen
/// right now. Applies and returns the new horizontal FOV in degrees, or nothing without a
/// tracked face.
#[wasm_bindgen]
pub fn calibrate_distance(distance_m: f32) -> Option<f32> {
    with_shared(|s| {
        let o = s.last_observation?;
        let d = (o.right[0] - o.left[0]).hypot(o.right[1] - o.left[1]);
        let fov = crate::tracking::fov_from_distance(
            d,
            o.image_size[0],
            distance_m + s.calibration.camera_pos[2],
            s.calibration.ipd,
        )?;
        s.calibration.camera_hfov_deg = fov;
        Some(fov)
    })
}
