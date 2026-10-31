<script lang="ts">
	import { onMount } from 'svelte';
	import { base } from '$app/paths';
	import init, * as room from '$lib/wasm/face_window.js';
	import { listCameras, loadLandmarker, openCamera, track, type Eyes } from '$lib/tracker';
	import { reset, save, settings } from '$lib/settings.svelte';
	import { devLog } from '$lib/devlog';
	import { CARD_MM, diagonalFromSpan, guessDiagonal, pitch, screenKey } from '$lib/screens';

	let canvas: HTMLCanvasElement;
	let video = $state<HTMLVideoElement>();
	let overlay = $state<HTMLCanvasElement>();

	let ready = $state(false);
	let panelOpen = $state(true);
	let status = $state('loading renderer…');
	let cameras = $state<MediaDeviceInfo[]>([]);
	let cameraOn = $state(false);
	let trackFps = $state(0);
	let detectMs = $state(0);
	let renderFps = $state(0);
	let delegate = $state('');
	/** `t` starts a few seconds of per-measurement logging, for tuning the filter. */
	let traceUntil = 0;
	let ageTotal = 0;
	let ageCount = 0;
	let head = $state([0, 0, 0, 0]);
	let windowCm = $state([0, 0]);
	let screenRes = $state('');
	let currentScreen = $state('');
	/** The current screen's diagonal: measured or entered, else guessed. */
	let diagonal = $derived(settings.screens[currentScreen] ?? guessDiagonal());
	let measured = $derived(currentScreen in settings.screens);
	/** Card-measuring overlay: the on-screen card width in CSS pixels, or 0 when closed. */
	let cardPx = $state(0);
	let distanceCm = $state(50);
	let calibrateNote = $state('');
	/** Implied pixel density, when it's implausible for the entered diagonal. */
	let dpiWarning = $state(0);

	let stopTracking: (() => void) | undefined;
	let stream: MediaStream | undefined;
	let landmarker: Awaited<ReturnType<typeof loadLandmarker>> | undefined;

	/** Push the canvas' physical size and the camera position (relative to it) to Rust. */
	function updateGeometry() {
		if (!ready || !canvas) return;
		// Meters per CSS pixel, from the diagonal and the screen's CSS resolution.
		currentScreen = screenKey();
		const mPerPx = pitch(diagonal);
		const r = canvas.getBoundingClientRect();
		// Canvas center in screen CSS pixels. Browser chrome sits above the viewport; in
		// fullscreen all of these offsets are zero.
		const s = screen as Screen & { availLeft?: number; availTop?: number };
		const chromeTop = window.outerHeight - window.innerHeight;
		const cx = window.screenX - (s.availLeft ?? 0) + r.left + r.width / 2;
		const cy = window.screenY - (s.availTop ?? 0) + chromeTop + r.top + r.height / 2;
		room.set_window_size(r.width * mPerPx, r.height * mPerPx);
		// The webcam sits centered above the screen's top edge.
		room.set_camera(
			settings.cameraFovDeg,
			(screen.width / 2 - cx) * mPerPx,
			cy * mPerPx + settings.cameraAboveCm / 100,
			0
		);
		room.set_ipd(settings.ipdMm / 1000);
		const [lx, ly, lz] = settings.lightCm;
		room.set_light_offset(lx / 100, ly / 100, lz / 100);
		room.set_mouse_mode(settings.mouseMode);
		room.set_view_follows(settings.viewFollows);
		room.set_view_invert(settings.flipX, settings.flipY);
		room.set_tracking(settings.smoothing, settings.predictionMs);
		const [nx, ny, ngx, ngy] = settings.neutral;
		room.set_neutral(nx, ny, ngx, ngy);
		room.set_flashlight(settings.lightMode === 'flashlight', settings.gazeGain);
		windowCm = [r.width * mPerPx * 100, r.height * mPerPx * 100];
		screenRes = `${screen.width}×${screen.height}`;
		// Device pixels per inch; desktop and laptop panels sit roughly between 90 and 300.
		const dpi = (Math.hypot(screen.width, screen.height) * devicePixelRatio) / diagonal;
		dpiWarning = dpi > 300 || dpi < 70 ? Math.round(dpi) : 0;
	}

	function drawEyes(e: Eyes | null) {
		const ctx = overlay?.getContext('2d');
		if (!overlay || !ctx) return;
		ctx.clearRect(0, 0, overlay.width, overlay.height);
		if (!e) return;
		overlay.width = e.width;
		overlay.height = e.height;
		ctx.fillStyle = '#4ef08a';
		for (const [x, y] of [e.left, e.right]) {
			ctx.beginPath();
			ctx.arc(x, y, Math.max(3, e.width / 160), 0, Math.PI * 2);
			ctx.fill();
		}
	}

	async function startCamera() {
		try {
			stopCamera();
			status = 'opening camera…';
			if (!video) return;
			stream = await openCamera(video, settings.cameraId || undefined);
			// Labels are only exposed once permission is granted.
			cameras = await listCameras();
			const active = stream.getVideoTracks()[0]?.getSettings().deviceId;
			if (active) settings.cameraId = active;
			save();
			status = 'loading face model…';
			if (landmarker && landmarker.delegate !== settings.detector) {
				landmarker.worker.terminate();
				landmarker = undefined;
			}
			landmarker ??= await loadLandmarker(base, settings.detector);
			delegate = landmarker.delegate;
			cameraOn = true;
			status = 'looking for a face…';
			stopTracking = track(video, landmarker, {
				onEyes: (e) => {
					const age = performance.now() - e.time;
					ageTotal += age;
					ageCount++;
					if (performance.now() < traceUntil) {
						const h = Array.from(room.head()).slice(0, 3).map(fmt).join(',');
						devLog(
							`m age=${age.toFixed(0)}ms L=${e.left.map((v) => v.toFixed(1))} ` +
								`R=${e.right.map((v) => v.toFixed(1))} gaze=${e.gaze.map((v) => v.toFixed(3))} head=${h}cm`
						);
					}
					room.push_eyes(
						e.left[0],
						e.left[1],
						e.right[0],
						e.right[1],
						e.width,
						e.height,
						e.gaze[0],
						e.gaze[1],
						e.time
					);
					if (settings.showVideo) drawEyes(e);
					status = 'tracking';
				},
				onLost: () => {
					room.face_lost();
					drawEyes(null);
					status = 'face lost';
				},
				onStats: (fps, ms) => {
					trackFps = fps;
					detectMs = ms;
				}
			});
		} catch (e) {
			status = `camera error: ${e instanceof Error ? e.message : e}`;
			stopCamera();
		}
	}

	function stopCamera() {
		stopTracking?.();
		stopTracking = undefined;
		stream?.getTracks().forEach((t) => t.stop());
		stream = undefined;
		cameraOn = false;
		trackFps = 0;
		detectMs = 0;
	}

	function openCard() {
		cardPx = CARD_MM.w / 1000 / pitch(diagonal);
	}

	function saveCard() {
		settings.screens[currentScreen] = Math.round(diagonalFromSpan(cardPx, CARD_MM.w) * 10) / 10;
		cardPx = 0;
	}

	function calibrateDistance() {
		const fov = room.calibrate_distance(distanceCm / 100);
		if (fov === undefined) {
			calibrateNote = 'No face tracked yet.';
			return;
		}
		settings.cameraFovDeg = Math.round(fov * 10) / 10;
		calibrateNote = `Camera FOV set to ${settings.cameraFovDeg}°. Measure the distance from your eyes to the screen.`;
	}

	function centerView() {
		const n = room.center_view();
		if (n) settings.neutral = [n[0], n[1], n[2], n[3]];
	}

	function fullscreen() {
		if (document.fullscreenElement) document.exitFullscreen();
		else document.documentElement.requestFullscreen();
	}

	function onKey(e: KeyboardEvent) {
		if (e.target instanceof HTMLInputElement || e.target instanceof HTMLSelectElement) return;
		if (e.key === 'h') panelOpen = !panelOpen;
		if (e.key === 'f') fullscreen();
		if (e.key === 'c' && cameraOn) centerView();
		if (e.key === 't') {
			traceUntil = performance.now() + 5000;
			devLog('trace: 5 s of measurements');
		}
	}

	onMount(() => {
		let disposed = false;
		init().then(
			() => {
				if (disposed) return;
				ready = true;
				status = 'renderer ready';
				// The adapter is picked asynchronously after init; report it once it is known.
				const report = setInterval(() => {
					const info = room.renderer_info();
					if (!info) return;
					clearInterval(report);
					devLog(`renderer: ${info}, dpr=${devicePixelRatio}`);
				}, 200);
				updateGeometry();
			},
			(e) => (status = `renderer failed: ${e}`)
		);
		listCameras().then((c) => (cameras = c));

		const ro = new ResizeObserver(updateGeometry);
		ro.observe(canvas);
		// Window moves don't fire events; re-check where the canvas sits now and then.
		const geo = setInterval(updateGeometry, 1000);
		// Count animation frames: the renderer draws once per frame, so this is its rate.
		let rafFrames = 0;
		let raf = requestAnimationFrame(function count() {
			rafFrames++;
			raf = requestAnimationFrame(count);
		});
		const fps = setInterval(() => {
			renderFps = rafFrames;
			rafFrames = 0;
			if (!ready) return;
			const [x, y, z, tracked] = Array.from(room.head());
			const age = ageCount ? ageTotal / ageCount : 0;
			ageTotal = ageCount = 0;
			const cam = cameraOn
				? ` track=${trackFps.toFixed(1)}fps detect=${detectMs.toFixed(1)}ms(${delegate}) age=${age.toFixed(0)}ms`
				: '';
			devLog(
				`render=${renderFps}fps@${canvas.width}x${canvas.height} ` +
					`window=${windowCm[0].toFixed(1)}x${windowCm[1].toFixed(1)}cm screen=${screenRes}@${diagonal}in${measured ? '' : '(guess)'}` +
					` smooth=${settings.smoothing} pred=${settings.predictionMs}ms light=${settings.lightMode}${cam} eye=${fmt(x)},${fmt(y)},${fmt(z)}cm ` +
					`${tracked > 0 ? 'tracked' : 'idle'} status="${status}"`
			);
		}, 1000);
		const poll = setInterval(() => {
			if (ready) head = Array.from(room.head());
		}, 100);
		return () => {
			disposed = true;
			ro.disconnect();
			clearInterval(geo);
			clearInterval(poll);
			clearInterval(fps);
			cancelAnimationFrame(raf);
			stopCamera();
		};
	});

	$effect(() => devLog(`status: ${status}`));


	// Re-send calibration whenever a setting changes.
	$effect(() => {
		JSON.stringify(settings);
		updateGeometry();
		save();
	});

	const fmt = (m: number) => (m * 100).toFixed(1);
</script>

<svelte:window onkeydown={onKey} />

<canvas id="room" bind:this={canvas} tabindex="0"></canvas>

<aside class:closed={!panelOpen}>
	<header>
		<strong>face-window</strong>
		<button class="ghost" onclick={() => (panelOpen = !panelOpen)} data-tip="Toggle panel (h)" aria-label="Toggle panel (h)">
			{panelOpen ? '–' : '+'}
		</button>
	</header>

	{#if panelOpen}
		<p class="status">{status}</p>
		<p class="perf">
			render {renderFps.toFixed(0)} fps{#if cameraOn} · track {trackFps.toFixed(0)} fps · detect {detectMs.toFixed(1)} ms ({delegate}){/if}
		</p>
		<p class="head">
			eye {fmt(head[0])}, {fmt(head[1])}, {fmt(head[2])} cm
			<span class:on={head[3] > 0}>{head[3] > 0 ? 'tracked' : 'idle'}</span>
		</p>

		<div class="row">
			{#if cameraOn}
				<button onclick={stopCamera}>Stop camera</button>
			{:else}
				<button onclick={startCamera} disabled={!ready}>Start camera</button>
			{/if}
			<button onclick={fullscreen} data-tip="Fullscreen (f)">Fullscreen</button>
		</div>

		{#if cameras.length > 1}
			<label>
				Camera
				<select bind:value={settings.cameraId} onchange={() => cameraOn && startCamera()}>
					{#each cameras as c, i (c.deviceId || i)}
						<option value={c.deviceId}>{c.label || `Camera ${i + 1}`}</option>
					{/each}
				</select>
			</label>
		{/if}

	{/if}

	<!-- Always mounted: tracking keeps running while the panel is collapsed. -->
	<div class="video" class:hidden={!panelOpen || !settings.showVideo || !cameraOn}>
		<video bind:this={video} playsinline muted></video>
		<canvas bind:this={overlay}></canvas>
	</div>

	{#if panelOpen}
		<fieldset>
			<legend>Calibration</legend>
			<div class="row">
				<button onclick={centerView} disabled={!cameraOn} data-tip="Sit as usual, then click (c)">Center view here</button>
				<button class="ghost" onclick={() => (settings.neutral = [0, 0, 0, 0])}>Physical</button>
			</div>
			<p class="hint">
				Center: {fmt(settings.neutral[0])}, {fmt(settings.neutral[1])} cm. "Physical" makes the screen center
				the straight-on view, which is exact but means looking down from above a laptop screen.
			</p>
			<label>
				Screen diagonal (in)
				<input
					type="number"
					step="0.1"
					value={diagonal}
					onchange={(e) => (settings.screens[currentScreen] = +e.currentTarget.value)}
				/>
			</label>
			<div class="row">
				<button onclick={openCard} data-tip="Match a bank card against the screen">Measure with a card</button>
				{#if !measured}<span class="hint">guessed</span>{/if}
			</div>
			{#if dpiWarning}
				<p class="warn">
					{screenRes} at {diagonal}" would be {dpiWarning} px per inch, which is unlikely.
					Enter this screen's real diagonal.
				</p>
			{/if}
			<p class="hint">
				Diagonal of the screen this window is on ({screenRes}). Canvas ≈ {windowCm[0].toFixed(1)} ×
				{windowCm[1].toFixed(1)} cm; if that's off, the room swings too much or too little.
			</p>
			<label>Camera FOV (°) <input type="number" step="0.5" bind:value={settings.cameraFovDeg} /></label>
			<div class="row">
				<label>I sit <input type="number" step="1" bind:value={distanceCm} /> cm away</label>
				<button onclick={calibrateDistance} disabled={!cameraOn} data-tip="Sets the camera FOV">Calibrate</button>
			</div>
			{#if calibrateNote}<p class="hint">{calibrateNote}</p>{/if}
			<label>Camera above screen (cm) <input type="number" step="0.5" bind:value={settings.cameraAboveCm} /></label>
			<label>Eye distance (mm) <input type="number" step="1" bind:value={settings.ipdMm} /></label>
		</fieldset>

		<fieldset>
			<legend>Light offset from eyes (cm)</legend>
			<div class="xyz">
				<label>x <input type="number" step="1" bind:value={settings.lightCm[0]} /></label>
				<label>y <input type="number" step="1" bind:value={settings.lightCm[1]} /></label>
				<label>z <input type="number" step="1" bind:value={settings.lightCm[2]} /></label>
			</div>
			<p class="hint">At 0, 0, 0 every shadow hides behind its object.</p>
		</fieldset>

		<div class="row">
			<label class="check"><input type="checkbox" bind:checked={settings.flipX} /> Invert view left/right</label>
			<label class="check"><input type="checkbox" bind:checked={settings.flipY} /> Invert view up/down</label>
		</div>
		<label>
			Face model runs on
			<select bind:value={settings.detector} onchange={() => cameraOn && startCamera()}>
				<option value="CPU">CPU (WASM)</option>
				<option value="GPU">GPU (WebGL)</option>
			</select>
		</label>
		<fieldset>
			<legend>Tracking feel</legend>
			<label>
				Smoothing {settings.smoothing.toFixed(2)}
				<input type="range" min="0" max="1" step="0.05" bind:value={settings.smoothing} />
			</label>
			<label>
				Prediction {settings.predictionMs} ms
				<input type="range" min="0" max="200" step="10" bind:value={settings.predictionMs} />
			</label>
			<p class="hint">Jittery: more smoothing, less prediction. Laggy: the other way round.</p>
		</fieldset>
		<label>
			View
			<select bind:value={settings.viewFollows}>
				<option value={true}>Window: room follows your head</option>
				<option value={false}>Fixed room: only the light moves</option>
			</select>
		</label>
		<label>
			Light
			<select bind:value={settings.lightMode}>
				<option value="lamp">Lamp at your head</option>
				<option value="flashlight">Flashlight where you look</option>
			</select>
		</label>
		{#if settings.lightMode === 'flashlight'}
			<label>
				Gaze sensitivity {settings.gazeGain.toFixed(1)}
				<input type="range" min="0.3" max="3" step="0.1" bind:value={settings.gazeGain} />
			</label>
			<p class="hint">Look at the screen center and press c to calibrate where "straight ahead" is.</p>
		{/if}
		<label class="check"><input type="checkbox" bind:checked={settings.mouseMode} /> Mouse drives the head (wheel: distance)</label>
		<label class="check"><input type="checkbox" bind:checked={settings.showVideo} /> Show camera preview</label>
		<div class="row">
			<button class="ghost" onclick={reset}>Reset settings</button>
		</div>
		<p class="hint">h hides this panel, f toggles fullscreen, c centers the view, t logs 5 s of raw tracking.</p>
	{/if}
</aside>

{#if cardPx > 0}
	<div class="card-overlay">
		<p>
			Hold a bank or ID card flat against the screen and resize the outline until it matches the
			card's edges. Arrow keys fine-tune.
		</p>
		<div class="card" style:width="{cardPx}px" style:height="{(cardPx * CARD_MM.h) / CARD_MM.w}px"></div>
		<input
			type="range"
			min="80"
			max="1200"
			step="0.5"
			bind:value={cardPx}
			style:width="min(600px, 80vw)"
		/>
		<p class="hint">≈ {Math.round(diagonalFromSpan(cardPx, CARD_MM.w) * 10) / 10}" diagonal for this screen</p>
		<div class="row">
			<button onclick={saveCard}>Save</button>
			<button class="ghost" onclick={() => (cardPx = 0)}>Cancel</button>
		</div>
	</div>
{/if}

<style>
	:global(html, body) {
		margin: 0;
		height: 100%;
		overflow: hidden;
		background: #000;
		font: 13px/1.4 system-ui, sans-serif;
		color: #e8e8ec;
	}

	/* winit writes an inline pixel size onto the canvas; the room must fill the viewport. */
	#room {
		position: fixed;
		inset: 0;
		width: 100vw !important;
		height: 100vh !important;
		display: block;
		outline: none;
	}

	aside {
		position: fixed;
		top: 12px;
		left: 12px;
		width: 280px;
		max-height: calc(100vh - 24px);
		overflow-y: auto;
		padding: 10px 12px;
		border-radius: 10px;
		background: rgb(18 18 24 / 0.82);
		backdrop-filter: blur(6px);
		box-shadow: 0 4px 24px rgb(0 0 0 / 0.4);
	}

	aside.closed {
		width: auto;
	}

	header {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 12px;
	}

	.status {
		margin: 6px 0 2px;
		color: #b8b8c4;
	}

	.perf {
		margin: 0 0 2px;
		color: #8a8a96;
		font-size: 11px;
		font-variant-numeric: tabular-nums;
	}

	.head {
		margin: 0 0 8px;
		font-variant-numeric: tabular-nums;
	}

	.head span {
		margin-left: 6px;
		padding: 0 6px;
		border-radius: 8px;
		background: #444;
		font-size: 11px;
	}

	.head span.on {
		background: #2c7a4b;
	}

	.row {
		display: flex;
		gap: 6px;
		margin: 6px 0;
	}

	button {
		padding: 5px 10px;
		border: 0;
		border-radius: 6px;
		background: #3b5bdb;
		color: #fff;
		font: inherit;
		cursor: pointer;
	}

	/* Our own tooltips: Chrome on X11 places native `title` tooltips at the screen's top left
	   until the mouse moves. */
	button[data-tip] {
		position: relative;
	}

	button[data-tip]::after {
		content: attr(data-tip);
		position: absolute;
		top: calc(100% + 6px);
		left: 50%;
		transform: translateX(-50%);
		z-index: 10;
		padding: 3px 7px;
		border-radius: 4px;
		background: rgb(0 0 0 / 0.85);
		color: #e8e8ec;
		font-size: 11px;
		white-space: nowrap;
		pointer-events: none;
		opacity: 0;
		transition: opacity 0.1s;
	}

	/* The panel toggle sits at the right edge, where a centered tip would be clipped. */
	header button[data-tip]::after {
		left: auto;
		right: 0;
		transform: none;
	}

	button[data-tip]:hover::after {
		opacity: 1;
		transition-delay: 0.4s;
	}

	button:disabled {
		opacity: 0.5;
		cursor: default;
	}

	button.ghost {
		background: transparent;
		color: #b8b8c4;
		padding: 2px 6px;
	}

	label {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 8px;
		margin: 4px 0;
	}

	label.check {
		justify-content: flex-start;
	}

	input[type='range'] {
		width: 110px;
	}

	input[type='number'] {
		width: 64px;
	}

	input,
	select {
		background: #2a2a33;
		color: inherit;
		border: 1px solid #444;
		border-radius: 4px;
		padding: 2px 4px;
		font: inherit;
	}

	select {
		max-width: 170px;
	}

	fieldset {
		margin: 8px 0;
		border: 1px solid #3a3a44;
		border-radius: 6px;
		padding: 4px 8px 6px;
	}

	legend {
		color: #b8b8c4;
	}

	.xyz {
		display: flex;
		gap: 6px;
	}

	.xyz input {
		width: 44px;
	}

	.card-overlay {
		position: fixed;
		inset: 0;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 12px;
		background: rgb(10 10 14 / 0.92);
		text-align: center;
	}

	.card-overlay p {
		max-width: 520px;
		margin: 0;
	}

	.card {
		border: 2px solid #4ef08a;
		border-radius: 3.2%/5%;
		box-sizing: border-box;
	}

	.warn {
		margin: 2px 0;
		color: #ffb35c;
		font-size: 11px;
	}

	.hint {
		margin: 2px 0;
		color: #8a8a96;
		font-size: 11px;
	}

	.video {
		position: relative;
		margin: 6px 0;
		/* Mirrored like a selfie view; the tracker itself works on the raw frame. */
		transform: scaleX(-1);
	}

	.video.hidden {
		display: none;
	}

	.video video,
	.video canvas {
		width: 100%;
		display: block;
		border-radius: 6px;
	}

	.video canvas {
		position: absolute;
		inset: 0;
		height: 100%;
	}
</style>
