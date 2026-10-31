// Physical screen size: browsers don't expose it, so it's measured (bank card) or entered
// per screen, with a guess from common panel sizes until then.

/** ISO/IEC 7810 ID-1: bank and ID cards. */
export const CARD_MM = { w: 85.6, h: 53.98 };

/** Identifies the screen the window is on. */
export function screenKey(): string {
	return `${screen.width}x${screen.height}@${devicePixelRatio}`;
}

/** Common diagonals by device resolution, in inches. */
const COMMON: Record<string, number> = {
	'5120x1440': 49,
	'3840x1080': 49,
	'3840x1600': 38,
	'3440x1440': 34,
	'2560x1080': 29,
	'3840x2160': 27,
	'2560x1440': 27,
	'1920x1080': 24
};

/** Best guess for the current screen's diagonal in inches. */
export function guessDiagonal(): number {
	const w = Math.round(screen.width * devicePixelRatio);
	const h = Math.round(screen.height * devicePixelRatio);
	// About 110 device pixels per inch is typical for desktop monitors.
	return COMMON[`${w}x${h}`] ?? Math.round((Math.hypot(w, h) / 110) * 10) / 10;
}

/** Meters per CSS pixel for a diagonal in inches. */
export function pitch(diagonalIn: number): number {
	return (diagonalIn * 0.0254) / Math.hypot(screen.width, screen.height);
}

/** The diagonal in inches that makes `px` CSS pixels span `mm` millimeters. */
export function diagonalFromSpan(px: number, mm: number): number {
	return ((mm / 1000 / px) * Math.hypot(screen.width, screen.height)) / 0.0254;
}
