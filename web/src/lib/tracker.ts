// Webcam + MediaPipe Face Landmarker. Reports eye centers in pixels of the raw (unmirrored)
// camera frame; turning them into a head position is the Rust side's job.
import { FaceLandmarker, FilesetResolver } from '@mediapipe/tasks-vision';

// Iris centers in the 478-point face mesh.
const LEFT_IRIS = 468;
const RIGHT_IRIS = 473;

export type Eyes = {
	left: [number, number];
	right: [number, number];
	width: number;
	height: number;
	time: number;
};

export type TrackerEvents = {
	onEyes: (eyes: Eyes) => void;
	onLost: () => void;
	onFps?: (fps: number) => void;
};

/** Load the landmarker from the vendored assets under `base` (the app's base path). */
export async function loadLandmarker(base: string): Promise<FaceLandmarker> {
	const fileset = await FilesetResolver.forVisionTasks(`${base}/mediapipe/wasm`);
	const options = (delegate: 'GPU' | 'CPU') => ({
		baseOptions: { modelAssetPath: `${base}/mediapipe/face_landmarker.task`, delegate },
		runningMode: 'VIDEO' as const,
		numFaces: 1
	});
	try {
		return await FaceLandmarker.createFromOptions(fileset, options('GPU'));
	} catch (e) {
		console.warn('GPU delegate unavailable, using CPU', e);
		return await FaceLandmarker.createFromOptions(fileset, options('CPU'));
	}
}

export async function listCameras(): Promise<MediaDeviceInfo[]> {
	const devices = await navigator.mediaDevices.enumerateDevices();
	return devices.filter((d) => d.kind === 'videoinput');
}

export async function openCamera(video: HTMLVideoElement, deviceId?: string): Promise<MediaStream> {
	const stream = await navigator.mediaDevices.getUserMedia({
		video: {
			deviceId: deviceId ? { exact: deviceId } : undefined,
			width: { ideal: 1280 },
			height: { ideal: 720 },
			frameRate: { ideal: 60 }
		},
		audio: false
	});
	video.srcObject = stream;
	await video.play();
	return stream;
}

/** Run the landmarker on every new video frame until the returned function is called. */
export function track(video: HTMLVideoElement, landmarker: FaceLandmarker, ev: TrackerEvents) {
	let running = true;
	let lastVideoTime = -1;
	let hadFace = false;
	let frames = 0;
	let fpsStart = performance.now();

	const step = () => {
		if (!running) return;
		if (video.readyState >= 2 && video.currentTime !== lastVideoTime) {
			lastVideoTime = video.currentTime;
			const now = performance.now();
			const result = landmarker.detectForVideo(video, now);
			const lm = result.faceLandmarks[0];
			if (lm && lm.length > RIGHT_IRIS) {
				const w = video.videoWidth;
				const h = video.videoHeight;
				ev.onEyes({
					left: [lm[LEFT_IRIS].x * w, lm[LEFT_IRIS].y * h],
					right: [lm[RIGHT_IRIS].x * w, lm[RIGHT_IRIS].y * h],
					width: w,
					height: h,
					time: now
				});
				hadFace = true;
			} else if (hadFace) {
				ev.onLost();
				hadFace = false;
			}
			frames++;
			if (now - fpsStart >= 1000) {
				ev.onFps?.((frames * 1000) / (now - fpsStart));
				frames = 0;
				fpsStart = now;
			}
		}
		requestAnimationFrame(step);
	};
	requestAnimationFrame(step);
	return () => {
		running = false;
	};
}
