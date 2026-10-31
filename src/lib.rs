//! A room seen through the screen as if it were a window. The viewer's head position (from a
//! webcam face tracker in the browser, or the mouse) drives an off-axis projection, and a
//! light next to the head casts shadows into the room.

#![forbid(unsafe_code)]

pub mod projection;
pub mod scene;
pub mod tracking;

mod renderer;
#[cfg(target_arch = "wasm32")]
mod web;

use std::cell::RefCell;

use game_toolkit::prelude::*;

use crate::projection::{Vec3, off_axis};
use crate::renderer::{Globals, Renderer};
use crate::scene::Room;
use crate::tracking::{Calibration, OneEuro};

/// Where the eye rests when nothing is tracked: centered, 60 cm from the screen.
const DEFAULT_EYE: Vec3 = [0.0, 0.0, 0.6];
/// Window size guess when the page has not told us: about a 15.6" 1080p panel.
const DEFAULT_METERS_PER_PIXEL: f32 = 0.345 / 1920.0;
/// How long a lost face keeps its last position before easing back to [`DEFAULT_EYE`].
const FACE_HOLD_SECS: f32 = 1.0;
/// The shadow frustum is this much wider than the window, so light reaches the walls next to
/// the opening even when the light sits off to the side.
const LIGHT_SPREAD: f32 = 1.8;

/// State the page writes (tracking, calibration) and reads (the current head position).
/// The browser runs everything on one thread, so a thread-local is all the sharing needed.
struct Shared {
    calibration: Calibration,
    /// `None` until the page reports the canvas' physical size.
    window_size: Option<[f32; 2]>,
    light_offset: Vec3,
    mouse_mode: bool,
    /// Fixed eye point overriding tracker and mouse (native screenshots, debugging).
    pinned_eye: Option<Vec3>,
    observation: Option<Observation>,
    face_lost: bool,
    /// The eye point used for the last frame, and whether it came from the tracker.
    head: (Vec3, bool),
}

#[derive(Copy, Clone, Debug)]
struct Observation {
    left: [f32; 2],
    right: [f32; 2],
    image_size: [f32; 2],
    time_ms: f64,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            calibration: Calibration::default(),
            window_size: None,
            light_offset: [0.0, 0.08, 0.0],
            mouse_mode: cfg!(not(target_arch = "wasm32")),
            pinned_eye: None,
            observation: None,
            face_lost: false,
            head: (DEFAULT_EYE, false),
        }
    }
}

thread_local! {
    static SHARED: RefCell<Shared> = RefCell::new(Shared::default());
}

fn with_shared<R>(f: impl FnOnce(&mut Shared) -> R) -> R {
    SHARED.with(|s| f(&mut s.borrow_mut()))
}

struct WindowRoom {
    renderer: Renderer,
    mesh: scene::Mesh,
    room: Room,
    filter: OneEuro,
    last_obs_ms: Option<f64>,
    since_face: f32,
    eye: Vec3,
    light: Vec3,
    mouse_z: f32,
    time: f32,
    frames: u32,
}

impl Game for WindowRoom {
    fn init(ctx: &mut Context) -> Result<Self> {
        let room = Room::for_window(window_size(ctx));
        let mesh = scene::build(&room, 0.0);
        let renderer = Renderer::new(&ctx.gfx, &mesh);
        Ok(Self {
            renderer,
            mesh,
            room,
            filter: OneEuro::new(1.2, 3.0),
            last_obs_ms: None,
            since_face: f32::INFINITY,
            eye: DEFAULT_EYE,
            light: DEFAULT_EYE,
            mouse_z: DEFAULT_EYE[2],
            time: 0.0,
            frames: 0,
        })
    }

    fn update(&mut self, ctx: &mut Context, dt: f32) {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        self.time += dt;
        self.frames += 1;
        // `FACE_WINDOW_SHOT=out.png`: save a frame once things settled, then quit.
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(path) = std::env::var("FACE_WINDOW_SHOT") {
            match self.frames {
                30 => ctx.gfx.request_screenshot(path),
                40 => ctx.quit(),
                _ => {}
            }
        }
        self.room = Room::for_window(window_size(ctx));

        let (cal, obs, lost, mouse_mode, light_offset, pinned) = with_shared(|s| {
            let lost = std::mem::take(&mut s.face_lost);
            let obs = s.observation.take();
            (
                s.calibration,
                obs,
                lost,
                s.mouse_mode,
                s.light_offset,
                s.pinned_eye,
            )
        });
        let cal = Calibration {
            window_size: [self.room.half_w * 2.0, self.room.half_h * 2.0],
            ..cal
        };

        if lost {
            self.filter.reset();
            self.last_obs_ms = None;
        }
        let mut tracked = false;
        if let Some(o) = obs
            && let Some(p) = tracking::eye_point(o.left, o.right, o.image_size, &cal)
        {
            let obs_dt = self
                .last_obs_ms
                .map_or(1.0 / 30.0, |t| ((o.time_ms - t) / 1000.0) as f32);
            self.last_obs_ms = Some(o.time_ms);
            self.eye = self.filter.filter(p, obs_dt);
            self.since_face = 0.0;
            tracked = true;
        } else {
            self.since_face += dt;
        }

        if let Some(p) = pinned {
            self.eye = p;
        } else if mouse_mode {
            let (w, h) = ctx.gfx.size();
            let (mx, my) = ctx.input.mouse_pos();
            let nx = (mx / w.max(1) as f32) * 2.0 - 1.0;
            let ny = (my / h.max(1) as f32) * 2.0 - 1.0;
            self.mouse_z = (self.mouse_z - ctx.input.scroll().1 * 0.02).clamp(0.15, 2.0);
            self.eye = [
                nx * self.room.half_w * 2.5,
                -ny * self.room.half_h * 2.5,
                self.mouse_z,
            ];
        } else if self.since_face > FACE_HOLD_SECS {
            // Nobody in view: drift back to the rest position instead of freezing mid-lean.
            let k = 1.0 - (-dt * 2.0).exp();
            for (e, d) in self.eye.iter_mut().zip(DEFAULT_EYE) {
                *e += (d - *e) * k;
            }
        }
        self.eye[2] = self.eye[2].max(projection::MIN_EYE_DISTANCE * 2.0);
        self.light = [
            self.eye[0] + light_offset[0],
            self.eye[1] + light_offset[1],
            (self.eye[2] + light_offset[2]).max(projection::MIN_EYE_DISTANCE * 2.0),
        ];
        let tracked_now = tracked || (!mouse_mode && self.since_face <= FACE_HOLD_SECS);
        with_shared(|s| s.head = (self.eye, tracked_now));

        self.mesh = scene::build(&self.room, self.time);
    }

    fn render(&mut self, ctx: &mut Context, frame: &mut Frame) {
        // The toolkit clears and composites its 2D layers on flush; flush the (empty) 2D
        // layers first so that does not run after, and over, the room.
        ctx.gfx.flush_pending(frame);

        let Room {
            half_w,
            half_h,
            depth,
        } = self.room;
        let far = self.eye[2] + depth + 1.0;
        let globals = Globals {
            view_proj: off_axis(self.eye, half_w, half_h, 0.02, far),
            light_view_proj: off_axis(
                self.light,
                half_w * LIGHT_SPREAD,
                half_h * LIGHT_SPREAD,
                0.02,
                self.light[2] + depth + 1.0,
            ),
            eye: [self.eye[0], self.eye[1], self.eye[2], 1.0],
            light: [self.light[0], self.light[1], self.light[2], 1.8],
            params: [
                self.room.grid(),
                0.05,
                self.renderer.shadow_texel(),
                depth * 0.004,
            ],
            room: [half_w, half_h, depth, self.light[2]],
        };
        self.renderer.draw(&ctx.gfx, frame, &self.mesh, &globals);
    }
}

/// The window's physical size in meters: what the page reported, or a guess from pixels.
fn window_size(ctx: &Context) -> [f32; 2] {
    with_shared(|s| s.window_size).unwrap_or_else(|| {
        let (w, h) = ctx.gfx.size();
        [
            w.max(1) as f32 * DEFAULT_METERS_PER_PIXEL,
            h.max(1) as f32 * DEFAULT_METERS_PER_PIXEL,
        ]
    })
}

/// Pin the eye to a fixed point, or `None` to follow the tracker or mouse again.
pub fn pin_eye(eye: Option<Vec3>) {
    with_shared(|s| s.pinned_eye = eye);
}

/// Start the app. Natively this blocks until the window closes; on the web it returns at once
/// and renders into `<canvas id="room">`.
pub fn run() -> Result<()> {
    game_toolkit::prelude::run::<WindowRoom>(AppConfig {
        title: "face-window".into(),
        width: 1280,
        height: 720,
        canvas_id: Some("room".into()),
        // Nothing to preload; skips fetching an asset manifest.
        web_preload: Some(Vec::new()),
        ..Default::default()
    })
}
