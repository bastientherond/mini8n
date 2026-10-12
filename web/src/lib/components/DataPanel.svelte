<script lang="ts">
	import ChevronLeft from '@lucide/svelte/icons/chevron-left';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import CircleX from '@lucide/svelte/icons/circle-x';
	import Download from '@lucide/svelte/icons/download';
	import Table2 from '@lucide/svelte/icons/table-2';
	import { toast } from 'svelte-sonner';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as Table from '$lib/components/ui/table';
	import * as Tabs from '$lib/components/ui/tabs';
	import { engine } from '$lib/engine/client';
	import type { Preview } from '$lib/engine/types';
	import { formatBytes, formatCount } from '$lib/icons';
	import type { Snippet } from 'svelte';
	import { wf } from '$lib/workflow.svelte';
	import { columnRef } from '$lib/expressions/context';
	import { FIELD_MIME } from './code/CodeEditor.svelte';
	import GripVertical from '@lucide/svelte/icons/grip-vertical';

	const PAGE = 50;

	let {
		nodeId,
		label = 'Sortie',
		emptyAction,
		fieldBase,
	}: {
		nodeId: string | null;
		label?: string;
		/** Action proposée quand le nœud n'a pas encore de données (ex. bouton Exécuter). */
		emptyAction?: Snippet;
		/** Colonnes glissables vers les expressions, préfixées par cette base (`$json`). */
		fieldBase?: string;
	} = $props();

	function dragColumn(event: DragEvent, name: string) {
		if (!fieldBase || !event.dataTransfer) return;
		const ref = columnRef(name, fieldBase);
		event.dataTransfer.setData(FIELD_MIME, ref);
		event.dataTransfer.setData('text/plain', ref);
		event.dataTransfer.effectAllowed = 'copy';
	}

	const node = $derived(nodeId ? (wf.nodes.find((n) => n.id === nodeId) ?? null) : null);
	const result = $derived(node ? wf.results[node.id] : undefined);
	const bytes = $derived(node ? wf.data[node.id] : undefined);

	let page = $state(0);
	let preview = $state.raw<Preview | null>(null);
	let previewError = $state<string | null>(null);

	// Revenir à la première page quand la sortie change.
	$effect(() => {
		void bytes;
		page = 0;
	});

	$effect(() => {
		const b = bytes;
		const offset = page * PAGE;
		if (!b) {
			preview = null;
			return;
		}
		let cancelled = false;
		engine
			.preview(b, offset, PAGE)
			.then((p) => {
				if (!cancelled) {
					preview = p;
					previewError = null;
				}
			})
			.catch((e: Error) => {
				if (!cancelled) previewError = e.message;
			});
		return () => {
			cancelled = true;
		};
	});

	const pages = $derived(preview ? Math.max(1, Math.ceil(preview.total / PAGE)) : 1);

	function fileBase() {
		return (node?.data.name || 'sortie').replace(/[^\p{L}\p{N}_-]+/gu, '_');
	}

	function save(blob: Blob, filename: string) {
		const url = URL.createObjectURL(blob);
		const a = Object.assign(document.createElement('a'), { href: url, download: filename });
		a.click();
		setTimeout(() => URL.revokeObjectURL(url), 1000);
	}

	async function download(format: 'parquet' | 'csv' | 'json') {
		if (!bytes) return;
		try {
			if (format === 'parquet') {
				save(new Blob([bytes as BlobPart], { type: 'application/vnd.apache.parquet' }), `${fileBase()}.parquet`);
			} else if (format === 'csv') {
				save(new Blob([await engine.toCsv(bytes)], { type: 'text/csv' }), `${fileBase()}.csv`);
			} else {
				save(new Blob([await engine.toJson(bytes)], { type: 'application/json' }), `${fileBase()}.json`);
			}
		} catch (e) {
			toast.error(`Export impossible : ${e instanceof Error ? e.message : e}`);
		}
	}

	function cell(v: unknown): string {
		if (v === null || v === undefined) return '';
		if (typeof v === 'object') return JSON.stringify(v);
		if (typeof v === 'number') return new Intl.NumberFormat('fr-FR', { maximumFractionDigits: 6 }).format(v);
		return String(v);
	}
</script>

<section class="bg-background flex h-full min-h-0 flex-col">
	{#if !node}
		<div class="text-muted-foreground flex flex-1 items-center justify-center gap-2 text-sm">
			<Table2 class="size-4" /> Aucune donnée.
		</div>
	{:else}
		<Tabs.Root value="table" class="flex min-h-0 flex-1 flex-col gap-0">
			<div class="flex flex-wrap items-center gap-x-3 gap-y-2 border-b px-4 py-2">
				<span class="text-sm font-medium">{label} · {node.data.name}</span>
				{#if result?.status === 'success'}
					<Badge variant="secondary">{formatCount(result.rows)} lignes</Badge>
					<Badge variant="secondary">{result.columns.length} colonnes</Badge>
					<span class="text-muted-foreground text-xs">
						Parquet {formatBytes(result.bytes)} · {result.durationMs.toFixed(1)} ms
					</span>
				{:else if result?.status === 'error'}
					<Badge variant="destructive">Erreur</Badge>
				{:else if result?.status === 'skipped'}
					<Badge variant="outline">Ignoré</Badge>
				{/if}

				<div class="ml-auto flex items-center gap-2">
					{#if bytes}
						<Tabs.List class="h-8">
							<Tabs.Trigger value="table">Table</Tabs.Trigger>
							<Tabs.Trigger value="schema">Schéma</Tabs.Trigger>
						</Tabs.List>
						<DropdownMenu.Root>
							<DropdownMenu.Trigger>
								{#snippet child({ props })}
									<Button {...props} variant="outline" size="sm"><Download /> Exporter</Button>
								{/snippet}
							</DropdownMenu.Trigger>
							<DropdownMenu.Content align="end">
								<DropdownMenu.Item onclick={() => download('parquet')}>Parquet (.parquet)</DropdownMenu.Item>
								<DropdownMenu.Item onclick={() => download('csv')}>CSV (.csv)</DropdownMenu.Item>
								<DropdownMenu.Item onclick={() => download('json')}>JSON (.json)</DropdownMenu.Item>
							</DropdownMenu.Content>
						</DropdownMenu.Root>
					{/if}
				</div>
			</div>

			{#if result?.status === 'error' || result?.status === 'skipped'}
				<div class="flex items-start gap-2 p-4 text-sm">
					<CircleX class="text-destructive mt-0.5 size-4 shrink-0" />
					<pre class="font-mono text-xs whitespace-pre-wrap">{result.error}</pre>
				</div>
			{:else if !bytes}
				<div class="text-muted-foreground flex flex-1 flex-col items-center justify-center gap-3 p-6 text-center text-sm">
					<p>Pas encore de données : exécutez le nœud pour les voir.</p>
					{@render emptyAction?.()}
				</div>
			{:else if previewError}
				<p class="text-destructive p-4 text-sm">{previewError}</p>
			{:else if preview}
				<Tabs.Content value="table" class="flex min-h-0 flex-1 flex-col">
					{#if preview.columns.length === 0}
						<p class="text-muted-foreground p-4 text-sm">Table vide (aucune colonne).</p>
					{:else}
						<div class="min-h-0 flex-1 overflow-auto">
							<Table.Root class="text-xs">
								<Table.Header class="bg-muted/60 sticky top-0 z-10 backdrop-blur">
									<Table.Row>
										<Table.Head class="text-muted-foreground w-12 text-right">#</Table.Head>
										{#each preview.columns as col (col.name)}
											<Table.Head
												draggable={fieldBase ? 'true' : undefined}
												ondragstart={(e: DragEvent) => dragColumn(e, col.name)}
												class={fieldBase ? 'group/col cursor-grab active:cursor-grabbing' : ''}
												title={fieldBase ? `Glisser vers un champ : ${columnRef(col.name, fieldBase)}` : undefined}
											>
												<div class="flex items-center gap-1 font-medium">
													{#if fieldBase}<GripVertical
															class="text-muted-foreground/50 group-hover/col:text-muted-foreground -ml-1 size-3 shrink-0"
														/>{/if}
													{col.name}
												</div>
												<div class="text-muted-foreground font-mono text-[10px] font-normal">{col.type}</div>
											</Table.Head>
										{/each}
									</Table.Row>
								</Table.Header>
								<Table.Body>
									{#each preview.rows as row, i (preview.offset + i)}
										<Table.Row>
											<Table.Cell class="text-muted-foreground text-right tabular-nums">{preview.offset + i + 1}</Table.Cell>
											{#each preview.columns as col (col.name)}
												{@const v = row[col.name]}
												<Table.Cell
													class={[
														'max-w-80 truncate',
														typeof v === 'number' && 'text-right tabular-nums',
														v == null && 'text-muted-foreground/60 italic',
													]}
													title={cell(v)}>{v == null ? 'null' : cell(v)}</Table.Cell
												>
											{/each}
										</Table.Row>
									{/each}
								</Table.Body>
							</Table.Root>
						</div>
						<div class="flex items-center justify-end gap-2 border-t px-4 py-1.5 text-xs">
							<span class="text-muted-foreground">
								{formatCount(preview.total === 0 ? 0 : preview.offset + 1)}–{formatCount(preview.offset + preview.rows.length)}
								sur {formatCount(preview.total)}
							</span>
							<Button variant="ghost" size="icon-sm" disabled={page === 0} onclick={() => page--} aria-label="Page précédente">
								<ChevronLeft />
							</Button>
							<span class="tabular-nums">{page + 1} / {pages}</span>
							<Button variant="ghost" size="icon-sm" disabled={page + 1 >= pages} onclick={() => page++} aria-label="Page suivante">
								<ChevronRight />
							</Button>
						</div>
					{/if}
				</Tabs.Content>
				<Tabs.Content value="schema" class="min-h-0 flex-1 overflow-auto">
					<Table.Root class="text-xs">
						<Table.Header>
							<Table.Row>
								<Table.Head>Colonne</Table.Head>
								<Table.Head>Type Arrow</Table.Head>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each preview.columns as col (col.name)}
								<Table.Row>
									<Table.Cell class="font-medium">{col.name}</Table.Cell>
									<Table.Cell class="font-mono">{col.type}</Table.Cell>
								</Table.Row>
							{/each}
						</Table.Body>
					</Table.Root>
				</Tabs.Content>
			{/if}
		</Tabs.Root>
	{/if}
</section>
