// Messages between the page and the face-tracking worker.

export type Eyes = {
	left: [number, number];
	right: [number, number];
	width: number;
	height: number;
	/** Iris offset from the eye centers, in eye widths, image orientation (x right, y down). */
	gaze: [number, number];
	/** Capture time of the frame, on the page's `performance.now()` clock. */
	time: number;
};

export type ToWorker =
	| { type: 'load'; base: string; delegate: 'GPU' | 'CPU' }
	| { type: 'frame'; frame: ImageBitmap; time: number };

export type FromWorker =
	| { type: 'ready'; delegate: 'GPU' | 'CPU' }
	| { type: 'eyes'; eyes: Eyes; detectMs: number }
	| { type: 'none'; detectMs: number }
	| { type: 'error'; message: string };
