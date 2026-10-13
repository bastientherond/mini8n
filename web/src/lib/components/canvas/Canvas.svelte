<script lang="ts">
	// Canevas de workflow fait maison : nœuds positionnés en HTML, liens en SVG routés
	// autour des nœuds (voir $lib/canvas/router.ts), interactions à la souris, au doigt
	// et au clavier.
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { layered } from '$lib/canvas/layout';
	import { type Point, type Rect, crossings, labelPoint, pathFor, routeEdges } from '$lib/canvas/router';
	import { Viewport, boundsOf } from '$lib/canvas/viewport.svelte';
	import { formatCount } from '$lib/icons';
	import { cn } from '$lib/utils';
	import { inputHandle, wf, type XYPosition } from '$lib/workflow.svelte';
	import StepNode from '../StepNode.svelte';
	import CanvasControls from './CanvasControls.svelte';
	import Minimap from './Minimap.svelte';
	import NodePicker from './NodePicker.svelte';

	let { viewport: vp }: { viewport: Viewport } = $props();

	const DEFAULT_SIZE = { w: 240, h: 92 };
	const GRID = 20;
	const SNAP = 10;
	/** Le bout du lien s'arrête au bord du port. */
	const PORT_GAP = 7;

	let root: HTMLDivElement;
	let sizes = $state<Record<string, { w: number; h: number }>>({});
	let spaceDown = $state(false);

	type Mode =
		| { kind: 'idle' }
		| { kind: 'pan'; last: Point; moved: boolean; onBackground: boolean }
		| { kind: 'drag'; start: Point; origins: Map<string, XYPosition>; moved: boolean }
		| { kind: 'box'; start: Point; current: Point; additive: boolean }
		| {
				kind: 'connect';
				node: string;
				/** `out` : depuis une sortie ; `in-n` : depuis une entrée (vers une sortie). */
				port: string;
				pointer: Point;
				target: { node: string; port: string } | null;
		  };
	let mode = $state<Mode>({ kind: 'idle' });
	let hoveredEdge = $state<string | null>(null);

	// --- Géométrie --------------------------------------------------------

	const rects = $derived(
		new Map<string, Rect>(
			wf.nodes.map((n) => {
				const s = sizes[n.id] ?? DEFAULT_SIZE;
				return [n.id, { x: n.position.x, y: n.position.y, w: s.w, h: s.h }];
			}),
		),
	);

	function inputCount(nodeId: string): number {
		const node = wf.nodes.find((n) => n.id === nodeId);
		return node ? (wf.spec(node.data.kind)?.inputs.length ?? 0) : 0;
	}

	/** Centre d'un port, en coordonnées du canevas. */
	function portPoint(nodeId: string, port: string): Point | null {
		const r = rects.get(nodeId);
		if (!r) return null;
		if (port === 'out') return { x: r.x + r.w, y: r.y + r.h / 2 };
		const i = Number(port.replace('in-', '')) || 0;
		const n = Math.max(inputCount(nodeId), 1);
		return { x: r.x, y: r.y + (r.h * (i + 1)) / (n + 1) };
	}

	const routed = $derived.by(() => {
		const requests = [];
		for (const e of wf.edges) {
			const from = portPoint(e.source, 'out');
			const to = portPoint(e.target, e.targetHandle ?? inputHandle(0));
			if (from && to) requests.push({ id: e.id, from: { x: from.x + PORT_GAP, y: from.y }, to: { x: to.x - PORT_GAP, y: to.y } });
		}
		const routes = routeEdges(requests, [...rects.values()]);
		const hops = crossings(routes);
		return { routes, hops };
	});

	const edgeViews = $derived.by(() => {
		// Liens qui partagent un tronc : une seule étiquette au même endroit.
		const taken = new Set<string>();
		return wf.edges.flatMap((e) => {
			const points = routed.routes.get(e.id);
			if (!points) return [];
			const source = wf.results[e.source];
			const target = wf.results[e.target];
			const flowed = source?.status === 'success' && !!target && target.status !== 'skipped';
			const label = labelPoint(points);
			const key = `${Math.round(label.x / 6)}:${Math.round(label.y / 6)}`;
			const duplicate = taken.has(key);
			taken.add(key);
			return [
				{
					edge: e,
					d: pathFor(points, routed.hops.get(e.id)),
					end: points[points.length - 1],
					label,
					rows: flowed ? source.rows : null,
					showRows: flowed && !duplicate,
				},
			];
		});
	});

	/** Zone couverte par les liens : le calque SVG doit l'englober entièrement, car Safari
	 *  ignore le survol et les clics sur ce qui dépasse de la boîte d'un SVG. */
	const svgBox = $derived.by(() => {
		const xs: number[] = [];
		const ys: number[] = [];
		for (const pts of routed.routes.values()) for (const p of pts) xs.push(p.x), ys.push(p.y);
		if (mode.kind === 'connect') {
			xs.push(mode.pointer.x);
			ys.push(mode.pointer.y);
			const start = portPoint(mode.node, mode.port);
			if (start) xs.push(start.x), ys.push(start.y);
		}
		if (xs.length === 0) return { x: 0, y: 0, w: 1, h: 1 };
		const margin = 60;
		const x = Math.floor(Math.min(...xs) - margin);
		const y = Math.floor(Math.min(...ys) - margin);
		return { x, y, w: Math.ceil(Math.max(...xs) + margin) - x, h: Math.ceil(Math.max(...ys) + margin) - y };
	});

	/** Survol d'un lien : maintenu un court instant après l'avoir quitté, le temps
	 *  d'atteindre sa corbeille (le bouton ne doit pas disparaître sous le pointeur). */
	let leaveTimer: ReturnType<typeof setTimeout> | undefined;

	function enterEdge(id: string) {
		clearTimeout(leaveTimer);
		hoveredEdge = id;
	}

	function leaveEdge(e: PointerEvent, id: string) {
		const next = (e.relatedTarget as Element | null)?.closest?.('[data-edge]');
		if ((next as HTMLElement | SVGElement | null)?.dataset.edge === id) return;
		clearTimeout(leaveTimer);
		leaveTimer = setTimeout(() => {
			if (hoveredEdge === id) hoveredEdge = null;
		}, 250);
	}

	// --- Insertion d'un nœud sur un lien (« + ») ----------------------------

	let picker = $state<{ edge: string; x: number; y: number } | null>(null);

	function openPicker(e: Event, id: string, at: Point) {
		e.preventDefault();
		e.stopPropagation();
		clearTimeout(leaveTimer);
		const W = 288;
		const H = 330;
		let x = at.x * vp.zoom + vp.x - W / 2;
		let y = at.y * vp.zoom + vp.y + 20;
		if (y + H > vp.height - 8) y = at.y * vp.zoom + vp.y - H - 20;
		x = Math.max(8, Math.min(x, vp.width - W - 8));
		y = Math.max(8, y);
		picker = { edge: id, x, y };
	}

	/** Nœud inséré entre la source et la cible du lien ; les nœuds suivants s'écartent si besoin. */
	function insertOnEdge(edgeId: string, kind: string) {
		picker = null;
		hoveredEdge = null;
		const edge = wf.edges.find((x) => x.id === edgeId);
		const src = edge && rects.get(edge.source);
		const tgt = edge && rects.get(edge.target);
		const out = edge && portPoint(edge.source, 'out');
		if (!edge || !src || !tgt || !out) return;
		const GAP = 80;
		const size = DEFAULT_SIZE;
		const moved = new Map<string, XYPosition>();
		let position: XYPosition;
		if (tgt.x > src.x + src.w) {
			// Lien vers la droite : nœud aligné sur la sortie de la source.
			position = { x: snap(src.x + src.w + GAP), y: snap(out.y - size.h / 2) };
			const dx = Math.ceil(Math.max(0, position.x + size.w + GAP - tgt.x) / SNAP) * SNAP;
			if (dx > 0) {
				// La cible et tout ce qui la suit se décalent vers la droite.
				const stack = [edge.target];
				while (stack.length) {
					const id = stack.pop()!;
					if (moved.has(id) || id === edge.source) continue;
					const n = wf.nodes.find((x) => x.id === id);
					if (n) moved.set(id, { x: n.position.x + dx, y: n.position.y });
					for (const e of wf.edges) if (e.source === id) stack.push(e.target);
				}
			}
		} else {
			// Lien qui revient en arrière : nœud au milieu du tracé.
			const mid = edgeViews.find((v) => v.edge.id === edgeId)?.label ?? { x: src.x, y: src.y + src.h + GAP };
			position = { x: snap(mid.x - size.w / 2), y: snap(mid.y - size.h / 2) };
		}
		const id = wf.insertOnEdge(edgeId, kind, position, moved);
		if (id) wf.openNodeId = id;
	}

	/** Corbeille d'un lien : suppression immédiate, sans sélection préalable. */
	function deleteEdge(e: Event, id: string) {
		e.preventDefault();
		e.stopPropagation();
		clearTimeout(leaveTimer);
		hoveredEdge = null;
		wf.removeEdge(id);
	}

	const connectPreview = $derived.by(() => {
		if (mode.kind !== 'connect') return null;
		const start = portPoint(mode.node, mode.port);
		if (!start) return null;
		const target = mode.target ? portPoint(mode.target.node, mode.target.port) : null;
		const end = target ?? mode.pointer;
		const [a, b] = mode.port === 'out' ? [start, end] : [end, start];
		const dx = Math.max(40, Math.abs(b.x - a.x) / 2);
		return `M${a.x},${a.y} C${a.x + dx},${a.y} ${b.x - dx},${b.y} ${b.x},${b.y}`;
	});

	// --- Utilitaires d'événements -------------------------------------------

	function syncRect() {
		const r = root.getBoundingClientRect();
		vp.left = r.left;
		vp.top = r.top;
	}

	function pointer(e: { clientX: number; clientY: number }): Point {
		return vp.toCanvas({ x: e.clientX, y: e.clientY });
	}

	const snap = (v: number) => Math.round(v / SNAP) * SNAP;

	function selectedIds() {
		return wf.nodes.filter((n) => n.selected).map((n) => n.id);
	}

	function editing(target: EventTarget | null) {
		const el = target as HTMLElement | null;
		return !!el?.closest?.('input, textarea, select, [contenteditable="true"], .cm-editor, [role="dialog"]');
	}

	/** Port ou nœud sous le pointeur, compatible avec le lien en cours. */
	function targetAt(clientX: number, clientY: number, from: { node: string; port: string }) {
		const el = document.elementFromPoint(clientX, clientY) as HTMLElement | null;
		const port = el?.closest<HTMLElement>('[data-port]');
		const wantInput = from.port === 'out';
		if (port) {
			const node = port.dataset.node!;
			const name = port.dataset.port!;
			if (node !== from.node && name.startsWith('in') === wantInput) return { node, port: name };
			return null;
		}
		// Lâcher sur le corps d'un nœud : sa première entrée libre (ou sa sortie).
		const body = el?.closest<HTMLElement>('[data-node-id]');
		const node = body?.dataset.nodeId;
		if (!node || node === from.node) return null;
		if (!wantInput) return { node, port: 'out' };
		const n = inputCount(node);
		if (n === 0) return null;
		for (let i = 0; i < n; i++) {
			if (!wf.edges.some((e) => e.target === node && (e.targetHandle ?? inputHandle(0)) === inputHandle(i))) {
				return { node, port: inputHandle(i) };
			}
		}
		return { node, port: inputHandle(0) };
	}

	// --- Pointeur ------------------------------------------------------------

	/** Pointeurs actifs (pincement à deux doigts). */
	const touches = new Map<number, Point>();
	let pinch: { distance: number; zoom: number } | null = null;

	/** Position d'appui : la capture du pointeur n'a lieu qu'après un vrai déplacement
	 *  (sinon le clic et le double-clic seraient redirigés vers le canevas). */
	let downAt: Point | null = null;

	function onpointerdown(e: PointerEvent) {
		syncRect();
		downAt = { x: e.clientX, y: e.clientY };
		const target = e.target as HTMLElement;
		if (target.closest('[data-canvas-ui]')) return;
		touches.set(e.pointerId, { x: e.clientX, y: e.clientY });
		if (touches.size === 2) {
			const [a, b] = [...touches.values()];
			pinch = { distance: Math.hypot(a.x - b.x, a.y - b.y), zoom: vp.zoom };
			mode = { kind: 'idle' };
			return;
		}
		if (e.button === 2) return;
		root.focus({ preventScroll: true });

		// Panoramique : bouton du milieu ou barre d'espace.
		if (e.button === 1 || spaceDown) {
			e.preventDefault();
			mode = { kind: 'pan', last: { x: e.clientX, y: e.clientY }, moved: false, onBackground: false };
			return;
		}
		if (e.button !== 0) return;

		const port = target.closest<HTMLElement>('[data-port]');
		if (port) {
			e.preventDefault();
			mode = { kind: 'connect', node: port.dataset.node!, port: port.dataset.port!, pointer: pointer(e), target: null };
			capture(e.pointerId);
			return;
		}

		const edgeEl = target.closest<SVGElement>('[data-edge]');
		if (edgeEl) {
			const id = edgeEl.dataset.edge!;
			const edge = wf.edges.find((x) => x.id === id);
			if (e.shiftKey || e.metaKey || e.ctrlKey) {
				wf.setSelection(
					selectedIds(),
					wf.edges.filter((x) => (x.id === id ? !x.selected : x.selected)).map((x) => x.id),
				);
			} else if (!edge?.selected) {
				wf.setSelection([], [id]);
			}
			return;
		}

		const nodeEl = target.closest<HTMLElement>('[data-node-id]');
		if (nodeEl) {
			if (target.closest('button, a, input, textarea')) return;
			const id = nodeEl.dataset.nodeId!;
			const node = wf.nodes.find((n) => n.id === id);
			let ids = selectedIds();
			if (e.shiftKey || e.metaKey || e.ctrlKey) {
				ids = node?.selected ? ids.filter((x) => x !== id) : [...ids, id];
				wf.setSelection(ids, wf.edges.filter((x) => x.selected).map((x) => x.id));
			} else if (!node?.selected) {
				ids = [id];
				wf.setSelection(ids);
			}
			const origins = new Map(wf.nodes.filter((n) => ids.includes(n.id)).map((n) => [n.id, { ...n.position }]));
			mode = { kind: 'drag', start: pointer(e), origins, moved: false };
			return;
		}

		// Fond : rectangle de sélection (Maj) ou panoramique.
		if (e.shiftKey) {
			const p = pointer(e);
			mode = { kind: 'box', start: p, current: p, additive: true };
		} else {
			mode = { kind: 'pan', last: { x: e.clientX, y: e.clientY }, moved: false, onBackground: true };
		}
	}

	function capture(id: number) {
		try {
			root.setPointerCapture(id);
		} catch {
			// Pointeur déjà relâché : rien à capturer.
		}
	}

	let frame = 0;
	let pending: PointerEvent | null = null;

	function onpointermove(e: PointerEvent) {
		if (touches.has(e.pointerId)) touches.set(e.pointerId, { x: e.clientX, y: e.clientY });
		if (pinch && touches.size === 2) {
			const [a, b] = [...touches.values()];
			const d = Math.hypot(a.x - b.x, a.y - b.y);
			vp.zoomAt({ x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }, (pinch.zoom * d) / pinch.distance);
			return;
		}
		if (mode.kind === 'idle') return;
		if (downAt && !root.hasPointerCapture(e.pointerId) && Math.hypot(e.clientX - downAt.x, e.clientY - downAt.y) > 3) {
			capture(e.pointerId);
		}
		if (mode.kind === 'pan') {
			const dx = e.clientX - mode.last.x;
			const dy = e.clientY - mode.last.y;
			vp.panBy(dx, dy);
			mode.last = { x: e.clientX, y: e.clientY };
			if (Math.abs(dx) + Math.abs(dy) > 0) mode.moved = true;
			return;
		}
		// Les autres modes recalculent des tracés : une mise à jour par image suffit.
		pending = e;
		if (!frame) frame = requestAnimationFrame(flushMove);
	}

	function flushMove() {
		frame = 0;
		const e = pending;
		pending = null;
		if (!e) return;
		const p = pointer(e);
		if (mode.kind === 'drag') {
			const dx = p.x - mode.start.x;
			const dy = p.y - mode.start.y;
			if (!mode.moved) {
				if (Math.hypot(dx, dy) * vp.zoom < 3) return;
				mode.moved = true;
				wf.checkpoint();
			}
			const free = e.altKey;
			const positions = new Map<string, XYPosition>();
			for (const [id, o] of mode.origins) {
				positions.set(id, free ? { x: o.x + dx, y: o.y + dy } : { x: snap(o.x + dx), y: snap(o.y + dy) });
			}
			wf.moveNodes(positions);
		} else if (mode.kind === 'connect') {
			mode.pointer = p;
			mode.target = targetAt(e.clientX, e.clientY, mode);
		} else if (mode.kind === 'box') {
			mode.current = p;
		}
	}

	function onpointerup(e: PointerEvent) {
		downAt = null;
		touches.delete(e.pointerId);
		if (touches.size < 2) pinch = null;
		if (root.hasPointerCapture(e.pointerId)) root.releasePointerCapture(e.pointerId);
		if (frame) {
			cancelAnimationFrame(frame);
			frame = 0;
			if (pending) flushMove();
		}
		const m = mode;
		mode = { kind: 'idle' };
		if (m.kind === 'pan' && !m.moved && m.onBackground) {
			wf.setSelection([]);
		} else if (m.kind === 'connect') {
			const t = targetAt(e.clientX, e.clientY, m);
			if (!t) return;
			const [source, target, port] = m.port === 'out' ? [m.node, t.node, t.port] : [t.node, m.node, m.port];
			const error = wf.connect(source, target, Number(port.replace('in-', '')) || 0);
			if (error) toast.error(error);
		} else if (m.kind === 'box') {
			const box = normalize(m.start, m.current);
			const hit = [...rects].filter(([, r]) => r.x < box.x + box.w && r.x + r.w > box.x && r.y < box.y + box.h && r.y + r.h > box.y);
			const ids = new Set([...(m.additive ? selectedIds() : []), ...hit.map(([id]) => id)]);
			wf.setSelection(ids);
		}
	}

	function normalize(a: Point, b: Point): Rect {
		return { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), w: Math.abs(a.x - b.x), h: Math.abs(a.y - b.y) };
	}

	function onwheel(e: WheelEvent) {
		if ((e.target as HTMLElement).closest('[data-canvas-ui]')) return;
		e.preventDefault();
		syncRect();
		if (e.ctrlKey || e.metaKey) {
			// Pincement sur pavé tactile (ctrl + molette) ou Ctrl/⌘ + molette.
			const delta = e.deltaMode === 1 ? e.deltaY * 16 : e.deltaY;
			vp.zoomAt({ x: e.clientX, y: e.clientY }, vp.zoom * Math.exp(-delta * 0.008));
		} else {
			const unit = e.deltaMode === 1 ? 16 : 1;
			vp.panBy(-e.deltaX * unit, -e.deltaY * unit);
		}
	}

	// --- Clavier -------------------------------------------------------------

	function onkeydown(e: KeyboardEvent) {
		if (wf.openNodeId || editing(e.target)) return;
		const mod = e.metaKey || e.ctrlKey;
		if (e.key === ' ' && !e.repeat) {
			spaceDown = true;
			if (e.target === root || e.target === document.body) e.preventDefault();
		} else if (e.key === 'Delete' || e.key === 'Backspace') {
			if (wf.deleteSelection() > 0) e.preventDefault();
		} else if (e.key === 'Escape') {
			if (mode.kind !== 'idle') mode = { kind: 'idle' };
			else wf.setSelection([]);
		} else if (mod && e.key.toLowerCase() === 'a') {
			e.preventDefault();
			wf.setSelection(wf.nodes.map((n) => n.id));
		} else if (mod && e.key.toLowerCase() === 'z') {
			e.preventDefault();
			if (e.shiftKey) wf.redo();
			else wf.undo();
		} else if (mod && e.key.toLowerCase() === 'y') {
			e.preventDefault();
			wf.redo();
		} else if (e.shiftKey && e.altKey && e.code === 'KeyT') {
			e.preventDefault();
			arrange();
		} else if (e.shiftKey && e.code === 'Digit1') {
			fit();
		}
	}

	function onkeyup(e: KeyboardEvent) {
		if (e.key === ' ') spaceDown = false;
	}

	// --- API pour l'éditeur --------------------------------------------------

	/** Cadrage demandé avant que le canevas ait une taille. */
	let pendingFit = false;

	/** Cadre tous les nœuds. */
	export function fit() {
		if (!root || root.clientWidth === 0) {
			pendingFit = true;
			return;
		}
		vp.width = root.clientWidth;
		vp.height = root.clientHeight;
		syncRect();
		vp.fit(boundsOf([...rects.values()]));
	}

	/** Position du canevas sous un point de l'écran. */
	export function toCanvas(clientX: number, clientY: number): Point {
		syncRect();
		return vp.toCanvas({ x: clientX, y: clientY });
	}

	/** Centre de la zone visible. */
	export function center(): Point {
		syncRect();
		return vp.toCanvas({ x: vp.left + vp.width / 2, y: vp.top + vp.height / 2 });
	}

	/** Range automatiquement les nœuds en colonnes. */
	export function arrange() {
		if (wf.nodes.length === 0) return;
		const positions = layered(
			wf.nodes.map((n) => ({ id: n.id, ...(sizes[n.id] ?? DEFAULT_SIZE), y: n.position.y })),
			wf.edges.map((e) => ({ source: e.source, target: e.target, input: Number(e.targetHandle?.replace('in-', '')) || 0 })),
		);
		// Garde l'emplacement du graphe : décalage pour que le premier nœud ne bouge pas.
		const first = wf.nodes[0];
		const anchor = positions.get(first.id)!;
		const dx = Math.round((first.position.x - anchor.x) / SNAP) * SNAP;
		const dy = Math.round((first.position.y - anchor.y) / SNAP) * SNAP;
		for (const [id, p] of positions) positions.set(id, { x: p.x + dx, y: p.y + dy });
		wf.checkpoint();
		wf.moveNodes(positions);
		requestAnimationFrame(fit);
		toast.success('Nœuds rangés', { action: { label: 'Annuler', onClick: () => wf.undo() } });
	}

	// --- Mesures ---------------------------------------------------------------

	function measure(el: HTMLElement, id: string) {
		const update = () => {
			const s = { w: el.offsetWidth, h: el.offsetHeight };
			const old = sizes[id];
			if (!old || old.w !== s.w || old.h !== s.h) sizes = { ...sizes, [id]: s };
		};
		const ro = new ResizeObserver(update);
		ro.observe(el);
		update();
		return { destroy: () => ro.disconnect() };
	}

	onMount(() => {
		const ro = new ResizeObserver(() => {
			vp.width = root.clientWidth;
			vp.height = root.clientHeight;
			syncRect();
			if (pendingFit && vp.width > 0) {
				pendingFit = false;
				fit();
			}
		});
		ro.observe(root);
		// Écouteur non passif : la molette ne doit pas faire défiler la page.
		root.addEventListener('wheel', onwheel, { passive: false });
		return () => {
			ro.disconnect();
			root.removeEventListener('wheel', onwheel);
		};
	});

	const box = $derived(mode.kind === 'box' ? normalize(mode.start, mode.current) : null);
	const connectingFromOutput = $derived(mode.kind === 'connect' && mode.port === 'out');
	const dotSize = $derived(GRID * vp.zoom);
</script>

<svelte:window {onkeydown} {onkeyup} onblur={() => (spaceDown = false)} />

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
	bind:this={root}
	class={cn(
		'mini8n-canvas relative h-full w-full touch-none overflow-hidden outline-none select-none',
		(spaceDown || mode.kind === 'pan') && 'cursor-grab',
		mode.kind === 'pan' && mode.moved && 'cursor-grabbing',
		mode.kind === 'connect' && 'cursor-crosshair',
		wf.running && 'mini8n-running',
	)}
	style="background-size: {dotSize}px {dotSize}px; background-position: {vp.x}px {vp.y}px;"
	tabindex="0"
	role="application"
	aria-label="Canevas du workflow : glissez le fond pour vous déplacer, Ctrl + molette pour zoomer"
	{onpointerdown}
	{onpointermove}
	{onpointerup}
	onpointercancel={onpointerup}
	oncontextmenu={(e) => mode.kind !== 'idle' && e.preventDefault()}
>
	<div class="absolute top-0 left-0 origin-top-left" style="transform: translate({vp.x}px, {vp.y}px) scale({vp.zoom});">
		<!-- Liens (sous les nœuds) -->
		<svg
			class="pointer-events-none absolute overflow-visible"
			style="left: {svgBox.x}px; top: {svgBox.y}px;"
			width={svgBox.w}
			height={svgBox.h}
			viewBox="{svgBox.x} {svgBox.y} {svgBox.w} {svgBox.h}"
			aria-hidden="true"
		>
			{#each edgeViews as v (v.edge.id)}
				{@const active = v.edge.selected || hoveredEdge === v.edge.id}
				<g
					class={cn('mini8n-edge', v.edge.selected && 'is-selected', active && 'is-active', v.rows !== null && 'has-data')}
					data-edge={v.edge.id}
				>
					<!-- Zone de clic élargie -->
					<path
						d={v.d}
						class="cursor-pointer"
						style="pointer-events: stroke;"
						fill="none"
						stroke="transparent"
						stroke-width="16"
						role="presentation"
						onpointerenter={() => enterEdge(v.edge.id)}
						onpointerleave={(e) => leaveEdge(e, v.edge.id)}
					/>
					<path d={v.d} class="mini8n-edge-path" fill="none" />
					<path d="M{v.end.x - 7},{v.end.y - 4.5} L{v.end.x + 1},{v.end.y} L{v.end.x - 7},{v.end.y + 4.5} Z" class="mini8n-edge-arrow" />
				</g>
			{/each}
			{#if connectPreview}
				<path d={connectPreview} class="mini8n-edge-preview" fill="none" />
			{/if}
		</svg>

		<!-- Étiquettes des liens : lignes transmises, suppression -->
		{#each edgeViews as v (v.edge.id)}
			{@const active = v.edge.selected || hoveredEdge === v.edge.id || picker?.edge === v.edge.id}
			{#if active || v.showRows}
				<div
					class="absolute flex -translate-x-1/2 -translate-y-1/2 items-center gap-1"
					style="left: {v.label.x}px; top: {v.label.y}px;"
					data-edge={v.edge.id}
					role="presentation"
					onpointerenter={() => enterEdge(v.edge.id)}
					onpointerleave={(e) => leaveEdge(e, v.edge.id)}
				>
					{#if v.showRows && v.rows !== null}
						<span
							class="whitespace-nowrap rounded-full border border-emerald-500/30 bg-emerald-50 px-1.5 py-px text-[10px] font-medium text-emerald-700 tabular-nums dark:bg-emerald-950 dark:text-emerald-300"
						>
							{formatCount(v.rows)} ligne{v.rows > 1 ? 's' : ''}
						</span>
					{/if}
					{#if active}
						<button
							type="button"
							class="bg-card text-muted-foreground hover:text-primary hover:border-primary/40 flex size-6 items-center justify-center rounded-md border shadow-sm"
							title="Insérer un nœud ici"
							aria-label="Insérer un nœud sur ce lien"
							data-canvas-ui
							onpointerdown={(e) => e.button === 0 && openPicker(e, v.edge.id, v.label)}
							onclick={(e) => openPicker(e, v.edge.id, v.label)}
						>
							<Plus class="size-3.5" />
						</button>
						<button
							type="button"
							class="bg-card text-muted-foreground hover:text-destructive hover:border-destructive/40 flex size-6 items-center justify-center rounded-md border shadow-sm"
							title="Supprimer le lien"
							aria-label="Supprimer le lien"
							data-canvas-ui
							onpointerdown={(e) => e.button === 0 && deleteEdge(e, v.edge.id)}
							onclick={(e) => deleteEdge(e, v.edge.id)}
						>
							<Trash2 class="size-3.5" />
						</button>
					{/if}
				</div>
			{/if}
		{/each}

		<!-- Nœuds -->
		{#each wf.nodes as node (node.id)}
			{@const hover = mode.kind === 'connect' && mode.target?.node === node.id ? mode.target.port : null}
			<div
				class="absolute top-0 left-0"
				style="transform: translate({node.position.x}px, {node.position.y}px);"
				data-node-id={node.id}
				use:measure={node.id}
			>
				<StepNode
					id={node.id}
					data={node.data}
					selected={!!node.selected}
					connecting={connectingFromOutput && mode.kind === 'connect' && mode.node !== node.id}
					hoverInput={hover?.startsWith('in') ? Number(hover.replace('in-', '')) : null}
				/>
			</div>
		{/each}
	</div>

	{#if box}
		<div
			class="border-primary/60 bg-primary/10 pointer-events-none absolute rounded-sm border"
			style="left: {box.x * vp.zoom + vp.x}px; top: {box.y * vp.zoom + vp.y}px; width: {box.w * vp.zoom}px; height: {box.h * vp.zoom}px;"
		></div>
	{/if}

	{#if picker}
		{@const p = picker}
		<NodePicker x={p.x} y={p.y} onpick={(kind) => insertOnEdge(p.edge, kind)} onclose={() => (picker = null)} />
	{/if}

	<CanvasControls viewport={vp} onfit={fit} onarrange={arrange} />
	<Minimap viewport={vp} rects={rects} />
</div>
