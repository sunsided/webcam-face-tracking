// Face Landmarker off the main thread. A detection takes tens of milliseconds; on the main
// thread that time comes straight out of the renderer's frame budget.
import { FaceLandmarker, FilesetResolver } from '@mediapipe/tasks-vision';

import type { FromWorker, ToWorker } from './tracker-protocol';

// Eye corners in the 478-point face mesh. The midpoint of each eye's corners sits fixed on
// the skull, unlike the iris, which follows your gaze; their spacing still matches the
// interpupillary distance closely.
const LEFT_EYE = [33, 133];
const RIGHT_EYE = [362, 263];
const LEFT_IRIS = 468;
const RIGHT_IRIS = 473;
const LAST_LANDMARK = RIGHT_IRIS;

type Point = { x: number; y: number };

/**
 * Iris offset from the middle of the eye corners, in eye widths, along the eye's axis (x) and
 * across it (y, toward the image bottom). Pixel aspect matters, so points come scaled.
 */
function irisOffset(a: Point, b: Point, iris: Point): [number, number] {
	let ux = b.x - a.x;
	let uy = b.y - a.y;
	const width = Math.hypot(ux, uy) || 1;
	// Point the axis to image-right whichever corner comes first.
	if (ux < 0) {
		ux = -ux;
		uy = -uy;
	}
	ux /= width;
	uy /= width;
	const dx = iris.x - (a.x + b.x) / 2;
	const dy = iris.y - (a.y + b.y) / 2;
	return [(dx * ux + dy * uy) / width, (-dx * uy + dy * ux) / width];
}

let landmarker: FaceLandmarker | undefined;
let lastTime = -1;

const post = (msg: FromWorker) => postMessage(msg);

async function load(base: string, delegate: 'GPU' | 'CPU') {
	// `useModule`: this is a module worker, which has no importScripts for the classic build.
	const fileset = await FilesetResolver.forVisionTasks(`${base}/mediapipe/wasm`, true);
	const options = (delegate: 'GPU' | 'CPU') => ({
		baseOptions: { modelAssetPath: `${base}/mediapipe/face_landmarker.task`, delegate },
		runningMode: 'VIDEO' as const,
		numFaces: 1
	});
	if (delegate === 'GPU') {
		try {
			landmarker = await FaceLandmarker.createFromOptions(fileset, options('GPU'));
			post({ type: 'ready', delegate: 'GPU' });
			return;
		} catch (e) {
			console.warn('GPU delegate unavailable, using CPU', e);
		}
	}
	landmarker = await FaceLandmarker.createFromOptions(fileset, options('CPU'));
	post({ type: 'ready', delegate: 'CPU' });
}

function detect(frame: ImageBitmap, time: number) {
	try {
		if (!landmarker) return;
		// MediaPipe requires strictly increasing timestamps.
		const ts = Math.max(time, lastTime + 0.001);
		lastTime = ts;
		const start = performance.now();
		const result = landmarker.detectForVideo(frame, ts);
		const detectMs = performance.now() - start;
		const lm = result.faceLandmarks[0];
		if (lm && lm.length > LAST_LANDMARK) {
			const w = frame.width;
			const h = frame.height;
			const center = ([a, b]: number[]): [number, number] => [
				((lm[a].x + lm[b].x) / 2) * w,
				((lm[a].y + lm[b].y) / 2) * h
			];
			const px = (i: number): Point => ({ x: lm[i].x * w, y: lm[i].y * h });
			const gl = irisOffset(px(LEFT_EYE[0]), px(LEFT_EYE[1]), px(LEFT_IRIS));
			const gr = irisOffset(px(RIGHT_EYE[0]), px(RIGHT_EYE[1]), px(RIGHT_IRIS));
			post({
				type: 'eyes',
				eyes: {
					left: center(LEFT_EYE),
					right: center(RIGHT_EYE),
					gaze: [(gl[0] + gr[0]) / 2, (gl[1] + gr[1]) / 2],
					width: w,
					height: h,
					time
				},
				detectMs
			});
		} else {
			post({ type: 'none', detectMs });
		}
	} finally {
		frame.close();
	}
}

onmessage = (e: MessageEvent<ToWorker>) => {
	const msg = e.data;
	if (msg.type === 'load') {
		load(msg.base, msg.delegate).catch((err) => post({ type: 'error', message: String(err) }));
	} else if (msg.type === 'frame') {
		detect(msg.frame, msg.time);
	}
};
