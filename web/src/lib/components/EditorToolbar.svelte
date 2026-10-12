<script lang="ts">
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import Cloud from '@lucide/svelte/icons/cloud';
	import Copy from '@lucide/svelte/icons/copy';
	import Download from '@lucide/svelte/icons/download';
	import Ellipsis from '@lucide/svelte/icons/ellipsis';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import Play from '@lucide/svelte/icons/play';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { Input } from '$lib/components/ui/input';
	import { attempt, duplicateWorkflow, exportWorkflow, ui } from '$lib/ui.svelte';
	import { wf } from '$lib/workflow.svelte';

	let { onrun }: { onrun: () => void } = $props();

	const summary = $derived.by(() => {
		const run = wf.lastRun;
		if (!run) return null;
		const errors = Object.values(run.results).filter((r) => r.status === 'error').length;
		return { ok: run.ok && !run.error, errors, ms: run.durationMs };
	});
</script>

<div class="bg-background flex h-11 shrink-0 items-center gap-2 border-b px-3">
	<Input
		bind:value={wf.name}
		class="h-8 w-72 border-transparent px-1.5 font-medium shadow-none hover:border-input"
		maxlength={120}
		aria-label="Nom du workflow"
		disabled={!wf.workflowId}
	/>
	<span class="text-muted-foreground flex items-center gap-1 text-xs" aria-live="polite">
		{#if wf.saveState === 'saving'}
			<LoaderCircle class="size-3.5 animate-spin" /> Enregistrement…
		{:else if wf.saveState === 'saved'}
			<Cloud class="size-3.5" /> Enregistré
		{:else if wf.saveState === 'error'}
			<CircleAlert class="text-destructive size-3.5" />
			<span class="text-destructive" title={wf.saveError}>Non enregistré</span>
		{:else if wf.saveState === 'conflict'}
			<CircleAlert class="text-destructive size-3.5" />
			<span class="text-destructive">Modifié ailleurs</span>
			<Button variant="link" size="sm" class="h-auto p-0 text-xs" onclick={() => attempt(() => wf.reloadCurrent(), 'Workflow rechargé')}>
				Recharger
			</Button>
		{/if}
	</span>

	<div class="ml-auto flex items-center gap-2">
		{#if summary && !wf.running}
			{#if summary.ok}
				<Badge variant="secondary">Succès · {summary.ms.toFixed(0)} ms</Badge>
			{:else}
				<Badge variant="destructive">{summary.errors || 1} erreur{summary.errors > 1 ? 's' : ''}</Badge>
			{/if}
		{/if}
		<DropdownMenu.Root>
			<DropdownMenu.Trigger>
				{#snippet child({ props })}
					<Button {...props} variant="outline" size="icon-sm" aria-label="Actions du workflow" disabled={!wf.workflowId}>
						<Ellipsis />
					</Button>
				{/snippet}
			</DropdownMenu.Trigger>
			<DropdownMenu.Content align="end" class="w-52">
				<DropdownMenu.Item onclick={() => wf.workflowId && duplicateWorkflow(wf.workflowId)}><Copy /> Dupliquer</DropdownMenu.Item>
				<DropdownMenu.Item onclick={() => wf.workflowId && exportWorkflow(wf.workflowId)}><Download /> Exporter (.json)</DropdownMenu.Item>
				<DropdownMenu.Separator />
				<DropdownMenu.Item
					variant="destructive"
					onclick={() => wf.workflowId && (ui.confirmDelete = { id: wf.workflowId, name: wf.name })}
				>
					<Trash2 /> Supprimer
				</DropdownMenu.Item>
			</DropdownMenu.Content>
		</DropdownMenu.Root>
		<Button size="sm" onclick={onrun} disabled={wf.running || !wf.workflowId} title="⌘/Ctrl + Entrée">
			{#if wf.running}<LoaderCircle class="animate-spin" />{:else}<Play />{/if}
			Exécuter
		</Button>
	</div>
</div>
