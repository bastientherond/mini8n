<script lang="ts">
	// Commandes du canevas : zoom, cadrage, réorganisation, annuler / rétablir.
	import LayoutGrid from '@lucide/svelte/icons/layout-grid';
	import Maximize from '@lucide/svelte/icons/maximize';
	import Minus from '@lucide/svelte/icons/minus';
	import Plus from '@lucide/svelte/icons/plus';
	import Redo2 from '@lucide/svelte/icons/redo-2';
	import Undo2 from '@lucide/svelte/icons/undo-2';
	import type { Viewport } from '$lib/canvas/viewport.svelte';
	import { wf } from '$lib/workflow.svelte';

	let { viewport: vp, onfit, onarrange }: { viewport: Viewport; onfit: () => void; onarrange: () => void } = $props();

	const btn =
		'text-muted-foreground hover:bg-accent hover:text-foreground flex size-8 items-center justify-center disabled:pointer-events-none disabled:opacity-35';
</script>

<div class="absolute bottom-3 left-3 z-10 flex items-center gap-2" data-canvas-ui>
	<div class="bg-card flex overflow-hidden rounded-lg border shadow-sm">
		<button type="button" class={btn} onclick={() => vp.zoomBy(1 / 1.2)} title="Dézoomer" aria-label="Dézoomer"><Minus class="size-4" /></button>
		<button
			type="button"
			class="text-muted-foreground hover:bg-accent hover:text-foreground w-12 border-x text-xs tabular-nums"
			onclick={() => vp.zoomBy(1 / vp.zoom)}
			title="Zoom à 100 %"
		>
			{Math.round(vp.zoom * 100)} %
		</button>
		<button type="button" class={btn} onclick={() => vp.zoomBy(1.2)} title="Zoomer" aria-label="Zoomer"><Plus class="size-4" /></button>
	</div>
	<div class="bg-card flex overflow-hidden rounded-lg border shadow-sm">
		<button type="button" class={btn} onclick={onfit} title="Tout afficher (Maj + 1)" aria-label="Tout afficher"><Maximize class="size-4" /></button>
		<button type="button" class={btn} onclick={onarrange} title="Ranger et aligner les nœuds (Maj + Alt + T)" aria-label="Ranger et aligner les nœuds">
			<LayoutGrid class="size-4" />
		</button>
	</div>
	<div class="bg-card flex overflow-hidden rounded-lg border shadow-sm">
		<button type="button" class={btn} disabled={!wf.canUndo} onclick={() => wf.undo()} title="Annuler (⌘/Ctrl + Z)" aria-label="Annuler">
			<Undo2 class="size-4" />
		</button>
		<button
			type="button"
			class={btn}
			disabled={!wf.canRedo}
			onclick={() => wf.redo()}
			title="Rétablir (⌘/Ctrl + Maj + Z)"
			aria-label="Rétablir"
		>
			<Redo2 class="size-4" />
		</button>
	</div>
</div>
