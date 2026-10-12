<script lang="ts">
	import { Handle, Position, type NodeProps } from '@xyflow/svelte';
	import CircleCheck from '@lucide/svelte/icons/circle-check';
	import CircleX from '@lucide/svelte/icons/circle-x';
	import CircleMinus from '@lucide/svelte/icons/circle-minus';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import Play from '@lucide/svelte/icons/play';
	import Settings2 from '@lucide/svelte/icons/settings-2';
	import { cn } from '$lib/utils';
	import { categoryAccent, formatCount, iconFor } from '$lib/icons';
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
		'group bg-card text-card-foreground relative w-56 rounded-xl border shadow-sm transition-[box-shadow,border-color]',
		spec?.category === 'trigger' && 'rounded-l-[28px]',
		selected && 'border-primary ring-primary/20 ring-4',
		result?.status === 'error' && 'border-destructive',
		data.disabled && 'opacity-50',
	)}
>
	<div class={cn('flex items-center gap-2.5 p-3', spec?.category === 'trigger' && 'pl-4')}>
		<div class={cn('flex size-9 shrink-0 items-center justify-center rounded-lg', categoryAccent[spec?.category ?? 'transform'])}>
			<Icon class="size-[18px]" />
		</div>
		<div class="min-w-0 flex-1">
			<div class="truncate text-sm leading-tight font-medium">{data.name}</div>
			<div class="text-muted-foreground truncate text-xs">{spec?.label ?? data.kind}</div>
		</div>
		<button
			class="nodrag text-muted-foreground hover:text-foreground hover:bg-accent hidden size-7 items-center justify-center rounded-md group-hover:flex"
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
			class="nodrag text-muted-foreground hover:text-foreground hover:bg-accent -mr-1 hidden size-7 items-center justify-center rounded-md group-hover:flex"
			title="Exécuter jusqu'ici"
			disabled={wf.running}
			onclick={(e) => {
				e.stopPropagation();
				wf.run(id);
			}}
		>
			<Play class="size-3.5" />
		</button>
	</div>

	<div class="text-muted-foreground flex h-7 items-center gap-1.5 border-t px-3 text-xs">
		{#if running}
			<LoaderCircle class="size-3.5 animate-spin" />
			<span>Exécution…</span>
		{:else if result?.status === 'success'}
			<CircleCheck class="size-3.5 text-emerald-500" />
			<span>{formatCount(result.rows)} ligne{result.rows > 1 ? 's' : ''} · {result.columns.length} col.</span>
		{:else if result?.status === 'error'}
			<CircleX class="text-destructive size-3.5" />
			<span class="text-destructive truncate" title={result.error}>{result.error}</span>
		{:else if result?.status === 'skipped'}
			<CircleMinus class="size-3.5" />
			<span>Ignoré</span>
		{:else}
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
