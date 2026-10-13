<script lang="ts">
	// Barre d'actions flottante au-dessus des nœuds sélectionnés : paramétrer, exécuter
	// jusqu'ici, dupliquer, désactiver, supprimer. Taille constante quel que soit le zoom.
	import Copy from '@lucide/svelte/icons/copy';
	import Play from '@lucide/svelte/icons/play';
	import Power from '@lucide/svelte/icons/power';
	import Settings2 from '@lucide/svelte/icons/settings-2';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import { toast } from 'svelte-sonner';
	import { errorMessage } from '$lib/api';
	import type { Rect } from '$lib/canvas/router';
	import { type Viewport, boundsOf } from '$lib/canvas/viewport.svelte';
	import { cn } from '$lib/utils';
	import { wf } from '$lib/workflow.svelte';

	let { viewport: vp, rects }: { viewport: Viewport; rects: Map<string, Rect> } = $props();

	const selected = $derived(wf.nodes.filter((n) => n.selected));
	const single = $derived(selected.length === 1 ? selected[0] : null);
	const allDisabled = $derived(selected.length > 0 && selected.every((n) => n.data.disabled));

	/** Position écran : centrée au-dessus de la sélection (en dessous s'il manque de place). */
	const place = $derived.by(() => {
		const b = boundsOf(selected.flatMap((n) => rects.get(n.id) ?? []));
		if (!b) return null;
		const x = (b.x + b.w / 2) * vp.zoom + vp.x;
		const top = b.y * vp.zoom + vp.y - 10;
		const above = top >= 52;
		return { x: Math.max(150, Math.min(x, vp.width - 150)), y: above ? top : (b.y + b.h) * vp.zoom + vp.y + 10, above };
	});

	async function runTo(id: string) {
		try {
			const report = await wf.run(id);
			if (report?.error) toast.error(report.error);
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	function toggleDisabled() {
		const disabled = !allDisabled;
		wf.checkpoint();
		for (const n of selected) wf.updateData(n.id, { disabled });
	}

	const btn =
		'text-muted-foreground hover:bg-accent hover:text-foreground flex h-8 shrink-0 items-center gap-1.5 rounded-md px-2 text-xs font-medium whitespace-nowrap disabled:pointer-events-none disabled:opacity-35';
	const icon = cn(btn, 'w-8 justify-center px-0');
</script>

{#if place && selected.length > 0}
	<div
		class="bg-card absolute z-20 flex items-center gap-0.5 rounded-lg border p-0.5 shadow-md"
		style="left: {place.x}px; top: {place.y}px; transform: translate(-50%, {place.above ? '-100%' : '0'});"
		data-canvas-ui
		role="toolbar"
		aria-label="Actions sur la sélection"
	>
		{#if single}
			<button type="button" class={btn} onclick={() => (wf.openNodeId = single.id)} title="Paramétrer (Entrée ou double-clic)">
				<Settings2 class="size-4" /> Paramétrer
			</button>
			<button type="button" class={btn} disabled={wf.running} onclick={() => runTo(single.id)} title="Exécute le workflow jusqu'à ce nœud">
				<Play class="size-4" /> Exécuter jusqu'ici
			</button>
			<span class="bg-border mx-0.5 h-5 w-px"></span>
			<button type="button" class={icon} onclick={() => wf.duplicateNode(single.id)} title="Dupliquer" aria-label="Dupliquer le nœud">
				<Copy class="size-4" />
			</button>
		{:else}
			<span class="text-muted-foreground px-2 text-xs font-medium tabular-nums">{selected.length} nœuds</span>
			<span class="bg-border mx-0.5 h-5 w-px"></span>
		{/if}
		<button
			type="button"
			class={cn(icon, allDisabled && 'text-primary')}
			onclick={toggleDisabled}
			title={allDisabled ? 'Réactiver' : 'Désactiver (le nœud est ignoré à l’exécution)'}
			aria-label={allDisabled ? 'Réactiver' : 'Désactiver'}
			aria-pressed={allDisabled}
		>
			<Power class="size-4" />
		</button>
		<button
			type="button"
			class={cn(icon, 'hover:text-destructive')}
			onclick={() => wf.deleteSelection()}
			title="Supprimer (Retour arrière)"
			aria-label="Supprimer la sélection"
		>
			<Trash2 class="size-4" />
		</button>
	</div>
{/if}
