<script lang="ts">
	import type { Snippet } from 'svelte';
	import { ScrollArea } from '$lib/components/ui/scroll-area';

	let {
		title,
		description,
		actions,
		children,
	}: { title: string; description?: string; actions?: Snippet; children: Snippet } = $props();
</script>

<ScrollArea class="h-full">
	<div class="mx-auto w-full max-w-6xl px-6 py-8 lg:px-10 lg:py-10">
		<div class="mb-8 flex flex-wrap items-end justify-between gap-4 border-b pb-6">
			<div class="min-w-0">
				<h1 class="truncate text-2xl font-semibold tracking-tight sm:text-[28px]">{title}</h1>
				{#if description}<p class="text-muted-foreground mt-1.5 max-w-2xl text-sm">{description}</p>{/if}
			</div>
			{#if actions}<div class="flex items-center gap-2">{@render actions()}</div>{/if}
		</div>
		{@render children()}
	</div>
</ScrollArea>
