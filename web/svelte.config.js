import adapter from '@sveltejs/adapter-static';

/** @type {import('@sveltejs/kit').Config} */
const config = {
	compilerOptions: { runes: true },
	kit: {
		// A static SPA: everything runs in the browser, so the build can be served from any
		// static host (or opened offline once the MediaPipe assets are vendored).
		adapter: adapter(),
		paths: { relative: true }
	}
};

export default config;
