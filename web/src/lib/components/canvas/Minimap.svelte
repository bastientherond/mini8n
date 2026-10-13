<script lang="ts">
	// Mini-carte : vue d'ensemble des nœuds et de la zone visible ; un clic ou un
	// glissé déplace la vue.
	import type { Rect } from '$lib/canvas/router';
	import { type Viewport, boundsOf } from '$lib/canvas/viewport.svelte';
	import { wf } from '$lib/workflow.svelte';

	let { viewport: vp, rects }: { viewport: Viewport; rects: Map<string, Rect> } = $props();

	const WIDTH = 200;
	const HEIGHT = 130;

	const visible = $derived(vp.visible());
	const view = $derived.by(() => {
		const b = boundsOf([...rects.values(), visible])!;
		const pad = 40;
		const x = b.x - pad;
		const y = b.y - pad;
		const w = b.w + pad * 2;
		const h = b.h + pad * 2;
		// Même rapport que la mini-carte.
		const scale = Math.max(w / WIDTH, h / HEIGHT);
		return { x: x - (WIDTH * scale - w) / 2, y: y - (HEIGHT * scale - h) / 2, scale };
	});

	let svg: SVGSVGElement | undefined = $state();
	let dragging = false;

	function move(e: PointerEvent) {
		if (!svg) return;
		const r = svg.getBoundingClientRect();
		vp.centerOn({ x: view.x + (e.clientX - r.left) * view.scale, y: view.y + (e.clientY - r.top) * view.scale });
	}
</script>

{#if rects.size > 0}
	<div class="bg-card/90 absolute right-3 bottom-3 z-10 hidden overflow-hidden rounded-lg border shadow-sm backdrop-blur lg:block" data-canvas-ui>
		<svg
			bind:this={svg}
			width={WIDTH}
			height={HEIGHT}
			viewBox="{view.x} {view.y} {WIDTH * view.scale} {HEIGHT * view.scale}"
			class="block cursor-pointer"
			role="img"
			aria-label="Mini-carte du workflow"
			onpointerdown={(e) => {
				dragging = true;
				svg?.setPointerCapture(e.pointerId);
				move(e);
			}}
			onpointermove={(e) => dragging && move(e)}
			onpointerup={() => (dragging = false)}
		>
			{#each wf.nodes as node (node.id)}
				{@const r = rects.get(node.id)}
				{#if r}
					<rect
						x={r.x}
						y={r.y}
						width={r.w}
						height={r.h}
						rx={12 * 1}
						class={node.selected ? 'fill-primary/70' : 'fill-muted-foreground/50'}
					/>
				{/if}
			{/each}
			<rect
				x={visible.x}
				y={visible.y}
				width={visible.w}
				height={visible.h}
				class="fill-primary/5 stroke-primary/70"
				stroke-width={1.5 * view.scale}
				rx={6 * view.scale}
			/>
		</svg>
	</div>
{/if}
