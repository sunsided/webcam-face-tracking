# face-window

A 3D room behind your screen. A webcam tracks your face, and the room is re-projected from
where your eyes are, so the screen behaves like a window you look through. A light next to
your head throws the objects' shadows onto the walls.

The renderer is Rust on [game-toolkit](https://github.com/sunsided/game-toolkit) (wgpu), built
to WASM; the page around it is a small SvelteKit app that owns the webcam and runs
MediaPipe's Face Landmarker.

## Run it

```sh
task dev     # build the WASM renderer, install + vendor MediaPipe, serve on :5173
task run     # native window; the mouse stands in for your head, the wheel for distance
task build   # static site in web/build (works offline, no CDN)
task test    # projection, tracking and scene tests
task lint    # clippy (native + wasm32) and svelte-check
```

Needs `wasm-pack`, go-task, and Node with corepack (yarn 4 is pinned in `web/package.json`).

In the page: **Start camera**, then **Fullscreen** (`f`). `h` hides the panel. Without a
camera, tick *Mouse drives the head*.

## Calibration

The illusion holds when the numbers match your hardware:

- **Screen diagonal**: the page derives the canvas' physical size from it and the screen's
  CSS resolution. Run fullscreen, or the window position is estimated from browser offsets.
- **Camera FOV**: horizontal field of view of the webcam (laptops: 55 to 70°). If leaning
  in or out makes the room breathe, adjust this first.
- **Camera above screen**: lens distance above the top edge of the screen.
- **Eye distance**: depth is estimated from it; 63 mm is the adult average.
- **Light offset**: where the light sits relative to your eyes. A light exactly at your eyes
  hides every shadow behind the object casting it, so it defaults to 8 cm above.

## How it works

- `src/projection.rs`: off-axis projection. The canvas is a window in the `z = 0` plane
  (meters, `+z` toward you); the frustum runs from your eye through its edges.
- `src/tracking.rs`: eye centers in camera pixels to an eye point (pinhole model, depth from
  eye distance), smoothed with a One Euro filter.
- `src/scene.rs`: the room and its objects, rebuilt on the CPU every frame.
- `src/renderer.rs`, `src/room.wgsl`: a shadow-map pass from the light (its frustum also goes
  through the window, widened), then the lit scene with 3x3 PCF. WebGPU where available,
  WebGL2 otherwise.
- `src/web.rs`: the JavaScript API (`push_eyes`, `set_window_size`, `set_camera`, ...).
- `web/`: SvelteKit page, webcam + Face Landmarker (`src/lib/tracker.ts`), calibration panel.
  `yarn mediapipe` vendors the MediaPipe runtime and model into `web/static/mediapipe`.
