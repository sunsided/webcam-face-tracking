<script lang="ts">
	import { onMount } from 'svelte';
	import { base } from '$app/paths';
	import init, * as room from '$lib/wasm/face_window.js';
	import { listCameras, loadLandmarker, openCamera, track, type Eyes } from '$lib/tracker';
	import { reset, save, settings } from '$lib/settings.svelte';

	let canvas: HTMLCanvasElement;
	let video = $state<HTMLVideoElement>();
	let overlay = $state<HTMLCanvasElement>();

	let ready = $state(false);
	let panelOpen = $state(true);
	let status = $state('loading renderer…');
	let cameras = $state<MediaDeviceInfo[]>([]);
	let cameraOn = $state(false);
	let trackFps = $state(0);
	let head = $state([0, 0, 0, 0]);
	let windowCm = $state([0, 0]);

	let stopTracking: (() => void) | undefined;
	let stream: MediaStream | undefined;
	let landmarker: Awaited<ReturnType<typeof loadLandmarker>> | undefined;

	/** Push the canvas' physical size and the camera position (relative to it) to Rust. */
	function updateGeometry() {
		if (!ready || !canvas) return;
		// Meters per CSS pixel, from the diagonal and the screen's CSS resolution.
		const pitch = (settings.diagonalIn * 0.0254) / Math.hypot(screen.width, screen.height);
		const r = canvas.getBoundingClientRect();
		// Canvas center in screen CSS pixels. Browser chrome sits above the viewport; in
		// fullscreen all of these offsets are zero.
		const s = screen as Screen & { availLeft?: number; availTop?: number };
		const chromeTop = window.outerHeight - window.innerHeight;
		const cx = window.screenX - (s.availLeft ?? 0) + r.left + r.width / 2;
		const cy = window.screenY - (s.availTop ?? 0) + chromeTop + r.top + r.height / 2;
		room.set_window_size(r.width * pitch, r.height * pitch);
		// The webcam sits centered above the screen's top edge.
		room.set_camera(
			settings.cameraFovDeg,
			(screen.width / 2 - cx) * pitch,
			cy * pitch + settings.cameraAboveCm / 100,
			0
		);
		room.set_ipd(settings.ipdMm / 1000);
		const [lx, ly, lz] = settings.lightCm;
		room.set_light_offset(lx / 100, ly / 100, lz / 100);
		room.set_mouse_mode(settings.mouseMode);
		windowCm = [r.width * pitch * 100, r.height * pitch * 100];
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
			landmarker ??= await loadLandmarker(base);
			cameraOn = true;
			status = 'looking for a face…';
			stopTracking = track(video, landmarker, {
				onEyes: (e) => {
					room.push_eyes(e.left[0], e.left[1], e.right[0], e.right[1], e.width, e.height, e.time);
					if (settings.showVideo) drawEyes(e);
					status = 'tracking';
				},
				onLost: () => {
					room.face_lost();
					drawEyes(null);
					status = 'face lost';
				},
				onFps: (fps) => (trackFps = fps)
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
	}

	function fullscreen() {
		if (document.fullscreenElement) document.exitFullscreen();
		else document.documentElement.requestFullscreen();
	}

	function onKey(e: KeyboardEvent) {
		if (e.target instanceof HTMLInputElement || e.target instanceof HTMLSelectElement) return;
		if (e.key === 'h') panelOpen = !panelOpen;
		if (e.key === 'f') fullscreen();
	}

	onMount(() => {
		let disposed = false;
		init().then(
			() => {
				if (disposed) return;
				ready = true;
				status = 'renderer ready';
				updateGeometry();
			},
			(e) => (status = `renderer failed: ${e}`)
		);
		listCameras().then((c) => (cameras = c));

		const ro = new ResizeObserver(updateGeometry);
		ro.observe(canvas);
		// Window moves don't fire events; re-check where the canvas sits now and then.
		const geo = setInterval(updateGeometry, 1000);
		const poll = setInterval(() => {
			if (ready) head = Array.from(room.head());
		}, 100);
		return () => {
			disposed = true;
			ro.disconnect();
			clearInterval(geo);
			clearInterval(poll);
			stopCamera();
		};
	});

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
		<button class="ghost" onclick={() => (panelOpen = !panelOpen)} title="Toggle panel (h)">
			{panelOpen ? '–' : '+'}
		</button>
	</header>

	{#if panelOpen}
		<p class="status">{status}{trackFps ? ` · ${trackFps.toFixed(0)} fps` : ''}</p>
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
			<button onclick={fullscreen} title="Fullscreen (f)">Fullscreen</button>
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
			<label>Screen diagonal (in) <input type="number" step="0.1" bind:value={settings.diagonalIn} /></label>
			<p class="hint">canvas ≈ {windowCm[0].toFixed(1)} × {windowCm[1].toFixed(1)} cm</p>
			<label>Camera FOV (°) <input type="number" step="1" bind:value={settings.cameraFovDeg} /></label>
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

		<label class="check"><input type="checkbox" bind:checked={settings.mouseMode} /> Mouse drives the head (wheel: distance)</label>
		<label class="check"><input type="checkbox" bind:checked={settings.showVideo} /> Show camera preview</label>
		<div class="row">
			<button class="ghost" onclick={reset}>Reset settings</button>
		</div>
		<p class="hint">h hides this panel, f toggles fullscreen.</p>
	{/if}
</aside>

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
