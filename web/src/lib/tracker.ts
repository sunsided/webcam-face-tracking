// Webcam capture on the page, Face Landmarker in a worker. Reports eye centers in pixels of
// the raw (unmirrored) camera frame; turning them into a head position is the Rust side's job.
import type { Eyes, FromWorker, ToWorker } from './tracker-protocol';

export type { Eyes };

export type TrackerEvents = {
	onEyes: (eyes: Eyes) => void;
	onLost: () => void;
	/** Once a second: detections per second and the mean time one detection takes. */
	onStats?: (fps: number, detectMs: number) => void;
};

export type Landmarker = { worker: Worker; delegate: 'GPU' | 'CPU' };

/** Start the worker and load the landmarker from the vendored assets under `base`. */
export function loadLandmarker(base: string, delegate: 'GPU' | 'CPU'): Promise<Landmarker> {
	const worker = new Worker(new URL('./tracker.worker.ts', import.meta.url), { type: 'module' });
	return new Promise((resolve, reject) => {
		worker.onmessage = (e: MessageEvent<FromWorker>) => {
			if (e.data.type === 'ready') resolve({ worker, delegate: e.data.delegate });
			else if (e.data.type === 'error') reject(new Error(e.data.message));
		};
		worker.onerror = (e) => reject(new Error(e.message));
		// Resolve the base against the page: the worker's own URL is elsewhere.
		const abs = new URL(base || '.', location.href).href.replace(/\/$/, '');
		worker.postMessage({ type: 'load', base: abs, delegate } satisfies ToWorker);
	});
}

export async function listCameras(): Promise<MediaDeviceInfo[]> {
	const devices = await navigator.mediaDevices.enumerateDevices();
	return devices.filter((d) => d.kind === 'videoinput');
}

export async function openCamera(video: HTMLVideoElement, deviceId?: string): Promise<MediaStream> {
	const stream = await navigator.mediaDevices.getUserMedia({
		video: {
			deviceId: deviceId ? { exact: deviceId } : undefined,
			// The landmarker scales frames down to 256 px anyway; bigger frames only cost
			// conversion time on the main thread.
			width: { ideal: 640 },
			height: { ideal: 480 },
			frameRate: { ideal: 60 }
		},
		audio: false
	});
	video.srcObject = stream;
	await video.play();
	return stream;
}

/**
 * Send camera frames to the worker until the returned function is called. A frame is only
 * sent while the worker is idle, so a slow detector drops frames instead of queueing them.
 */
export function track(video: HTMLVideoElement, lm: Landmarker, ev: TrackerEvents) {
	let running = true;
	let busy = false;
	let hadFace = false;
	let frames = 0;
	let detectTotal = 0;
	let statsStart = performance.now();

	const done = (detectMs: number) => {
		busy = false;
		frames++;
		detectTotal += detectMs;
		const now = performance.now();
		if (now - statsStart >= 1000) {
			ev.onStats?.((frames * 1000) / (now - statsStart), detectTotal / Math.max(frames, 1));
			frames = 0;
			detectTotal = 0;
			statsStart = now;
		}
	};

	lm.worker.onmessage = (e: MessageEvent<FromWorker>) => {
		if (!running) return;
		const msg = e.data;
		if (msg.type === 'eyes') {
			done(msg.detectMs);
			ev.onEyes(msg.eyes);
			hadFace = true;
		} else if (msg.type === 'none') {
			done(msg.detectMs);
			if (hadFace) ev.onLost();
			hadFace = false;
		} else if (msg.type === 'error') {
			busy = false;
			console.error('tracker worker:', msg.message);
		}
	};

	// Once per camera frame, stamped with its capture time where the browser knows it: that
	// is what the renderer's prediction extrapolates from.
	const onFrame = async (now: number, meta: VideoFrameCallbackMetadata) => {
		if (!running) return;
		video.requestVideoFrameCallback(onFrame);
		if (busy) return;
		busy = true;
		try {
			const frame = await createImageBitmap(video);
			const msg: ToWorker = { type: 'frame', frame, time: meta.captureTime ?? now };
			lm.worker.postMessage(msg, [frame]);
		} catch {
			busy = false;
		}
	};
	video.requestVideoFrameCallback(onFrame);
	return () => {
		running = false;
	};
}
