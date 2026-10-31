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
/// `image_w` x `image_h` camera frame, captured at `time_ms` (e.g. `performance.now()`).
#[wasm_bindgen]
pub fn push_eyes(
    left_x: f32,
    left_y: f32,
    right_x: f32,
    right_y: f32,
    image_w: f32,
    image_h: f32,
    time_ms: f64,
) {
    with_shared(|s| {
        s.observation = Some(Observation {
            left: [left_x, left_y],
            right: [right_x, right_y],
            image_size: [image_w, image_h],
            time_ms,
        });
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
