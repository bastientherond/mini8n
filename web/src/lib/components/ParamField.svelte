<script lang="ts">
	import Copy from '@lucide/svelte/icons/copy';
	import ExternalLink from '@lucide/svelte/icons/external-link';
	import Plus from '@lucide/svelte/icons/plus';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Upload from '@lucide/svelte/icons/upload';
	import { toast } from 'svelte-sonner';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import * as Select from '$lib/components/ui/select';
	import { Switch } from '$lib/components/ui/switch';
	import { Textarea } from '$lib/components/ui/textarea';
	import { api, errorMessage } from '$lib/api';
	import { brandOfCredential } from '$lib/brands';
	import BrandLogo from './BrandLogo.svelte';
	import { newToken } from '$lib/engine/convert';
	import { formatBytes } from '$lib/icons';
	import { resources } from '$lib/resources.svelte';
	import CredentialDialog from './CredentialDialog.svelte';
	import CodeEditor from './code/CodeEditor.svelte';
	import ExpressionField from './code/ExpressionField.svelte';
	import { buildContext, emptyContext } from '$lib/expressions/context';
	import type { ParamSpec } from '$lib/engine/types';
	import { wf } from '$lib/workflow.svelte';

	let {
		param,
		value,
		columns = [],
		nodeId = '',
		nodeKind = '',
		onchange,
	}: {
		param: ParamSpec;
		value: unknown;
		columns?: string[];
		nodeId?: string;
		nodeKind?: string;
		onchange: (value: unknown) => void;
	} = $props();

	/** Champ en code : expression, texte à `{{ … }}`, SQL ou JSON. */
	const codeKind = $derived(
		param.type === 'expression'
			? 'expression'
			: param.template && (param.type === 'string' || param.type === 'text')
				? 'template'
				: param.type === 'sql' || param.type === 'json'
					? param.type
					: null,
	);
	const context = $derived(codeKind === 'expression' || codeKind === 'template' ? buildContext(nodeId, param) : emptyContext);

	/** URL publique d'un jeton (webhook : API ; formulaire : page de l'application). */
	const publicUrl = $derived.by(() => {
		if (param.type !== 'token' || !value) return '';
		const token = encodeURIComponent(String(value));
		return nodeKind === 'trigger_form' ? `${location.origin}/#/f/${token}` : `${location.origin}/api/hooks/${token}`;
	});

	const otherWorkflows = $derived(wf.workflows.filter((w) => w.id !== wf.workflowId));
	const credentials = $derived(resources.credentials.filter((c) => c.kind === param.credential));
	let credentialDialog = $state(false);
	let uploading = $state(false);

	async function upload(event: Event) {
		const input = event.currentTarget as HTMLInputElement;
		const file = input.files?.[0];
		input.value = '';
		if (!file || !resources.workspaceId) return;
		uploading = true;
		try {
			const saved = await api.uploadFile(resources.workspaceId, file);
			await resources.refreshFiles();
			onchange(saved.id);
			toast.success(`« ${saved.name} » téléversé`);
		} catch (e) {
			toast.error(errorMessage(e));
		} finally {
			uploading = false;
		}
	}

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

	/** Les listes de colonnes proposent les colonnes en entrée. */
	const COLUMN_LISTS = new Set(['columns', 'by', 'group_by', 'left_key', 'right_key']);
	const acceptsColumns = $derived(COLUMN_LISTS.has(param.name));

	function insertColumn(name: string) {
		onchange(text.trim() ? `${text.replace(/,\s*$/, '')}, ${name}` : name);
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

	{#if codeKind === 'expression' || codeKind === 'template'}
		<ExpressionField
			id={fieldId}
			label={param.label}
			value={text}
			onchange={(v) => onchange(v)}
			template={codeKind === 'template'}
			singleLine={param.type === 'string'}
			placeholder={param.placeholder}
			{context}
		/>
	{:else if codeKind === 'sql' || codeKind === 'json'}
		<CodeEditor
			id={fieldId}
			value={text}
			onchange={(v) => onchange(v)}
			mode={codeKind}
			placeholder={param.placeholder}
			ariaLabel={param.label}
			class={codeKind === 'json' ? 'mini8n-code-tall' : 'mini8n-code-medium'}
		/>
	{:else if param.type === 'string'}
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
	{:else if param.type === 'credential'}
		<div class="flex gap-1">
			<Select.Root type="single" value={text} onValueChange={(v) => onchange(v)}>
				<Select.Trigger id={fieldId} class="min-w-0 flex-1">
					{@const current = credentials.find((c) => c.id === text)}
					{#if current}<BrandLogo brand={brandOfCredential(current.kind, current.summary)} class="size-5 rounded" />{/if}
					<span class="truncate">{current?.name ?? (text ? 'Identifiant introuvable' : 'Choisir un identifiant…')}</span>
				</Select.Trigger>
				<Select.Content>
					{#each credentials as c (c.id)}
						<Select.Item value={c.id} label={c.name}>
							<BrandLogo brand={brandOfCredential(c.kind, c.summary)} class="size-5 rounded" />
							<span class="truncate">{c.name}</span>
							{#if c.summary.host}<span class="text-muted-foreground ml-auto text-xs">{c.summary.host}</span>{/if}
						</Select.Item>
					{:else}
						<p class="text-muted-foreground px-2 py-1.5 text-sm">Aucun identifiant de ce type.</p>
					{/each}
				</Select.Content>
			</Select.Root>
			<Button variant="outline" size="icon" onclick={() => (credentialDialog = true)} aria-label="Nouvel identifiant" title="Nouvel identifiant">
				<Plus />
			</Button>
		</div>
		<CredentialDialog bind:open={credentialDialog} kind={param.credential} onsaved={(c) => onchange(c.id)} />
	{:else if param.type === 'file'}
		<div class="flex gap-1">
			<Select.Root type="single" value={text} onValueChange={(v) => onchange(v)}>
				<Select.Trigger id={fieldId} class="min-w-0 flex-1">
					{resources.files.find((f) => f.id === text)?.name ?? (text ? 'Fichier introuvable' : 'Choisir un fichier…')}
				</Select.Trigger>
				<Select.Content>
					{#each resources.files as f (f.id)}
						<Select.Item value={f.id} label={f.name}>
							<span class="truncate">{f.name}</span>
							<span class="text-muted-foreground ml-auto text-xs">{formatBytes(f.size)}</span>
						</Select.Item>
					{:else}
						<p class="text-muted-foreground px-2 py-1.5 text-sm">Aucun fichier : téléversez-en un.</p>
					{/each}
				</Select.Content>
			</Select.Root>
			<label
				class="border-input hover:bg-accent flex size-8 shrink-0 cursor-pointer items-center justify-center rounded-lg border"
				title="Téléverser un fichier"
			>
				<Upload class="size-4" />
				<input type="file" class="sr-only" accept=".csv,.json,.parquet,.xlsx,.xlsm,.xls,.ods,.txt" disabled={uploading} onchange={upload} />
			</label>
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
