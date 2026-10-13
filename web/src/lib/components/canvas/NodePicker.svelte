<script lang="ts">
	// Choix du nœud à insérer sur un lien (bouton « + ») : recherche et clavier.
	import Search from '@lucide/svelte/icons/search';
	import { onMount } from 'svelte';
	import { categoryAccent, categoryLabels, iconFor } from '$lib/icons';
	import { cn } from '$lib/utils';
	import { wf } from '$lib/workflow.svelte';

	let {
		x,
		y,
		onpick,
		onclose,
	}: {
		/** Position dans le canevas (pixels écran, relatifs au canevas). */
		x: number;
		y: number;
		onpick: (kind: string) => void;
		onclose: () => void;
	} = $props();

	let query = $state('');
	let active = $state(0);
	let input: HTMLInputElement | undefined = $state();
	let panel: HTMLDivElement | undefined = $state();

	/** Seuls les nœuds qui ont une entrée peuvent s'insérer entre deux nœuds. */
	const candidates = $derived(wf.catalog.filter((s) => s.category !== 'trigger' && s.inputs.length > 0));
	const filtered = $derived.by(() => {
		const q = query.trim().toLowerCase();
		return q
			? candidates.filter((s) => `${s.label} ${s.description} ${categoryLabels[s.category]}`.toLowerCase().includes(q))
			: candidates;
	});
	$effect(() => {
		void query;
		active = 0;
	});

	function onkeydown(e: KeyboardEvent) {
		if (e.key === 'ArrowDown') {
			e.preventDefault();
			active = Math.min(active + 1, filtered.length - 1);
			panel?.querySelector(`[data-index="${active}"]`)?.scrollIntoView({ block: 'nearest' });
		} else if (e.key === 'ArrowUp') {
			e.preventDefault();
			active = Math.max(active - 1, 0);
			panel?.querySelector(`[data-index="${active}"]`)?.scrollIntoView({ block: 'nearest' });
		} else if (e.key === 'Enter' && filtered[active]) {
			e.preventDefault();
			onpick(filtered[active].kind);
		} else if (e.key === 'Escape') {
			e.preventDefault();
			e.stopPropagation();
			onclose();
		}
	}

	onMount(() => {
		input?.focus();
		// Clic en dehors : fermeture.
		const outside = (e: PointerEvent) => {
			if (panel && !panel.contains(e.target as Node)) onclose();
		};
		setTimeout(() => window.addEventListener('pointerdown', outside, true));
		return () => window.removeEventListener('pointerdown', outside, true);
	});
</script>

<div
	bind:this={panel}
	class="bg-popover text-popover-foreground absolute z-30 flex max-h-80 w-72 flex-col overflow-hidden rounded-xl border shadow-xl"
	style="left: {x}px; top: {y}px;"
	data-canvas-ui
	role="dialog"
	aria-label="Insérer un nœud"
>
	<div class="relative border-b p-2">
		<Search class="text-muted-foreground absolute top-1/2 left-4 size-3.5 -translate-y-1/2" />
		<input
			bind:this={input}
			bind:value={query}
			{onkeydown}
			placeholder="Insérer un nœud…"
			class="bg-muted/60 focus-visible:ring-ring/50 h-8 w-full rounded-md pr-2 pl-7 text-sm outline-none focus-visible:ring-2"
			aria-label="Rechercher un nœud à insérer"
		/>
	</div>
	<div class="min-h-0 flex-1 overflow-y-auto p-1" role="listbox">
		{#each filtered as spec, i (spec.kind)}
			{@const Icon = iconFor(spec.icon)}
			<button
				type="button"
				role="option"
				aria-selected={i === active}
				data-index={i}
				class={cn('flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left', i === active && 'bg-accent')}
				onpointerenter={() => (active = i)}
				onclick={() => onpick(spec.kind)}
			>
				<span class={cn('flex size-7 shrink-0 items-center justify-center rounded-md', categoryAccent[spec.category])}>
					<Icon class="size-4" />
				</span>
				<span class="min-w-0">
					<span class="block truncate text-sm font-medium">{spec.label}</span>
					<span class="text-muted-foreground block truncate text-xs">{spec.description}</span>
				</span>
			</button>
		{:else}
			<p class="text-muted-foreground px-2 py-3 text-center text-sm">Aucun nœud ne correspond.</p>
		{/each}
	</div>
</div>
