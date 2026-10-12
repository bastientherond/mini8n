<script lang="ts">
	// Éditeur d'expression agrandi : données disponibles | éditeur | résultat.
	import Braces from '@lucide/svelte/icons/braces';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import type { ExprContext } from '$lib/expressions/context';
	import CodeEditor, { type EditorApi } from './CodeEditor.svelte';
	import ResultPanel from './ResultPanel.svelte';
	import VariablesPanel from './VariablesPanel.svelte';
	import { TrialRunner, waitingForInput } from './useTrial.svelte';

	let {
		open = $bindable(false),
		title,
		value,
		onchange,
		template,
		context,
	}: {
		open?: boolean;
		title: string;
		value: string;
		onchange: (v: string) => void;
		template: boolean;
		context: ExprContext;
	} = $props();

	let api: EditorApi | null = $state(null);
	const trial = new TrialRunner();

	$effect(() => {
		if (!open) return;
		void trial.row;
		trial.run(value, template, context, 80);
	});
	$effect(() => () => trial.dispose());

	const diagnostics = $derived(
		trial.result && !trial.result.ok && trial.result.from !== undefined && !waitingForInput(trial.result, context)
			? [{ from: trial.result.from, to: trial.result.to ?? trial.result.from, severity: 'error' as const, message: trial.result.error ?? '' }]
			: [],
	);
</script>

<Dialog.Root bind:open>
	<Dialog.Content
		onOpenAutoFocus={(e) => {
			// Le curseur va directement dans l'éditeur.
			e.preventDefault();
			setTimeout(() => api?.focus());
		}}
		class="flex h-[min(860px,calc(100dvh-2rem))] w-[min(1280px,calc(100vw-2rem))] max-w-none flex-col gap-0 overflow-hidden p-0 sm:max-w-none">
		<Dialog.Header class="flex-row items-center gap-3 border-b px-4 py-3">
			<span class="bg-primary/10 text-primary flex size-8 items-center justify-center rounded-lg"><Braces class="size-4" /></span>
			<div class="min-w-0">
				<Dialog.Title class="text-base">{title}</Dialog.Title>
				<Dialog.Description class="text-xs">
					{template
						? 'Texte libre avec des expressions entre {{ … }}.'
						: 'Expression JavaScript évaluée pour chaque ligne.'}
					Ctrl+Espace : suggestions · glissez une colonne dans l'éditeur.
				</Dialog.Description>
			</div>
		</Dialog.Header>
		<div class="grid min-h-0 flex-1 grid-cols-1 md:grid-cols-[280px_minmax(0,1fr)] lg:grid-cols-[300px_minmax(0,1fr)_minmax(280px,380px)]">
			<aside class="bg-sidebar hidden min-h-0 border-r md:block" aria-label="Données disponibles">
				<VariablesPanel {context} onref={(r) => api?.insertRef(r)} oninsert={(t) => api?.insert(t)} />
			</aside>
			<section class="flex min-h-0 flex-col p-4" aria-label="Éditeur">
				<CodeEditor
					{value}
					{onchange}
					mode={template ? 'template' : 'expression'}
					{context}
					{diagnostics}
					autofocus
					ariaLabel={title}
					class="mini8n-code-large min-h-0 flex-1 overflow-auto"
					onready={(a) => (api = a)}
				/>
				<ResultPanel {trial} {context} class="mt-3 lg:hidden" />
			</section>
			<aside class="hidden min-h-0 flex-col border-l lg:flex" aria-label="Résultat">
				<div class="min-h-0 flex-1 overflow-y-auto p-4">
					<ResultPanel {trial} {context} expanded />
					{#if !trial.result}
						<p class="text-muted-foreground text-xs">Le résultat s'affiche ici pendant la saisie.</p>
					{/if}
				</div>
			</aside>
		</div>
		<div class="flex justify-end border-t px-4 py-2.5">
			<Button size="sm" onclick={() => (open = false)}>Terminé</Button>
		</div>
	</Dialog.Content>
</Dialog.Root>
