<script lang="ts">
	// Champ de paramètre en code : expression JavaScript, ou texte avec des `{{ … }}`.
	// Autocomplétion, colonnes glissables, résultat en direct et éditeur agrandi.
	import Maximize2 from '@lucide/svelte/icons/maximize-2';
	import type { ExprContext } from '$lib/expressions/context';
	import { columnRef } from '$lib/expressions/context';
	import { cn } from '$lib/utils';
	import CodeEditor, { type EditorApi } from './CodeEditor.svelte';
	import ExpressionDialog from './ExpressionDialog.svelte';
	import ResultPanel from './ResultPanel.svelte';
	import { TrialRunner, waitingForInput } from './useTrial.svelte';

	let {
		id,
		label,
		value,
		onchange,
		template,
		singleLine = false,
		placeholder = '',
		context,
	}: {
		id: string;
		label: string;
		value: string;
		onchange: (v: string) => void;
		template: boolean;
		singleLine?: boolean;
		placeholder?: string;
		context: ExprContext;
	} = $props();

	let api: EditorApi | null = $state(null);
	let focused = $state(false);
	let expanded = $state(false);
	const trial = new TrialRunner();

	$effect(() => {
		void trial.row;
		trial.run(value, template, context);
	});
	$effect(() => () => trial.dispose());

	const hasCode = $derived(template ? value.includes('{{') : value.trim().length > 0);
	const diagnostics = $derived(
		trial.result && !trial.result.ok && trial.result.from !== undefined && !waitingForInput(trial.result, context)
			? [{ from: trial.result.from, to: trial.result.to ?? trial.result.from, severity: 'error' as const, message: trial.result.error ?? '' }]
			: [],
	);
	const columns = $derived(context.input?.columns ?? []);
</script>

<div class="space-y-1.5">
	<div class="group/field relative">
		<CodeEditor
			{id}
			{value}
			{onchange}
			mode={template ? 'template' : 'expression'}
			{singleLine}
			{placeholder}
			{context}
			{diagnostics}
			ariaLabel={label}
			class={cn('pr-8', !singleLine && 'mini8n-code-medium')}
			onready={(a) => (api = a)}
			onfocuschange={(f) => (focused = f)}
		/>
		<button
			type="button"
			class="text-muted-foreground hover:bg-accent hover:text-foreground absolute top-1 right-1 rounded-md p-1 opacity-60 group-hover/field:opacity-100 focus-visible:opacity-100"
			onclick={() => (expanded = true)}
			title="Agrandir l'éditeur"
			aria-label="Agrandir l'éditeur de {label}"
		>
			<Maximize2 class="size-3.5" />
		</button>
	</div>

	{#if columns.length > 0 && focused}
		<div class="flex flex-wrap items-center gap-1">
			{#each columns.slice(0, 16) as col (col.name)}
				<button
					type="button"
					class="bg-muted hover:bg-accent text-muted-foreground hover:text-foreground rounded px-1.5 py-0.5 font-mono text-[11px]"
					title="Insérer {columnRef(col.name)}"
					onmousedown={(e) => e.preventDefault()}
					onclick={() => api?.insertRef(columnRef(col.name))}>{col.name}</button
				>
			{/each}
			{#if columns.length > 16}<span class="text-muted-foreground text-[11px]">+{columns.length - 16}</span>{/if}
		</div>
	{/if}

	{#if hasCode && (focused || !waitingForInput(trial.result, context))}
		<ResultPanel {trial} {context} />
	{:else if template && focused}
		<p class="text-muted-foreground text-[11px]">Tapez <code class="bg-muted rounded px-1">{'{{'}</code> pour insérer une expression.</p>
	{/if}
</div>

<ExpressionDialog bind:open={expanded} title={label} {value} {onchange} {template} {context} />
