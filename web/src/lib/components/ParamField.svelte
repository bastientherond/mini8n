<script lang="ts">
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import * as Select from '$lib/components/ui/select';
	import { Switch } from '$lib/components/ui/switch';
	import { Textarea } from '$lib/components/ui/textarea';
	import type { ParamSpec } from '$lib/engine/types';

	let {
		param,
		value,
		columns = [],
		onchange,
	}: {
		param: ParamSpec;
		value: unknown;
		columns?: string[];
		onchange: (value: unknown) => void;
	} = $props();

	const fieldId = $derived(`param-${param.name}`);
	const text = $derived(value == null ? '' : String(value));

	const jsonError = $derived.by(() => {
		if (param.type !== 'json' || !text.trim()) return null;
		try {
			JSON.parse(text);
			return null;
		} catch (e) {
			return e instanceof Error ? e.message : 'JSON invalide';
		}
	});

	/** Les listes de colonnes et les expressions proposent les colonnes en entrée. */
	const COLUMN_LISTS = new Set(['columns', 'by', 'group_by', 'left_key', 'right_key']);
	const acceptsColumns = $derived(param.type === 'expression' || COLUMN_LISTS.has(param.name));

	function insertColumn(name: string) {
		if (param.type === 'expression') {
			const ref = /^[\p{L}_$][\p{L}\p{N}_.$]*$/u.test(name) ? name : `\`${name}\``;
			onchange(text && !/\s$/.test(text) ? `${text} ${ref}` : text + ref);
		} else {
			onchange(text.trim() ? `${text.replace(/,\s*$/, '')}, ${name}` : name);
		}
	}
</script>

<div class="space-y-1.5">
	{#if param.type === 'boolean'}
		<div class="flex items-center justify-between gap-2">
			<Label for={fieldId}>{param.label}</Label>
			<Switch id={fieldId} checked={value === true} onCheckedChange={(v) => onchange(v)} />
		</div>
	{:else}
		<Label for={fieldId}>{param.label}</Label>
	{/if}

	{#if param.type === 'string'}
		<Input id={fieldId} value={text} placeholder={param.placeholder} oninput={(e) => onchange(e.currentTarget.value)} />
	{:else if param.type === 'number'}
		<Input
			id={fieldId}
			type="number"
			value={text}
			placeholder={param.placeholder}
			oninput={(e) => onchange(e.currentTarget.value === '' ? null : Number(e.currentTarget.value))}
		/>
	{:else if param.type === 'select'}
		<Select.Root type="single" value={text} onValueChange={(v) => onchange(v)}>
			<Select.Trigger id={fieldId} class="w-full">{text || 'Choisir…'}</Select.Trigger>
			<Select.Content>
				{#each param.options ?? [] as option (option)}
					<Select.Item value={option} label={option}>{option}</Select.Item>
				{/each}
			</Select.Content>
		</Select.Root>
	{:else if param.type !== 'boolean'}
		<Textarea
			id={fieldId}
			value={text}
			placeholder={param.placeholder}
			spellcheck={false}
			rows={param.type === 'json' ? 12 : param.type === 'expression' ? 3 : 5}
			class="font-mono text-xs leading-relaxed"
			aria-invalid={jsonError ? true : undefined}
			oninput={(e) => onchange(e.currentTarget.value)}
		/>
	{/if}

	{#if jsonError}
		<p class="text-destructive text-xs">{jsonError}</p>
	{/if}

	{#if acceptsColumns && columns.length > 0}
		<div class="flex flex-wrap gap-1">
			{#each columns as col (col)}
				<button
					type="button"
					class="bg-muted hover:bg-accent text-muted-foreground hover:text-foreground rounded px-1.5 py-0.5 font-mono text-[11px]"
					title="Insérer « {col} »"
					onclick={() => insertColumn(col)}>{col}</button
				>
			{/each}
		</div>
	{/if}

	{#if param.help}
		<p class="text-muted-foreground text-xs leading-snug">{param.help}</p>
	{/if}
</div>
