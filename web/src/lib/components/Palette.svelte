<script lang="ts">
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import PanelLeftClose from '@lucide/svelte/icons/panel-left-close';
	import Plus from '@lucide/svelte/icons/plus';
	import Search from '@lucide/svelte/icons/search';
	import X from '@lucide/svelte/icons/x';
	import { ScrollArea } from '$lib/components/ui/scroll-area';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import type { NodeSpec } from '$lib/engine/types';
	import { cn } from '$lib/utils';
	import { categoryAccent, categoryLabels, iconFor } from '$lib/icons';
	import { wf } from '$lib/workflow.svelte';

	let { onadd, onclose }: { onadd: (kind: string) => void; onclose: () => void } = $props();

	const COLLAPSED_KEY = 'mini8n.paletteCollapsed';

	let query = $state('');
	let category = $state<string | null>(null);
	let collapsed = $state<Record<string, boolean>>(readCollapsed());
	let searchInput: HTMLInputElement | null = $state(null);

	function readCollapsed(): Record<string, boolean> {
		try {
			return JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? '{}');
		} catch {
			return {};
		}
	}

	function toggle(cat: string) {
		collapsed = { ...collapsed, [cat]: !collapsed[cat] };
		try {
			localStorage.setItem(COLLAPSED_KEY, JSON.stringify(collapsed));
		} catch {
			// confort uniquement
		}
	}

	/** Recherche insensible à la casse et aux accents. */
	const fold = (s: string) => s.normalize('NFD').replace(/\p{Diacritic}/gu, '').toLowerCase();

	const counts = $derived(
		Object.fromEntries(Object.keys(categoryLabels).map((c) => [c, wf.catalog.filter((s) => s.category === c).length])),
	);

	const groups = $derived.by(() => {
		const q = fold(query.trim());
		const match = (s: NodeSpec) => !q || fold(`${s.label} ${s.description} ${s.kind}`).includes(q);
		return Object.keys(categoryLabels)
			.filter((cat) => !category || cat === category)
			.map((cat) => ({ cat, items: wf.catalog.filter((s) => s.category === cat && match(s)) }))
			.filter((g) => g.items.length > 0);
	});

	const searching = $derived(query.trim().length > 0);
	const total = $derived(groups.reduce((n, g) => n + g.items.length, 0));

	function ondragstart(event: DragEvent, kind: string) {
		event.dataTransfer?.setData('application/mini8n', kind);
		if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move';
	}

	function onkeydown(event: KeyboardEvent) {
		const target = event.target as HTMLElement | null;
		const typing = target?.closest('input, textarea, [contenteditable="true"]');
		if (event.key === '/' && !typing && !event.metaKey && !event.ctrlKey) {
			event.preventDefault();
			searchInput?.focus();
		}
	}

	function onSearchKey(event: KeyboardEvent) {
		if (event.key === 'Escape') {
			query = '';
			searchInput?.blur();
		} else if (event.key === 'Enter' && total === 1) {
			onadd(groups[0].items[0].kind);
			query = '';
		}
	}
</script>

<svelte:window {onkeydown} />

<aside class="bg-sidebar flex h-full w-72 shrink-0 flex-col border-r" aria-label="Nœuds disponibles">
	<div class="flex items-center justify-between gap-2 px-3 pt-3">
		<h2 class="text-sm font-semibold">Ajouter un nœud</h2>
		<button
			class="text-muted-foreground hover:text-foreground hover:bg-accent rounded-md p-1"
			onclick={onclose}
			aria-label="Masquer la palette"
			title="Masquer la palette"
		>
			<PanelLeftClose class="size-4" />
		</button>
	</div>

	<div class="space-y-2 p-3">
		<div class="relative">
			<Search class="text-muted-foreground absolute top-1/2 left-2.5 size-4 -translate-y-1/2" />
			<input
				bind:this={searchInput}
				bind:value={query}
				onkeydown={onSearchKey}
				placeholder="Rechercher…"
				aria-label="Rechercher un nœud"
				class="border-input bg-background focus-visible:border-ring focus-visible:ring-ring/50 h-9 w-full rounded-lg border pr-14 pl-8 text-sm outline-none focus-visible:ring-3"
			/>
			{#if query}
				<button
					class="text-muted-foreground hover:text-foreground absolute top-1/2 right-2 -translate-y-1/2"
					onclick={() => (query = '')}
					aria-label="Effacer la recherche"><X class="size-4" /></button
				>
			{:else}
				<kbd class="bg-muted text-muted-foreground absolute top-1/2 right-2 -translate-y-1/2 rounded border px-1.5 font-mono text-[10px]">/</kbd>
			{/if}
		</div>

		<!-- Filtre par catégorie -->
		<div class="flex flex-wrap gap-1" role="group" aria-label="Catégories">
			<button
				class={cn(
					'rounded-full border px-2.5 py-0.5 text-xs transition-colors',
					category === null ? 'bg-foreground text-background border-foreground' : 'hover:bg-accent text-muted-foreground',
				)}
				aria-pressed={category === null}
				onclick={() => (category = null)}>Tout</button
			>
			{#each Object.entries(categoryLabels) as [cat, label] (cat)}
				{#if counts[cat] > 0}
					<button
						class={cn(
							'flex items-center gap-1 rounded-full border px-2.5 py-0.5 text-xs transition-colors',
							category === cat ? 'bg-foreground text-background border-foreground' : 'hover:bg-accent text-muted-foreground',
						)}
						aria-pressed={category === cat}
						onclick={() => (category = category === cat ? null : cat)}
					>
						{label}
						<span class="opacity-60">{counts[cat]}</span>
					</button>
				{/if}
			{/each}
		</div>
	</div>

	<ScrollArea class="min-h-0 flex-1 border-t">
		<div class="pb-3">
			{#each groups as group (group.cat)}
				{@const open = searching || category !== null || !collapsed[group.cat]}
				<section>
					<button
						class="bg-sidebar/95 text-muted-foreground hover:text-foreground sticky top-0 z-10 flex w-full items-center gap-2 px-3 py-2 text-xs font-medium tracking-wide uppercase backdrop-blur"
						onclick={() => toggle(group.cat)}
						aria-expanded={open}
						disabled={searching || category !== null}
					>
						<ChevronRight class={cn('size-3.5 transition-transform', open && 'rotate-90')} />
						<span class={cn('size-2 rounded-full', categoryAccent[group.cat]?.split(' ')[0])}></span>
						<span class="flex-1 text-left">{categoryLabels[group.cat]}</span>
						<span class="font-normal normal-case">{group.items.length}</span>
					</button>
					{#if open}
						<ul class="px-2">
							{#each group.items as spec (spec.kind)}
								{@const Icon = iconFor(spec.icon)}
								<li>
									<Tooltip.Root delayDuration={500}>
										<Tooltip.Trigger>
											{#snippet child({ props })}
												<button
													{...props}
													class="group hover:bg-accent focus-visible:bg-accent flex w-full cursor-grab items-center gap-2.5 rounded-lg px-2 py-1.5 text-left outline-none active:cursor-grabbing"
													draggable="true"
													ondragstart={(e) => ondragstart(e, spec.kind)}
													onclick={() => onadd(spec.kind)}
												>
													<span class={cn('flex size-8 shrink-0 items-center justify-center rounded-md', categoryAccent[spec.category])}>
														<Icon class="size-4" />
													</span>
													<span class="min-w-0 flex-1">
														<span class="block truncate text-sm font-medium">{spec.label}</span>
														<span class="text-muted-foreground block truncate text-xs">{spec.description}</span>
													</span>
													<Plus class="text-muted-foreground size-4 shrink-0 opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100" />
												</button>
											{/snippet}
										</Tooltip.Trigger>
										<Tooltip.Content side="right" sideOffset={8} class="max-w-64 flex-col items-start">
											<p class="font-medium">{spec.label}</p>
											<p class="opacity-80">{spec.description}</p>
										</Tooltip.Content>
									</Tooltip.Root>
								</li>
							{/each}
						</ul>
					{/if}
				</section>
			{:else}
				<p class="text-muted-foreground px-4 py-8 text-center text-sm">Aucun nœud ne correspond à « {query} ».</p>
			{/each}
		</div>
	</ScrollArea>

	<p class="text-muted-foreground border-t px-3 py-2 text-[11px]">Cliquez ou glissez un nœud sur le canevas.</p>
</aside>
