// Vendor MediaPipe's WASM runtime and the face landmarker model into static/, so the page
// needs no CDN and keeps working offline. Safe to re-run; the model downloads only once.
import { cp, mkdir, stat, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const out = join(root, 'static', 'mediapipe');
const MODEL_URL =
	'https://storage.googleapis.com/mediapipe-models/face_landmarker/face_landmarker/float16/1/face_landmarker.task';

await mkdir(out, { recursive: true });
await cp(join(root, 'node_modules', '@mediapipe', 'tasks-vision', 'wasm'), join(out, 'wasm'), {
	recursive: true
});
console.log('copied MediaPipe wasm runtime');

const model = join(out, 'face_landmarker.task');
const exists = await stat(model).then(
	() => true,
	() => false
);
if (exists) {
	console.log('model already present');
} else {
	const res = await fetch(MODEL_URL);
	if (!res.ok) throw new Error(`model download failed: ${res.status} ${res.statusText}`);
	await writeFile(model, new Uint8Array(await res.arrayBuffer()));
	console.log('downloaded face_landmarker.task');
}
