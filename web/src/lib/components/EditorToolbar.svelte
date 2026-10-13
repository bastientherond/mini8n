<script lang="ts">
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import Cloud from '@lucide/svelte/icons/cloud';
	import Copy from '@lucide/svelte/icons/copy';
	import Download from '@lucide/svelte/icons/download';
	import Ellipsis from '@lucide/svelte/icons/ellipsis';
	import History from '@lucide/svelte/icons/history';
	import Server from '@lucide/svelte/icons/server';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import Play from '@lucide/svelte/icons/play';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import WandSparkles from '@lucide/svelte/icons/wand-sparkles';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { Input } from '$lib/components/ui/input';
	import { Switch } from '$lib/components/ui/switch';
	import { toast } from 'svelte-sonner';
	import { errorMessage } from '$lib/api';
	import { router } from '$lib/router.svelte';
	import { attempt, duplicateWorkflow, exportWorkflow, ui } from '$lib/ui.svelte';
	import { wf } from '$lib/workflow.svelte';

	let { onrun, onarrange }: { onrun: () => void; onarrange: () => void } = $props();

	let serverRunning = $state(false);

	async function runOnServer() {
		serverRunning = true;
		try {
			const res = await wf.runOnServer();
			if (!res) return;
			if (res.status === 'success') toast.success('Exécution serveur réussie', { action: { label: 'Voir', onClick: openExecutions } });
			else if (res.status === 'error') toast.error(res.error ?? 'Exécution serveur en échec', { action: { label: 'Voir', onClick: openExecutions } });
			else toast.info('Exécution serveur en cours…', { action: { label: 'Voir', onClick: openExecutions } });
		} catch (e) {
			toast.error(errorMessage(e));
		} finally {
			serverRunning = false;
		}
	}

	function openExecutions() {
		if (wf.workspaceId) router.go({ name: 'executions', ws: wf.workspaceId });
	}

	const summary = $derived.by(() => {
		const run = wf.lastRun;
		if (!run) return null;
		const errors = Object.values(run.results).filter((r) => r.status === 'error').length;
		return { ok: run.ok && !run.error, errors, ms: run.durationMs };
	});
</script>

<div class="bg-background/80 supports-[backdrop-filter]:bg-background/70 flex h-12 shrink-0 items-center gap-2 border-b px-3 backdrop-blur">
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
		<Button
			variant="outline"
			size="sm"
			onclick={onarrange}
			disabled={wf.nodes.length === 0}
			title="Range et aligne tous les nœuds (Maj + Alt + T)"
		>
			<WandSparkles /> Ranger
		</Button>
		<label
			class="hover:bg-accent flex h-8 cursor-pointer items-center gap-2 rounded-md border px-2.5 text-xs font-medium"
			title="Met en service les déclencheurs automatiques (planification, webhook, formulaire…)"
		>
			<span class={wf.active ? 'text-emerald-600 dark:text-emerald-400' : 'text-muted-foreground'}>
				{wf.active ? 'Actif' : 'Inactif'}
			</span>
			<Switch
				checked={wf.active}
				disabled={!wf.workflowId || wf.activating}
				onCheckedChange={(v) => wf.setActive(v)}
				aria-label="Activer le workflow"
			/>
		</label>
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
			<DropdownMenu.Content align="end" class="w-60">
				<DropdownMenu.Item onclick={runOnServer} disabled={serverRunning}>
					{#if serverRunning}<LoaderCircle class="animate-spin" />{:else}<Server />{/if}
					Exécuter sur le serveur
				</DropdownMenu.Item>
				<DropdownMenu.Item onclick={openExecutions}><History /> Voir les exécutions</DropdownMenu.Item>
				<DropdownMenu.Separator />
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
		<Button size="sm" class="shadow-primary/25 shadow-md" onclick={onrun} disabled={wf.running || !wf.workflowId} title="⌘/Ctrl + Entrée">
			{#if wf.running}<LoaderCircle class="animate-spin" />{:else}<Play />{/if}
			Exécuter
		</Button>
	</div>
</div>
