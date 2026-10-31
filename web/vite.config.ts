import { appendFileSync } from 'node:fs';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig, type Plugin } from 'vite';

/** Where `devLog` lines from the page end up, next to this config. Overwritten per server start. */
const LOG_FILE = 'dev-stats.log';

/**
 * Dev only: the page sends stats over Vite's HMR websocket (`import.meta.hot.send`), and the
 * server prints them and appends them to `LOG_FILE`, which is easier to copy from than the
 * browser.
 */
function devLog(): Plugin {
	return {
		name: 'face-window-dev-log',
		apply: 'serve',
		configureServer(server) {
			appendFileSync(LOG_FILE, `--- dev server started ${new Date().toISOString()}\n`, { flag: 'w' });
			server.ws.on('face-window:log', (data: { line: string }) => {
				const line = `${new Date().toISOString().slice(11, 23)} ${data.line}`;
				console.log(`[page] ${line}`);
				appendFileSync(LOG_FILE, line + '\n');
			});
		}
	};
}

export default defineConfig({
	plugins: [sveltekit(), devLog()],
	server: { port: 5173 },
	// The tracker worker is an ES module (MediaPipe's module build needs it).
	worker: { format: 'es' }
});
