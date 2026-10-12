<script lang="ts">
	// Affichage d'une valeur produite par une expression.
	import { kindLabels } from '$lib/expressions/context';
	import { cn } from '$lib/utils';

	let { value, kind, expanded = false }: { value: unknown; kind?: string; expanded?: boolean } = $props();

	const text = $derived.by(() => {
		if (value === null || value === undefined) return 'null';
		if (typeof value === 'string') return value;
		if (typeof value === 'object') return JSON.stringify(value, null, expanded ? 2 : 0);
		if (typeof value === 'number') return Number.isInteger(value) ? String(value) : String(value);
		return String(value);
	});
	const muted = $derived(value === null || value === undefined || value === '');
</script>

<div class="flex min-w-0 items-start gap-2">
	<pre
		class={cn(
			'min-w-0 flex-1 font-mono text-xs leading-relaxed break-words whitespace-pre-wrap',
			!expanded && 'line-clamp-4',
			muted && 'text-muted-foreground italic',
			kind === 'number' && 'text-[var(--code-number)]',
			kind === 'boolean' && 'text-[var(--code-number)]',
		)}>{value === '' ? '(texte vide)' : text}</pre>
	{#if kind && kind !== 'null'}
		<span class="bg-background text-muted-foreground shrink-0 rounded border px-1.5 py-px text-[10px] leading-4">
			{kindLabels[kind] ?? kind}
		</span>
	{/if}
</div>
