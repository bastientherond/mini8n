<script lang="ts">
	import CircleCheck from '@lucide/svelte/icons/circle-check';
	import CircleX from '@lucide/svelte/icons/circle-x';
	import CornerDownRight from '@lucide/svelte/icons/corner-down-right';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import SquarePen from '@lucide/svelte/icons/square-pen';
	import { toast } from 'svelte-sonner';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Select from '$lib/components/ui/select';
	import * as Table from '$lib/components/ui/table';
	import { api, errorMessage, type ExecutionDetail, type ExecutionSummary } from '$lib/api';
	import { formatCount, formatDateTime, formatDuration, formatRelative, triggerKindLabels } from '$lib/icons';
	import { href } from '$lib/router.svelte';
	import { session } from '$lib/session.svelte';
	import { wf } from '$lib/workflow.svelte';
	import Page from './Page.svelte';

	const ws = $derived(session.current!);

	let workflowFilter = $state('');
	let statusFilter = $state('');
	let rows = $state.raw<ExecutionSummary[]>([]);
	let loading = $state(true);
	let detail = $state.raw<ExecutionDetail | null>(null);
	let detailOpen = $state(false);

	const STATUS: Record<string, string> = { '': 'Tous les statuts', success: 'Succès', error: 'Erreur', running: 'En cours' };

	async function load(showSpinner = false) {
		if (showSpinner) loading = true;
		try {
			rows = await api.executions(ws.id, { workflow: workflowFilter || undefined, status: statusFilter || undefined, limit: 100 });
		} catch (e) {
			toast.error(errorMessage(e));
		} finally {
			loading = false;
		}
	}

	// Rechargement à chaque changement de filtre, puis toutes les 5 s.
	$effect(() => {
		void workflowFilter;
		void statusFilter;
		void ws.id;
		void load(true);
		const timer = setInterval(() => void load(), 5000);
		return () => clearInterval(timer);
	});

	async function open(row: ExecutionSummary) {
		try {
			detail = await api.execution(ws.id, row.id);
			detailOpen = true;
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	function cell(v: unknown): string {
		if (v === null || v === undefined) return '';
		return typeof v === 'object' ? JSON.stringify(v) : String(v);
	}
</script>

<Page title="Exécutions" description="Exécutions côté serveur : déclencheurs automatiques, appels de sous-workflows, exécutions à distance.">
	{#snippet actions()}
		<Button variant="outline" size="sm" onclick={() => load(true)}><RefreshCw /> Actualiser</Button>
	{/snippet}

	<div class="mb-4 flex flex-wrap gap-2">
		<Select.Root type="single" bind:value={workflowFilter}>
			<Select.Trigger class="w-60">{wf.workflows.find((w) => w.id === workflowFilter)?.name ?? 'Tous les workflows'}</Select.Trigger>
			<Select.Content>
				<Select.Item value="" label="Tous les workflows">Tous les workflows</Select.Item>
				{#each wf.workflows as item (item.id)}
					<Select.Item value={item.id} label={item.name}>{item.name}</Select.Item>
				{/each}
			</Select.Content>
		</Select.Root>
		<Select.Root type="single" bind:value={statusFilter}>
			<Select.Trigger class="w-44">{STATUS[statusFilter]}</Select.Trigger>
			<Select.Content>
				{#each Object.entries(STATUS) as [value, label] (value)}
					<Select.Item {value} {label}>{label}</Select.Item>
				{/each}
			</Select.Content>
		</Select.Root>
	</div>

	<div class="rounded-xl border">
		<Table.Root>
			<Table.Header>
				<Table.Row>
					<Table.Head class="w-10"><span class="sr-only">Statut</span></Table.Head>
					<Table.Head>Workflow</Table.Head>
					<Table.Head>Déclencheur</Table.Head>
					<Table.Head>Début</Table.Head>
					<Table.Head class="text-right">Durée</Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#if loading && rows.length === 0}
					<Table.Row>
						<Table.Cell colspan={5} class="text-muted-foreground py-10">
							<span class="flex items-center justify-center gap-2"><LoaderCircle class="size-4 animate-spin" /> Chargement…</span>
						</Table.Cell>
					</Table.Row>
				{:else}
					{#each rows as row (row.id)}
						<Table.Row class="cursor-pointer" onclick={() => open(row)}>
							<Table.Cell>
								{#if row.status === 'success'}
									<CircleCheck class="size-4 text-emerald-500" aria-label="Succès" />
								{:else if row.status === 'error'}
									<CircleX class="text-destructive size-4" aria-label="Erreur" />
								{:else}
									<LoaderCircle class="text-muted-foreground size-4 animate-spin" aria-label="En cours" />
								{/if}
							</Table.Cell>
							<Table.Cell>
								<div class="flex items-center gap-1.5 font-medium">
									{#if row.parentId}<CornerDownRight class="text-muted-foreground size-3.5" />{/if}
									{row.workflowName}
								</div>
								{#if row.error}<div class="text-destructive max-w-md truncate text-xs" title={row.error}>{row.error}</div>{/if}
							</Table.Cell>
							<Table.Cell><Badge variant="secondary">{triggerKindLabels[row.triggerKind] ?? row.triggerKind}</Badge></Table.Cell>
							<Table.Cell class="text-muted-foreground" title={formatDateTime(row.startedAt)}>{formatRelative(row.startedAt)}</Table.Cell>
							<Table.Cell class="text-muted-foreground text-right tabular-nums">{formatDuration(row.durationMs)}</Table.Cell>
						</Table.Row>
					{:else}
						<Table.Row>
							<Table.Cell colspan={5} class="text-muted-foreground py-10 text-center">
								Aucune exécution. Activez un workflow avec un déclencheur automatique, ou utilisez « Exécuter sur le serveur ».
							</Table.Cell>
						</Table.Row>
					{/each}
				{/if}
			</Table.Body>
		</Table.Root>
	</div>
</Page>

<Dialog.Root bind:open={detailOpen}>
	<Dialog.Content class="max-h-[85vh] overflow-y-auto sm:max-w-3xl">
		{#if detail}
			{@const report = detail.report}
			<Dialog.Header>
				<Dialog.Title class="flex items-center gap-2">
					{#if detail.status === 'success'}<CircleCheck class="size-5 text-emerald-500" />{:else if detail.status === 'error'}<CircleX class="text-destructive size-5" />{:else}<LoaderCircle class="size-5 animate-spin" />{/if}
					{detail.workflowName}
				</Dialog.Title>
				<Dialog.Description>
					{triggerKindLabels[detail.triggerKind] ?? detail.triggerKind} · {formatDateTime(detail.startedAt)} · {formatDuration(detail.durationMs)}
				</Dialog.Description>
			</Dialog.Header>

			{#if detail.workflowId}
				<div>
					<Button variant="outline" size="sm" href={href({ name: 'editor', ws: ws.id, workflow: detail.workflowId })} onclick={() => (detailOpen = false)}>
						<SquarePen /> Ouvrir le workflow
					</Button>
				</div>
			{/if}

			{#if detail.error}
				<pre class="bg-destructive/10 text-destructive rounded-md p-3 font-mono text-xs whitespace-pre-wrap">{detail.error}</pre>
			{/if}

			{#if report?.order?.length}
				<div>
					<h3 class="mb-2 text-sm font-medium">Nœuds</h3>
					<div class="rounded-lg border">
						<Table.Root class="text-xs">
							<Table.Body>
								{#each report.order as id (id)}
									{@const r = report.results?.[id]}
									<Table.Row>
										<Table.Cell class="font-medium">{report.names?.[id] ?? id}</Table.Cell>
										<Table.Cell>
											<Badge variant={r?.status === 'error' ? 'destructive' : r?.status === 'success' ? 'secondary' : 'outline'}>
												{r?.status === 'success' ? 'succès' : r?.status === 'error' ? 'erreur' : 'ignoré'}
											</Badge>
										</Table.Cell>
										<Table.Cell class="text-muted-foreground max-w-xs truncate" title={r?.error}>{r?.error ?? `${formatCount(r?.rows ?? 0)} ligne(s)`}</Table.Cell>
										<Table.Cell class="text-muted-foreground text-right tabular-nums">{formatDuration(r?.durationMs)}</Table.Cell>
									</Table.Row>
								{/each}
							</Table.Body>
						</Table.Root>
					</div>
				</div>
			{/if}

			{#if detail.output && detail.output.columns.length > 0}
				<div>
					<h3 class="mb-2 text-sm font-medium">
						Sortie · {report?.names?.[report?.output ?? ''] ?? ''} ({formatCount(detail.output.total)} ligne{detail.output.total > 1 ? 's' : ''}{detail.output.total > detail.output.rows.length ? `, ${detail.output.rows.length} affichées` : ''})
					</h3>
					<div class="max-h-72 overflow-auto rounded-lg border">
						<Table.Root class="text-xs">
							<Table.Header class="bg-muted/60 sticky top-0">
								<Table.Row>
									{#each detail.output.columns as col (col.name)}<Table.Head>{col.name}</Table.Head>{/each}
								</Table.Row>
							</Table.Header>
							<Table.Body>
								{#each detail.output.rows as row, i (i)}
									<Table.Row>
										{#each detail.output.columns as col (col.name)}
											<Table.Cell class="max-w-60 truncate" title={cell(row[col.name])}>{cell(row[col.name])}</Table.Cell>
										{/each}
									</Table.Row>
								{/each}
							</Table.Body>
						</Table.Root>
					</div>
				</div>
			{/if}
		{/if}
	</Dialog.Content>
</Dialog.Root>
