<script lang="ts">
	import CircleCheck from '@lucide/svelte/icons/circle-check';
	import CircleX from '@lucide/svelte/icons/circle-x';
	import CircleMinus from '@lucide/svelte/icons/circle-minus';
	import Clock from '@lucide/svelte/icons/clock';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import { cn } from '$lib/utils';
	import { brandOfNode } from '$lib/brands';
	import { categoryAccent, categoryBar, formatCount, iconFor } from '$lib/icons';
	import { wf, type StepData } from '$lib/workflow.svelte';
	import BrandLogo from './BrandLogo.svelte';

	let {
		id,
		data,
		selected = false,
		connecting = false,
		hoverInput = null,
	}: {
		id: string;
		data: StepData;
		selected?: boolean;
		/** Un lien est en cours de tracé depuis une sortie : les entrées sont mises en avant. */
		connecting?: boolean;
		/** Entrée survolée pendant le tracé. */
		hoverInput?: number | null;
	} = $props();

	const spec = $derived(wf.spec(data.kind));
	const result = $derived(wf.results[id]);
	/** En cours d'exécution, ou en attente de son tour. */
	const running = $derived(wf.activeId === id);
	const queued = $derived(!running && wf.runningIds.has(id));
	const Icon = $derived(iconFor(spec?.icon));
	const inputs = $derived(spec?.inputs ?? []);
	/** Logo du service pour les nœuds d'intégration (base, e-mail, messagerie). */
	const brand = $derived(brandOfNode(data.kind, data.params.service));
</script>

<div
	ondblclick={() => (wf.openNodeId = id)}
	role="group"
	title="Cliquez pour sélectionner, double-cliquez pour paramétrer"
	class={cn(
		'group bg-card text-card-foreground relative w-60 rounded-xl border shadow-sm transition-[box-shadow,border-color,transform] hover:shadow-md',
		spec?.category === 'trigger' && 'rounded-l-[30px]',
		selected && 'border-primary/70 ring-primary/20 shadow-md ring-4',
		result?.status === 'error' && 'border-destructive/70 ring-destructive/10 ring-4',
		running && 'mini8n-node-running',
		queued && 'opacity-80',
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
		{#if brand}
			<BrandLogo {brand} class="size-10 rounded-xl" />
		{:else}
			<div
				class={cn(
					'flex size-10 shrink-0 items-center justify-center rounded-xl',
					categoryAccent[spec?.category ?? 'transform'],
					spec?.category === 'trigger' && 'rounded-full',
				)}
			>
				<Icon class="size-5" />
			</div>
		{/if}
		<div class="min-w-0 flex-1">
			<div class="truncate text-sm leading-tight font-semibold">{data.name}</div>
			<div class="text-muted-foreground mt-0.5 truncate text-xs">{spec?.label ?? data.kind}</div>
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
			<span class="text-primary font-medium">Exécution…</span>
		{:else if queued}
			<Clock class="size-3.5" />
			<span>En attente…</span>
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

	<!-- Ports : entrées à gauche, sortie à droite (le canevas gère les glisser-déposer). -->
	{#each inputs as label, i (i)}
		{@const top = ((i + 1) / (inputs.length + 1)) * 100}
		<span
			class={cn('mini8n-port -left-[7px]', connecting && 'mini8n-port-target', hoverInput === i && 'mini8n-port-hover')}
			style="top: {top}%"
			data-port="in-{i}"
			data-node={id}
			title={inputs.length > 1 ? `Entrée ${label}` : spec?.optionalInput ? 'Entrée facultative' : 'Entrée'}
		></span>
		{#if inputs.length > 1}
			<!-- Au-dessus de l'arrivée du lien, pour ne pas le masquer. -->
			<span
				class="text-muted-foreground pointer-events-none absolute -left-4 -translate-x-full text-[10px] leading-none font-semibold"
				style="top: calc({top}% - 13px)">{label}</span
			>
		{/if}
	{/each}
	<span class="mini8n-port -right-[7px]" style="top: 50%" data-port="out" data-node={id} title="Tirez pour relier à un autre nœud"></span>
</div>
