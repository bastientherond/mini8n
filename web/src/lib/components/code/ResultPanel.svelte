<script lang="ts">
	// Résultat d'une expression évaluée sur une ligne de l'entrée, avec navigation entre lignes.
	import ChevronLeft from '@lucide/svelte/icons/chevron-left';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import Hourglass from '@lucide/svelte/icons/hourglass';
	import type { ExprContext } from '$lib/expressions/context';
	import { cn } from '$lib/utils';
	import ResultValue from './ResultValue.svelte';
	import { type TrialRunner, waitingForInput } from './useTrial.svelte';

	let {
		trial,
		context,
		expanded = false,
		class: className = '',
	}: { trial: TrialRunner; context: ExprContext; expanded?: boolean; class?: string } = $props();

	const r = $derived(trial.result);
	const rows = $derived(r?.rows ?? context.input?.rows ?? 0);
	const waiting = $derived(waitingForInput(r, context));
</script>

{#if r}
	<div
		class={cn(
			'rounded-lg border px-2.5 py-2 text-xs',
			r.ok || waiting ? 'bg-muted/50' : 'border-destructive/30 bg-destructive/5',
			className,
		)}
		aria-live="polite"
	>
		<div class="mb-1 flex items-center gap-2">
			<span class="text-muted-foreground text-[10px] font-semibold tracking-wide uppercase">Résultat</span>
			{#if rows > 1 && !waiting}
				<div class="text-muted-foreground ml-auto flex items-center gap-0.5 tabular-nums">
					<button
						type="button"
						class="hover:bg-accent hover:text-foreground rounded p-0.5 disabled:opacity-40"
						disabled={trial.row <= 0}
						onclick={() => trial.row--}
						aria-label="Ligne précédente"><ChevronLeft class="size-3.5" /></button
					>
					<span>Ligne {Math.min(trial.row, rows - 1) + 1} / {rows}</span>
					<button
						type="button"
						class="hover:bg-accent hover:text-foreground rounded p-0.5 disabled:opacity-40"
						disabled={trial.row >= rows - 1}
						onclick={() => trial.row++}
						aria-label="Ligne suivante"><ChevronRight class="size-3.5" /></button
					>
				</div>
			{/if}
		</div>
		{#if waiting}
			<p class="text-muted-foreground flex items-center gap-1.5">
				<Hourglass class="size-3.5 shrink-0" /> Exécutez les nœuds précédents pour voir le résultat.
			</p>
		{:else if r.ok}
			<ResultValue value={r.value} kind={r.kind} {expanded} />
		{:else}
			<p class="text-destructive flex items-start gap-1.5 leading-snug">
				<CircleAlert class="mt-px size-3.5 shrink-0" />
				<span>{r.error}</span>
			</p>
		{/if}
	</div>
{/if}
