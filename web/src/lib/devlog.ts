// Send a line to the Vite dev server's terminal and dev-stats.log. A no-op in builds.
export function devLog(line: string) {
	import.meta.hot?.send('face-window:log', { line });
}
