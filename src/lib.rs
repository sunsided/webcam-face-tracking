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
use crate::tracking::{Calibration, HeadFilter};

/// Where the eye rests when nothing is tracked: centered, 60 cm from the screen.
const DEFAULT_EYE: Vec3 = [0.0, 0.0, 0.6];
/// Window size guess when the page has not told us: about a 15.6" 1080p panel.
const DEFAULT_METERS_PER_PIXEL: f32 = 0.345 / 1920.0;
/// How long a lost face keeps its last position before easing back to [`DEFAULT_EYE`].
const FACE_HOLD_SECS: f32 = 1.0;
/// How far ahead of now the head is predicted, in seconds: roughly camera exposure plus
/// readout, which the detection timestamp does not include.
const LATENCY_LEAD: f64 = 0.035;
/// How much of each new gaze sample is taken over; gaze from a webcam is noisy.
const GAZE_SMOOTHING: f32 = 0.25;
/// Flashlight beam radius where it meets the back wall, as a fraction of the room height.
const SPOT_RADIUS: f32 = 0.45;
/// The beam is fully bright out to this fraction of its radius, then fades.
const SPOT_CORE: f32 = 0.55;
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
    /// Tracked eye position (x, y) that counts as straight in front of the window. A laptop
    /// user's eyes usually sit well above the screen center, which physically means looking
    /// down at the floor; recentering trades exactness for a nicer default view.
    neutral: [f32; 2],
    /// The last tracked eye point before recentering, for picking a new neutral.
    raw_eye: Option<Vec3>,
    /// GPU adapter and backend, for the page's diagnostics.
    renderer_info: String,
    /// `false` keeps the camera fixed in front of the window and moves only the light.
    view_follows: bool,
    /// Mirror the view's parallax per axis. Physically wrong, yet on Markus's setup the
    /// mirrored re-projection felt more natural; the light always uses the real position.
    view_invert: [bool; 2],
    /// 0 (follow every measurement) to 1 (heavily smoothed).
    smoothing: f32,
    /// Longest extrapolation past the last measurement, in seconds. 0 turns prediction off.
    max_lead: f32,
    /// `true` turns the light into a spotlight aimed where the viewer looks.
    flashlight: bool,
    /// Gaze (world orientation) that counts as looking straight at the screen.
    gaze_neutral: [f32; 2],
    /// The smoothed gaze before subtracting the neutral, for picking a new neutral.
    raw_gaze: Option<[f32; 2]>,
    gaze_gain: f32,
    observation: Option<Observation>,
    /// The newest observation, kept for calibration (a web-only API).
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    last_observation: Option<Observation>,
    face_lost: bool,
    /// The eye point used for the last frame, and whether it came from the tracker.
    head: (Vec3, bool),
}

#[derive(Copy, Clone, Debug)]
struct Observation {
    left: [f32; 2],
    right: [f32; 2],
    /// Iris offset from the eye center in eye widths, image orientation (`x` right, `y` down).
    gaze: [f32; 2],
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
            neutral: [0.0, 0.0],
            raw_eye: None,
            renderer_info: String::new(),
            view_follows: true,
            view_invert: [true, true],
            smoothing: 0.5,
            max_lead: 0.08,
            flashlight: false,
            gaze_neutral: [0.0, 0.0],
            raw_gaze: None,
            gaze_gain: tracking::GAZE_GAIN,
            observation: None,
            last_observation: None,
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
    filter: HeadFilter,
    since_face: f32,
    eye: Vec3,
    /// Where the scene is viewed from: the eye, or a fixed point when only the light follows.
    view: Vec3,
    light: Vec3,
    /// Smoothed gaze in world orientation, before the neutral is subtracted.
    gaze: Option<[f32; 2]>,
    /// Unit direction of the spotlight, `None` for the plain lamp.
    spot: Option<Vec3>,
    mouse_z: f32,
    time: f32,
    frames: u32,
}

impl Game for WindowRoom {
    fn init(ctx: &mut Context) -> Result<Self> {
        let room = Room::for_window(window_size(ctx));
        let mesh = scene::build(&room, 0.0);
        let renderer = Renderer::new(&ctx.gfx, &mesh);
        let info = ctx.gfx.device().adapter_info();
        with_shared(|s| s.renderer_info = format!("{:?} {}", info.backend, info.name));
        Ok(Self {
            renderer,
            mesh,
            room,
            filter: HeadFilter::default(),
            since_face: f32::INFINITY,
            eye: DEFAULT_EYE,
            view: DEFAULT_EYE,
            light: DEFAULT_EYE,
            gaze: None,
            spot: None,
            mouse_z: DEFAULT_EYE[2],
            time: 0.0,
            frames: 0,
        })
    }

    fn update(&mut self, ctx: &mut Context, dt: f32) {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        self.frames += 1;
        // `FACE_WINDOW_SWEEP=dir`: freeze the scene, move the eye once around a loop in front
        // of the window and save a frame per step (for the README animation), then quit.
        #[cfg(not(target_arch = "wasm32"))]
        let sweeping = std::env::var("FACE_WINDOW_SWEEP").ok();
        #[cfg(target_arch = "wasm32")]
        let sweeping: Option<String> = None;
        if let Some(dir) = &sweeping {
            const STEPS: u32 = 48;
            const WARMUP: u32 = 30;
            self.time = 2.0;
            let step = self.frames.saturating_sub(WARMUP);
            if step >= STEPS {
                ctx.quit();
            }
            let a = std::f32::consts::TAU * step as f32 / STEPS as f32;
            let w = self.room.half_w;
            pin_eye(Some([w * 1.2 * a.sin(), w * 0.35 * (2.0 * a).sin(), 0.55]));
            if self.frames >= WARMUP {
                ctx.gfx
                    .request_screenshot(std::path::Path::new(dir).join(format!("{step:03}.png")));
            }
        } else {
            self.time += dt;
        }
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
        }
        let mut tracked = false;
        if let Some(o) = obs {
            // The camera faces the viewer, so image left is the viewer's right.
            let g = [-o.gaze[0], -o.gaze[1]];
            let smoothed = self.gaze.map_or(g, |p| {
                [
                    p[0] + (g[0] - p[0]) * GAZE_SMOOTHING,
                    p[1] + (g[1] - p[1]) * GAZE_SMOOTHING,
                ]
            });
            self.gaze = Some(smoothed);
            with_shared(|s| s.raw_gaze = Some(smoothed));
        }
        if let Some(o) = obs
            && let Some(p) = tracking::eye_point(o.left, o.right, o.image_size, &cal)
        {
            self.filter.update(p, o.time_ms / 1000.0);
            self.since_face = 0.0;
            tracked = true;
        } else {
            self.since_face += dt;
        }
        // Between camera frames (and ahead of them, to cover capture and detection latency)
        // the head moves on at its estimated velocity.
        let (smoothing, max_lead) = with_shared(|s| (s.smoothing, s.max_lead));
        let accel = 10f32.powf(1.0 - 3.0 * smoothing);
        self.filter.acceleration_variance = [accel, accel, accel * 0.5];
        if self.since_face <= FACE_HOLD_SECS
            && let Some(p) = self
                .filter
                .predict(now_ms() / 1000.0 + LATENCY_LEAD, max_lead)
        {
            let neutral = with_shared(|s| {
                s.raw_eye = Some(p);
                s.neutral
            });
            self.eye = [p[0] - neutral[0], p[1] - neutral[1], p[2]];
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
        let (view_follows, view_invert, flashlight, gaze_neutral, gaze_gain) = with_shared(|s| {
            (
                s.view_follows,
                s.view_invert,
                s.flashlight,
                s.gaze_neutral,
                s.gaze_gain,
            )
        });
        let sign = |invert: bool| if invert { -1.0 } else { 1.0 };
        self.view = if view_follows {
            [
                self.eye[0] * sign(view_invert[0]),
                self.eye[1] * sign(view_invert[1]),
                self.eye[2],
            ]
        } else {
            DEFAULT_EYE
        };
        // The flashlight aims along the gaze ray, at the point where it meets the back wall.
        self.spot = match self.gaze {
            Some(g) if flashlight && !mouse_mode => {
                let g = [g[0] - gaze_neutral[0], g[1] - gaze_neutral[1]];
                let hit = tracking::gaze_hit(self.eye, g, gaze_gain, -self.room.depth);
                let d = [
                    hit[0] - self.light[0],
                    hit[1] - self.light[1],
                    hit[2] - self.light[2],
                ];
                let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-6);
                Some([d[0] / len, d[1] / len, d[2] / len])
            }
            // With the mouse driving, aim from the head through the window center.
            _ if flashlight => {
                let l = self.light;
                let len = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt().max(1e-6);
                Some([-l[0] / len, -l[1] / len, -l[2] / len])
            }
            _ => None,
        };
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
        let far = self.view[2] + depth + 1.0;
        let globals = Globals {
            view_proj: off_axis(self.view, half_w, half_h, 0.02, far),
            light_view_proj: off_axis(
                self.light,
                half_w * LIGHT_SPREAD,
                half_h * LIGHT_SPREAD,
                0.02,
                self.light[2] + depth + 1.0,
            ),
            eye: [self.view[0], self.view[1], self.view[2], 1.0],
            light: [self.light[0], self.light[1], self.light[2], 1.8],
            params: [
                self.room.grid(),
                0.05,
                self.renderer.shadow_texel(),
                depth * 0.004,
            ],
            room: [half_w, half_h, depth, self.light[2]],
            spot: self
                .spot
                .map_or([0.0, 0.0, -1.0, 0.0], |d| [d[0], d[1], d[2], 1.0]),
            cone: self.spot_cone(),
        };
        self.renderer.draw(&ctx.gfx, frame, &self.mesh, &globals);
    }
}

/// Milliseconds on the clock the page timestamps observations with (`performance.now()`).
#[cfg(target_arch = "wasm32")]
fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map_or(0.0, |p| p.now())
}

#[cfg(not(target_arch = "wasm32"))]
fn now_ms() -> f64 {
    use std::sync::OnceLock;
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_secs_f64()
        * 1000.0
}

impl WindowRoom {
    /// Cosines of the flashlight's outer and inner half-angles, sized so the beam covers a
    /// similar share of the back wall however far away the viewer sits.
    fn spot_cone(&self) -> [f32; 4] {
        let dist = (self.light[2] + self.room.depth).max(0.05);
        let outer = (SPOT_RADIUS * self.room.half_h * 2.0 / dist).atan();
        [outer.cos(), (outer * SPOT_CORE).cos(), 0.0, 0.0]
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

/// Mirror the view's parallax per axis (see `web::set_view_invert`).
pub fn set_view_invert(x: bool, y: bool) {
    with_shared(|s| s.view_invert = [x, y]);
}

/// Light the room with a spotlight instead of a lamp (see `web::set_flashlight`).
pub fn set_flashlight(on: bool) {
    with_shared(|s| s.flashlight = on);
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
