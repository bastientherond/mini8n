<script lang="ts">
	// Choix d'un nœud à brancher (bouton « + » d'un lien, lien tiré dans le vide) :
	// suggestions selon l'étape voisine, puis tous les nœuds compatibles par catégorie ;
	// recherche et clavier.
	import Search from '@lucide/svelte/icons/search';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import { onMount } from 'svelte';
	import { brandOfNode } from '$lib/brands';
	import { propose } from '$lib/canvas/suggest';
	import BrandLogo from '../BrandLogo.svelte';
	import type { NodeSpec } from '$lib/engine/types';
	import { categoryAccent, categoryLabels, iconFor } from '$lib/icons';
	import { cn } from '$lib/utils';
	import { wf } from '$lib/workflow.svelte';

	let {
		x,
		y,
		neighbour,
		direction = 'after',
		title = 'Insérer un nœud',
		onpick,
		onclose,
	}: {
		/** Position dans le canevas (pixels écran, relatifs au canevas). */
		x: number;
		y: number;
		/** Type du nœud voisin : celui d'avant (`after`) ou celui d'après (`before`). */
		neighbour?: string;
		direction?: 'after' | 'before';
		title?: string;
		onpick: (kind: string) => void;
		onclose: () => void;
	} = $props();

	let query = $state('');
	let active = $state(0);
	let input: HTMLInputElement | undefined = $state();
	let panel: HTMLDivElement | undefined = $state();

	const proposal = $derived(propose(wf.catalog, neighbour ? wf.spec(neighbour) : undefined, direction));

	/** Recherche insensible à la casse et aux accents. */
	const fold = (s: string) => s.normalize('NFD').replace(/\p{Diacritic}/gu, '').toLowerCase();

	/** Sections affichées ; la navigation au clavier parcourt leurs éléments à la suite. */
	const sections = $derived.by((): { title: string; items: NodeSpec[]; suggested?: boolean }[] => {
		const q = fold(query.trim());
		if (q) {
			const items = proposal.candidates.filter((s) => fold(`${s.label} ${s.description} ${categoryLabels[s.category]} ${s.kind}`).includes(q));
			return items.length ? [{ title: 'Résultats', items }] : [];
		}
		const out: { title: string; items: NodeSpec[]; suggested?: boolean }[] = [];
		if (proposal.suggested.length) out.push({ title: 'Suggestions', items: proposal.suggested, suggested: true });
		for (const cat of Object.keys(categoryLabels)) {
			const items = proposal.candidates.filter((s) => s.category === cat);
			if (items.length) out.push({ title: categoryLabels[cat], items });
		}
		return out;
	});
	const flat = $derived(sections.flatMap((s) => s.items));
	/** Index de départ de chaque section dans la liste à plat. */
	const offsets = $derived(sections.reduce<number[]>((acc, s, i) => [...acc, i === 0 ? 0 : acc[i - 1] + sections[i - 1].items.length], []));

	$effect(() => {
		void query;
		active = 0;
	});

	function move(to: number) {
		active = Math.max(0, Math.min(to, flat.length - 1));
		panel?.querySelector(`[data-index="${active}"]`)?.scrollIntoView({ block: 'nearest' });
	}

	function onkeydown(e: KeyboardEvent) {
		if (e.key === 'ArrowDown') {
			e.preventDefault();
			move(active + 1);
		} else if (e.key === 'ArrowUp') {
			e.preventDefault();
			move(active - 1);
		} else if (e.key === 'Enter' && flat[active]) {
			e.preventDefault();
			onpick(flat[active].kind);
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
	class="bg-popover text-popover-foreground absolute z-30 flex max-h-96 w-72 flex-col overflow-hidden rounded-xl border shadow-xl"
	style="left: {x}px; top: {y}px;"
	data-canvas-ui
	role="dialog"
	aria-label={title}
>
	<div class="relative border-b p-2">
		<Search class="text-muted-foreground absolute top-1/2 left-4 size-3.5 -translate-y-1/2" />
		<input
			bind:this={input}
			bind:value={query}
			{onkeydown}
			placeholder="{title}…"
			class="bg-muted/60 focus-visible:ring-ring/50 h-8 w-full rounded-md pr-2 pl-7 text-sm outline-none focus-visible:ring-2"
			aria-label="Rechercher un nœud"
		/>
	</div>
	<div class="min-h-0 flex-1 overflow-y-auto p-1" role="listbox">
		{#each sections as section, si (section.title)}
			<div
				class="text-muted-foreground flex items-center gap-1 px-2 pt-2 pb-1 text-[10px] font-semibold tracking-wide uppercase"
				role="presentation"
			>
				{#if section.suggested}<Sparkles class="text-primary size-3" />{/if}
				{section.title}
			</div>
			{#each section.items as spec, j (spec.kind)}
				{@const i = offsets[si] + j}
				{@const Icon = iconFor(spec.icon)}
				{@const brand = brandOfNode(spec.kind)}
				<button
					type="button"
					role="option"
					aria-selected={i === active}
					data-index={i}
					class={cn('flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left', i === active && 'bg-accent')}
					onpointerenter={() => (active = i)}
					onclick={() => onpick(spec.kind)}
				>
					{#if brand}
						<BrandLogo {brand} class="size-7 rounded-md" />
					{:else}
						<span class={cn('flex size-7 shrink-0 items-center justify-center rounded-md', categoryAccent[spec.category])}>
							<Icon class="size-4" />
						</span>
					{/if}
					<span class="min-w-0">
						<span class="block truncate text-sm font-medium">{spec.label}</span>
						<span class="text-muted-foreground block truncate text-xs">{spec.description}</span>
					</span>
				</button>
			{/each}
		{:else}
			<p class="text-muted-foreground px-2 py-3 text-center text-sm">Aucun nœud ne correspond.</p>
		{/each}
	</div>
</div>
