// Calibration and display settings, remembered per browser.

export type Settings = {
	/** Screen diagonal in inches. */
	diagonalIn: number;
	/** Webcam horizontal field of view in degrees. */
	cameraFovDeg: number;
	/** Webcam lens distance above the top edge of the screen, in cm. */
	cameraAboveCm: number;
	/** Interpupillary distance in mm. */
	ipdMm: number;
	/** Light position relative to the eyes, in cm (x right, y up, z toward you). */
	lightCm: [number, number, number];
	mouseMode: boolean;
	showVideo: boolean;
	cameraId: string;
};

const KEY = 'face-window.settings';

const DEFAULTS: Settings = {
	diagonalIn: 15.6,
	cameraFovDeg: 60,
	cameraAboveCm: 1,
	ipdMm: 63,
	lightCm: [0, 8, 0],
	mouseMode: false,
	showVideo: true,
	cameraId: ''
};

function load(): Settings {
	try {
		const raw = localStorage.getItem(KEY);
		if (raw) return { ...DEFAULTS, ...JSON.parse(raw) };
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
