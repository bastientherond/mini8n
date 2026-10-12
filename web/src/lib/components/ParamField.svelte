<script lang="ts">
	import Copy from '@lucide/svelte/icons/copy';
	import ExternalLink from '@lucide/svelte/icons/external-link';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import { toast } from 'svelte-sonner';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import * as Select from '$lib/components/ui/select';
	import { Switch } from '$lib/components/ui/switch';
	import { Textarea } from '$lib/components/ui/textarea';
	import { newToken } from '$lib/engine/convert';
	import type { ParamSpec } from '$lib/engine/types';
	import { wf } from '$lib/workflow.svelte';

	let {
		param,
		value,
		columns = [],
		nodeKind = '',
		onchange,
	}: {
		param: ParamSpec;
		value: unknown;
		columns?: string[];
		nodeKind?: string;
		onchange: (value: unknown) => void;
	} = $props();

	/** URL publique d'un jeton (webhook : API ; formulaire : page de l'application). */
	const publicUrl = $derived.by(() => {
		if (param.type !== 'token' || !value) return '';
		const token = encodeURIComponent(String(value));
		return nodeKind === 'trigger_form' ? `${location.origin}/#/f/${token}` : `${location.origin}/api/hooks/${token}`;
	});

	const otherWorkflows = $derived(wf.workflows.filter((w) => w.id !== wf.workflowId));

	async function copy(text: string) {
		try {
			await navigator.clipboard.writeText(text);
			toast.success('URL copiée');
		} catch {
			toast.error('Copie impossible');
		}
	}

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
	{:else if param.type === 'token'}
		<div class="flex gap-1">
			<Input id={fieldId} value={publicUrl} readonly class="font-mono text-xs" onfocus={(e) => e.currentTarget.select()} />
			<Button variant="outline" size="icon" onclick={() => copy(publicUrl)} aria-label="Copier l'URL" title="Copier" disabled={!publicUrl}>
				<Copy />
			</Button>
			{#if nodeKind === 'trigger_form' && publicUrl}
				<Button variant="outline" size="icon" href={publicUrl} target="_blank" rel="noopener" aria-label="Ouvrir le formulaire" title="Ouvrir">
					<ExternalLink />
				</Button>
			{/if}
			<Button
				variant="outline"
				size="icon"
				onclick={() => onchange(newToken())}
				aria-label="Régénérer l'URL"
				title="Régénérer (l'ancienne URL cessera de fonctionner)"
			>
				<RefreshCw />
			</Button>
		</div>
	{:else if param.type === 'workflow'}
		<Select.Root type="single" value={text} onValueChange={(v) => onchange(v)}>
			<Select.Trigger id={fieldId} class="w-full">
				{wf.workflows.find((w) => w.id === text)?.name ?? (text ? 'Workflow introuvable' : 'Choisir un workflow…')}
			</Select.Trigger>
			<Select.Content>
				{#each otherWorkflows as item (item.id)}
					<Select.Item value={item.id} label={item.name}>{item.name}</Select.Item>
				{:else}
					<p class="text-muted-foreground px-2 py-1.5 text-sm">Aucun autre workflow dans ce workspace.</p>
				{/each}
			</Select.Content>
		</Select.Root>
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
