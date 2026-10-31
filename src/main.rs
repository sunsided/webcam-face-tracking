//! Native build for quick iteration: the mouse stands in for the head, the wheel for distance.
//! The browser build starts from the library's `#[wasm_bindgen(start)]` instead.
//!
//! `FACE_WINDOW_EYE=x,y,z` pins the eye (meters from the window center) instead.

#[cfg(not(target_arch = "wasm32"))]
fn main() -> game_toolkit::prelude::Result<()> {
    env_logger::init();
    if let Ok(v) = std::env::var("FACE_WINDOW_EYE") {
        let p: Vec<f32> = v.split(',').filter_map(|c| c.trim().parse().ok()).collect();
        if let [x, y, z] = p[..] {
            face_window::pin_eye(Some([x, y, z]));
        }
    }
    face_window::run()
}

#[cfg(target_arch = "wasm32")]
fn main() {}
