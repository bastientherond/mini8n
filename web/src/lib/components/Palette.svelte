<script lang="ts">
	import Search from '@lucide/svelte/icons/search';
	import { Input } from '$lib/components/ui/input';
	import { ScrollArea } from '$lib/components/ui/scroll-area';
	import { cn } from '$lib/utils';
	import { categoryAccent, categoryLabels, iconFor } from '$lib/icons';
	import { wf } from '$lib/workflow.svelte';

	let { onadd }: { onadd: (kind: string) => void } = $props();

	let query = $state('');

	const groups = $derived.by(() => {
		const q = query.trim().toLowerCase();
		const specs = wf.catalog.filter(
			(s) => !q || s.label.toLowerCase().includes(q) || s.description.toLowerCase().includes(q),
		);
		return Object.keys(categoryLabels)
			.map((cat) => ({ cat, items: specs.filter((s) => s.category === cat) }))
			.filter((g) => g.items.length > 0);
	});

	function ondragstart(event: DragEvent, kind: string) {
		event.dataTransfer?.setData('application/mini8n', kind);
		if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move';
	}
</script>

<aside class="bg-sidebar flex h-full w-64 shrink-0 flex-col border-r">
	<div class="p-3">
		<div class="relative">
			<Search class="text-muted-foreground absolute top-1/2 left-2.5 size-4 -translate-y-1/2" />
			<Input bind:value={query} placeholder="Rechercher un nœud…" class="pl-8" />
		</div>
	</div>
	<ScrollArea class="min-h-0 flex-1">
		<div class="space-y-4 px-3 pb-4">
			{#each groups as group (group.cat)}
				<section>
					<h3 class="text-muted-foreground mb-1.5 px-1 text-xs font-medium tracking-wide uppercase">
						{categoryLabels[group.cat]}
					</h3>
					<div class="space-y-1">
						{#each group.items as spec (spec.kind)}
							{@const Icon = iconFor(spec.icon)}
							<button
								class="hover:bg-accent flex w-full cursor-grab items-start gap-2.5 rounded-lg p-2 text-left active:cursor-grabbing"
								draggable="true"
								ondragstart={(e) => ondragstart(e, spec.kind)}
								onclick={() => onadd(spec.kind)}
								title="Cliquer ou glisser sur le canevas"
							>
								<span class={cn('mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-md', categoryAccent[spec.category])}>
									<Icon class="size-4" />
								</span>
								<span class="min-w-0">
									<span class="block text-sm font-medium">{spec.label}</span>
									<span class="text-muted-foreground block text-xs leading-snug">{spec.description}</span>
								</span>
							</button>
						{/each}
					</div>
				</section>
			{/each}
		</div>
	</ScrollArea>
</aside>
