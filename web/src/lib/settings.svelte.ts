// Calibration and display settings, remembered per browser.

export type Settings = {
	/** Screen diagonal in inches, per screen (`<width>x<height>@<dpr>`), measured or entered. */
	screens: Record<string, number>;
	/** Webcam horizontal field of view in degrees. */
	cameraFovDeg: number;
	/** Webcam lens distance above the top edge of the screen, in cm. */
	cameraAboveCm: number;
	/** Interpupillary distance in mm. */
	ipdMm: number;
	/** Light position relative to the eyes, in cm (x right, y up, z toward you). */
	lightCm: [number, number, number];
	mouseMode: boolean;
	/** Re-project the room from the eye; off moves only the light. */
	viewFollows: boolean;
	/** 0 = raw and jittery, 1 = smooth and laggy. */
	smoothing: number;
	/** How far the head may be extrapolated past the last measurement, in ms. */
	predictionMs: number;
	/** Mirror the view's parallax (not the light). On Markus's setup the mirrored view felt right. */
	flipX: boolean;
	flipY: boolean;
	/** Straight-on eye position (m) and straight-ahead gaze, from "Center view here". */
	neutral: [number, number, number, number];
	/** Lamp next to the head, or a flashlight that points where you look. */
	lightMode: 'lamp' | 'flashlight';
	/** Multiplier on the gaze angle estimate. */
	gazeGain: number;
	/** Where MediaPipe runs its model: WebGL in the worker, or WASM SIMD on the CPU. */
	detector: 'GPU' | 'CPU';
	showVideo: boolean;
	cameraId: string;
	version: number;
};

const KEY = 'face-window.settings';
/** Bumped when a default changes in a way stored settings should pick up. */
const VERSION = 2;

const DEFAULTS: Settings = {
	screens: {},
	cameraFovDeg: 60,
	cameraAboveCm: 1,
	ipdMm: 63,
	lightCm: [0, 8, 0],
	mouseMode: false,
	viewFollows: true,
	smoothing: 0.5,
	predictionMs: 80,
	// On by default: on Markus's setup the unflipped tracking felt inverted on both axes.
	flipX: true,
	flipY: true,
	neutral: [0, 0, 0, 0],
	lightMode: 'lamp',
	gazeGain: 1,
	detector: 'CPU',
	showVideo: true,
	cameraId: '',
	version: VERSION
};

function load(): Settings {
	try {
		const raw = localStorage.getItem(KEY);
		if (raw) {
			const stored = JSON.parse(raw);
			if ((stored.version ?? 1) < 2) {
				// v2 flipped both axes by default.
				stored.flipX = true;
				stored.flipY = true;
			}
			// Before per-screen sizes there was one diagonal for every screen; drop it, it was
			// usually the laptop default applied to a monitor.
			delete stored.diagonalIn;
			// Before the gaze, the neutral had only the eye position.
			if (Array.isArray(stored.neutral) && stored.neutral.length === 2) stored.neutral.push(0, 0);
			return { ...DEFAULTS, ...stored, version: VERSION };
		}
	} catch {
		/* storage unavailable or corrupt: use defaults */
	}
	return structuredClone(DEFAULTS);
}

export const settings: Settings = $state(load());

export function save() {
	try {
		localStorage.setItem(KEY, JSON.stringify(settings));
	} catch {
		/* storage unavailable */
	}
}

export function reset() {
	Object.assign(settings, structuredClone(DEFAULTS));
	save();
}
