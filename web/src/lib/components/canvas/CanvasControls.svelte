<script lang="ts">
	// Barre de commandes du canevas : zoom, cadrage, rangement, annuler / rétablir,
	// mini-carte et aide. Tout est regroupé en bas à gauche.
	import CircleHelp from '@lucide/svelte/icons/circle-help';
	import LayoutGrid from '@lucide/svelte/icons/layout-grid';
	import Map from '@lucide/svelte/icons/map';
	import Maximize from '@lucide/svelte/icons/maximize';
	import Minus from '@lucide/svelte/icons/minus';
	import Plus from '@lucide/svelte/icons/plus';
	import Redo2 from '@lucide/svelte/icons/redo-2';
	import Undo2 from '@lucide/svelte/icons/undo-2';
	import type { Viewport } from '$lib/canvas/viewport.svelte';
	import { ui } from '$lib/ui.svelte';
	import { cn } from '$lib/utils';
	import { wf } from '$lib/workflow.svelte';

	let {
		viewport: vp,
		onfit,
		onarrange,
		minimap = $bindable(true),
	}: { viewport: Viewport; onfit: () => void; onarrange: () => void; minimap?: boolean } = $props();

	const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);
	const mod = isMac ? '⌘' : 'Ctrl';

	const btn =
		'text-muted-foreground hover:bg-accent hover:text-foreground flex size-8 shrink-0 items-center justify-center rounded-md disabled:pointer-events-none disabled:opacity-35';
</script>

<div class="bg-card absolute bottom-3 left-3 z-10 flex items-center gap-0.5 rounded-lg border p-0.5 shadow-sm" data-canvas-ui role="toolbar" aria-label="Commandes du canevas">
	<button type="button" class={btn} onclick={() => vp.zoomBy(1 / 1.2)} title="Dézoomer ({mod} + molette)" aria-label="Dézoomer"><Minus class="size-4" /></button>
	<button
		type="button"
		class="text-muted-foreground hover:bg-accent hover:text-foreground h-8 w-12 rounded-md text-xs tabular-nums"
		onclick={() => vp.zoomBy(1 / vp.zoom)}
		title="Revenir à 100 %"
	>
		{Math.round(vp.zoom * 100)} %
	</button>
	<button type="button" class={btn} onclick={() => vp.zoomBy(1.2)} title="Zoomer ({mod} + molette)" aria-label="Zoomer"><Plus class="size-4" /></button>

	<span class="bg-border mx-1 h-5 w-px"></span>

	<button type="button" class={btn} onclick={onfit} disabled={wf.nodes.length === 0} title="Tout afficher (Maj + 1)" aria-label="Tout afficher">
		<Maximize class="size-4" />
	</button>
	<button
		type="button"
		class={cn(btn, 'w-auto gap-1.5 px-2.5 text-xs font-medium')}
		onclick={onarrange}
		disabled={wf.nodes.length === 0}
		title="Range et aligne tous les nœuds (Maj + Alt + T)"
	>
		<LayoutGrid class="size-4" /> Ranger
	</button>

	<span class="bg-border mx-1 h-5 w-px"></span>

	<button type="button" class={btn} disabled={!wf.canUndo} onclick={() => wf.undo()} title="Annuler ({mod} + Z)" aria-label="Annuler">
		<Undo2 class="size-4" />
	</button>
	<button type="button" class={btn} disabled={!wf.canRedo} onclick={() => wf.redo()} title="Rétablir ({mod} + Maj + Z)" aria-label="Rétablir">
		<Redo2 class="size-4" />
	</button>

	<span class="bg-border mx-1 hidden h-5 w-px lg:block"></span>

	<button
		type="button"
		class={cn(btn, 'hidden lg:flex', minimap && 'bg-accent text-foreground')}
		onclick={() => (minimap = !minimap)}
		title={minimap ? 'Masquer la mini-carte' : 'Afficher la mini-carte'}
		aria-label="Mini-carte"
		aria-pressed={minimap}
	>
		<Map class="size-4" />
	</button>
	<button type="button" class={btn} onclick={() => (ui.shortcuts = true)} title="Aide : gestes et raccourcis" aria-label="Aide : gestes et raccourcis">
		<CircleHelp class="size-4" />
	</button>
</div>
