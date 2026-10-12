<script lang="ts">
	// Explorateur des données et fonctions disponibles : un clic insère la référence
	// dans l'éditeur, les colonnes peuvent aussi être glissées.
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Search from '@lucide/svelte/icons/search';
	import { docs } from '$lib/expressions/completion';
	import { type ExprContext, type Table, columnRef, kindLabels, kindOf, sampleRow, shortValue } from '$lib/expressions/context';
	import type { ExprDoc } from '$lib/engine/types';
	import { cn } from '$lib/utils';
	import { FIELD_MIME } from './CodeEditor.svelte';

	let {
		context,
		onref,
		oninsert,
	}: {
		context: ExprContext;
		/** Référence de donnée (`$json.prix`) : placée entre `{{ }}` si besoin. */
		onref: (ref: string) => void;
		/** Texte brut (méthode, fonction). */
		oninsert: (text: string) => void;
	} = $props();

	let query = $state('');
	let open = $state<Record<string, boolean>>({ input: true, vars: true });
	let samples = $state<Record<string, Record<string, unknown>>>({});

	$effect(() => {
		for (const t of [context.input, ...context.nodes]) {
			if (!t?.bytes || samples[t.name]) continue;
			const name = t.name;
			sampleRow(t.bytes).then((row) => (samples = { ...samples, [name]: row }));
		}
	});

	const nodeBase = (name: string) => `$(${name.includes("'") ? JSON.stringify(name) : `'${name}'`}).item.json`;

	const GROUPS: [string, string][] = [
		['string', 'Texte'],
		['number', 'Nombre'],
		['list', 'Liste'],
		['date', 'Date'],
		['object', 'Objet'],
		['Math', 'Math'],
		['JSON', 'JSON'],
		['global', 'Fonctions globales'],
	];

	const q = $derived(query.trim().toLowerCase());
	const matches = (...texts: string[]) => !q || texts.some((t) => t.toLowerCase().includes(q));

	function insertDoc(d: ExprDoc) {
		const call = d.kind === 'method' || d.kind === 'function' ? '()' : '';
		if (d.on === 'Math' || d.on === 'JSON' || d.on === 'Object') oninsert(`${d.on}.${d.name}${call}`);
		else if (d.on === 'global') onref(d.name === '$' ? "$('')" : `${d.name}${d.kind === 'function' ? '()' : ''}`);
		else oninsert(`.${d.name}${call}`);
	}

	function dragStart(event: DragEvent, ref: string) {
		event.dataTransfer?.setData(FIELD_MIME, ref);
		event.dataTransfer?.setData('text/plain', ref);
	}
</script>

{#snippet columns(table: Table, base: string)}
	{#each table.columns.filter((c) => matches(c.name)) as c (c.name)}
		{@const ref = columnRef(c.name, base)}
		{@const sample = samples[table.name]?.[c.name]}
		<button
			type="button"
			draggable="true"
			ondragstart={(e) => dragStart(e, ref)}
			onclick={() => onref(ref)}
			class="hover:bg-accent group flex w-full items-center gap-2 rounded-md px-2 py-1 text-left"
			title="Insérer {ref}"
		>
			<span class="truncate font-mono text-xs text-[var(--code-property)]">{c.name}</span>
			<span class="text-muted-foreground ml-auto max-w-[45%] truncate font-mono text-[10px]">
				{sample !== undefined ? shortValue(sample, 30) : kindLabels[kindOf(c.type)]}
			</span>
		</button>
	{:else}
		<p class="text-muted-foreground px-2 py-1 text-xs">{table.columns.length ? 'Aucune colonne correspondante.' : 'Pas encore de données.'}</p>
	{/each}
{/snippet}

{#snippet section(key: string, title: string, count: number | null, body: import('svelte').Snippet)}
	<div>
		<button
			type="button"
			class="hover:bg-accent/60 flex w-full items-center gap-1.5 rounded-md px-1.5 py-1 text-xs font-semibold"
			onclick={() => (open = { ...open, [key]: !(open[key] || !!q) })}
			aria-expanded={open[key] || !!q}
		>
			<ChevronRight class={cn('size-3.5 transition-transform', (open[key] || q) && 'rotate-90')} />
			<span class="truncate">{title}</span>
			{#if count !== null}<span class="text-muted-foreground ml-auto font-normal">{count}</span>{/if}
		</button>
		{#if open[key] || q}
			<div class="mt-0.5 mb-1.5 ml-3 border-l pl-1.5">{@render body()}</div>
		{/if}
	</div>
{/snippet}

<div class="flex h-full min-h-0 flex-col">
	<div class="relative border-b p-2">
		<Search class="text-muted-foreground absolute top-1/2 left-4 size-3.5 -translate-y-1/2" />
		<input
			bind:value={query}
			placeholder="Rechercher une colonne, une méthode…"
			class="bg-muted/60 focus-visible:ring-ring/50 h-8 w-full rounded-md pr-2 pl-7 text-xs outline-none focus-visible:ring-2"
		/>
	</div>
	<div class="min-h-0 flex-1 space-y-1 overflow-y-auto p-2">
		{#if context.input}
			{@const input = context.input}
			{#snippet inputBody()}{@render columns(input, '$json')}{/snippet}
			{@render section('input', `Entrée · ${input.name}`, input.columns.length, inputBody)}
		{/if}

		{#snippet varsBody()}
			{#each [['$now', 'Date et heure actuelles'], ['$today', "Aujourd'hui à minuit"], ['$index', 'Position de la ligne'], ...context.vars.map(([n, v]) => [n, shortValue(v, 30)])].filter(([n, i]) => matches(n, i)) as [name, info] (name)}
				<button
					type="button"
					class="hover:bg-accent flex w-full items-center gap-2 rounded-md px-2 py-1 text-left"
					onclick={() => onref(name)}
				>
					<span class="font-mono text-xs font-semibold text-[var(--code-special)]">{name}</span>
					<span class="text-muted-foreground ml-auto truncate text-[10px]">{info}</span>
				</button>
			{/each}
		{/snippet}
		{@render section('vars', 'Variables', null, varsBody)}

		{#if context.allowNodes}
			{#each context.nodes as node (node.name)}
				{#snippet nodeBody()}{@render columns(node, nodeBase(node.name))}{/snippet}
				{@render section(`node:${node.name}`, `$('${node.name}')`, node.columns.length, nodeBody)}
			{/each}
		{/if}

		{#each GROUPS as [on, title] (on)}
			{@const items = docs().filter((d) => d.on === on && d.name !== '$json' && d.name !== '$input' && matches(d.name, d.info))}
			{#if items.length}
				{#snippet docsBody()}
					{#each items as d (d.kind + d.name)}
						<button
							type="button"
							class="hover:bg-accent flex w-full flex-col rounded-md px-2 py-1 text-left"
							onclick={() => insertDoc(d)}
							title={d.example}
						>
							<span class="font-mono text-xs">
								<span class="text-[var(--code-function)]">{d.kind === 'method' || d.kind === 'property' ? '.' : ''}{d.name}</span><span
									class="text-muted-foreground">{d.args ?? ''}</span
								>
							</span>
							<span class="text-muted-foreground text-[11px] leading-snug">{d.info}</span>
						</button>
					{/each}
				{/snippet}
				{@render section(`docs:${on}`, title, items.length, docsBody)}
			{/if}
		{/each}
	</div>
</div>
