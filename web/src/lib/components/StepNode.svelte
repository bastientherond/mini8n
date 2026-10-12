<script lang="ts">
	import { Handle, Position, type NodeProps } from '@xyflow/svelte';
	import CircleCheck from '@lucide/svelte/icons/circle-check';
	import CircleX from '@lucide/svelte/icons/circle-x';
	import CircleMinus from '@lucide/svelte/icons/circle-minus';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import Play from '@lucide/svelte/icons/play';
	import Settings2 from '@lucide/svelte/icons/settings-2';
	import { cn } from '$lib/utils';
	import { categoryAccent, categoryBar, formatCount, iconFor } from '$lib/icons';
	import { inputHandle, wf, type StepNode } from '$lib/workflow.svelte';

	let { id, data, selected }: NodeProps<StepNode> = $props();

	const spec = $derived(wf.spec(data.kind));
	const result = $derived(wf.results[id]);
	const running = $derived(wf.runningIds.has(id));
	const Icon = $derived(iconFor(spec?.icon));
	const inputs = $derived(spec?.inputs ?? []);
</script>

<div
	ondblclick={() => (wf.openNodeId = id)}
	role="group"
	title="Double-cliquez pour paramétrer"
	class={cn(
		'group bg-card text-card-foreground relative w-60 rounded-xl border shadow-sm transition-[box-shadow,border-color,transform] hover:shadow-md',
		spec?.category === 'trigger' && 'rounded-l-[30px]',
		selected && 'border-primary/70 ring-primary/20 shadow-md ring-4',
		running && 'border-primary/60 ring-primary/15 animate-pulse ring-4',
		result?.status === 'error' && 'border-destructive/70 ring-destructive/10 ring-4',
		data.disabled && 'opacity-50 grayscale',
	)}
>
	<!-- Liseré de catégorie -->
	<span
		class={cn(
			'absolute top-3 bottom-3 left-0 w-1 rounded-r-full',
			categoryBar[spec?.category ?? 'transform'],
			spec?.category === 'trigger' && 'hidden',
		)}
	></span>
	<div class={cn('flex items-center gap-3 p-3 pl-4', spec?.category === 'trigger' && 'pl-4')}>
		<div
			class={cn(
				'flex size-10 shrink-0 items-center justify-center rounded-xl',
				categoryAccent[spec?.category ?? 'transform'],
				spec?.category === 'trigger' && 'rounded-full',
			)}
		>
			<Icon class="size-5" />
		</div>
		<div class="min-w-0 flex-1">
			<div class="truncate text-sm leading-tight font-semibold">{data.name}</div>
			<div class="text-muted-foreground mt-0.5 truncate text-xs">{spec?.label ?? data.kind}</div>
		</div>
		<div class="bg-card/90 absolute -top-3.5 right-2 hidden items-center rounded-lg border p-0.5 shadow-sm group-hover:flex">
			<button
				class="nodrag text-muted-foreground hover:text-foreground hover:bg-accent flex size-6 items-center justify-center rounded-md"
				title="Paramétrer"
				aria-label="Paramétrer le nœud"
				onclick={(e) => {
					e.stopPropagation();
					wf.openNodeId = id;
				}}
			>
				<Settings2 class="size-3.5" />
			</button>
			<button
				class="nodrag text-muted-foreground hover:text-primary hover:bg-accent flex size-6 items-center justify-center rounded-md"
				title="Exécuter jusqu'ici"
				aria-label="Exécuter jusqu'ici"
				disabled={wf.running}
				onclick={(e) => {
					e.stopPropagation();
					wf.run(id);
				}}
			>
				<Play class="size-3.5" />
			</button>
		</div>
	</div>

	<div
		class={cn(
			'bg-muted/40 text-muted-foreground flex h-7 items-center gap-1.5 rounded-b-xl border-t px-3 text-[11px]',
			spec?.category === 'trigger' && 'rounded-bl-[30px] pl-5',
		)}
	>
		{#if running}
			<LoaderCircle class="text-primary size-3.5 animate-spin" />
			<span>Exécution…</span>
		{:else if result?.status === 'success'}
			<CircleCheck class="size-3.5 text-emerald-500" />
			<span class="tabular-nums">{formatCount(result.rows)} ligne{result.rows > 1 ? 's' : ''} · {result.columns.length} col.</span>
		{:else if result?.status === 'error'}
			<CircleX class="text-destructive size-3.5 shrink-0" />
			<span class="text-destructive truncate" title={result.error}>{result.error}</span>
		{:else if result?.status === 'skipped'}
			<CircleMinus class="size-3.5" />
			<span>Ignoré</span>
		{:else}
			<span class="bg-muted-foreground/40 size-1.5 rounded-full"></span>
			<span>Pas encore exécuté</span>
		{/if}
	</div>

	{#each inputs as label, i (i)}
		{@const top = ((i + 1) / (inputs.length + 1)) * 100}
		<Handle type="target" position={Position.Left} id={inputHandle(i)} style="top: {top}%" />
		{#if inputs.length > 1}
			<span
				class="text-muted-foreground pointer-events-none absolute -left-5 -translate-y-1/2 text-[10px] font-medium"
				style="top: {top}%">{label}</span
			>
		{/if}
	{/each}
	<Handle type="source" position={Position.Right} id="out" />
</div>
