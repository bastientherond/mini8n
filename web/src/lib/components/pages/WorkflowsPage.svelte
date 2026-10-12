<script lang="ts">
	import Copy from '@lucide/svelte/icons/copy';
	import Download from '@lucide/svelte/icons/download';
	import Ellipsis from '@lucide/svelte/icons/ellipsis';
	import FilePlus from '@lucide/svelte/icons/file-plus';
	import FileText from '@lucide/svelte/icons/file-text';
	import Search from '@lucide/svelte/icons/search';
	import SquarePen from '@lucide/svelte/icons/square-pen';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import Upload from '@lucide/svelte/icons/upload';
	import { Button } from '$lib/components/ui/button';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { Input } from '$lib/components/ui/input';
	import * as Table from '$lib/components/ui/table';
	import { formatRelative } from '$lib/icons';
	import { href, router } from '$lib/router.svelte';
	import { session } from '$lib/session.svelte';
	import { duplicateWorkflow, exportWorkflow, pickWorkflowFile, ui } from '$lib/ui.svelte';
	import { wf } from '$lib/workflow.svelte';
	import Page from './Page.svelte';

	const ws = $derived(session.current!);
	let query = $state('');
	const rows = $derived(
		wf.workflows.filter((w) => !query.trim() || w.name.toLowerCase().includes(query.trim().toLowerCase())),
	);
</script>

<Page title="Workflows" description="Tous les workflows du workspace « {ws.name} ».">
	{#snippet actions()}
		<Button variant="outline" size="sm" onclick={pickWorkflowFile}><Upload /> Importer</Button>
		<Button size="sm" onclick={() => (ui.newWorkflow = true)}><FilePlus /> Nouveau workflow</Button>
	{/snippet}

	<div class="relative mb-4 max-w-xs">
		<Search class="text-muted-foreground absolute top-1/2 left-2.5 size-4 -translate-y-1/2" />
		<Input bind:value={query} placeholder="Filtrer par nom…" class="pl-8" />
	</div>

	<div class="rounded-xl border">
		<Table.Root>
			<Table.Header>
				<Table.Row>
					<Table.Head>Nom</Table.Head>
					<Table.Head>Dernière modification</Table.Head>
					<Table.Head class="text-right">Version</Table.Head>
					<Table.Head class="w-12"><span class="sr-only">Actions</span></Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each rows as item (item.id)}
					<Table.Row class="group">
						<Table.Cell>
							<a href={href({ name: 'editor', ws: ws.id, workflow: item.id })} class="flex items-center gap-2.5 font-medium hover:underline">
								<FileText class="text-muted-foreground size-4" />
								{item.name}
							</a>
							{#if item.active}
								<span class="mt-1 inline-flex items-center gap-1 text-xs text-emerald-600 dark:text-emerald-400">
									<span class="size-1.5 rounded-full bg-emerald-500"></span> Actif
								</span>
							{/if}
						</Table.Cell>
						<Table.Cell class="text-muted-foreground">
							{formatRelative(item.updatedAt)}{item.updatedBy ? ` · ${item.updatedBy}` : ''}
						</Table.Cell>
						<Table.Cell class="text-muted-foreground text-right tabular-nums">v{item.version}</Table.Cell>
						<Table.Cell>
							<DropdownMenu.Root>
								<DropdownMenu.Trigger>
									{#snippet child({ props })}
										<Button {...props} variant="ghost" size="icon-sm" aria-label="Actions pour {item.name}"><Ellipsis /></Button>
									{/snippet}
								</DropdownMenu.Trigger>
								<DropdownMenu.Content align="end">
									<DropdownMenu.Item onclick={() => router.go({ name: 'editor', ws: ws.id, workflow: item.id })}><SquarePen /> Ouvrir</DropdownMenu.Item>
									<DropdownMenu.Item onclick={() => duplicateWorkflow(item.id)}><Copy /> Dupliquer</DropdownMenu.Item>
									<DropdownMenu.Item onclick={() => exportWorkflow(item.id)}><Download /> Exporter (.json)</DropdownMenu.Item>
									<DropdownMenu.Separator />
									<DropdownMenu.Item variant="destructive" onclick={() => (ui.confirmDelete = { id: item.id, name: item.name })}>
										<Trash2 /> Supprimer
									</DropdownMenu.Item>
								</DropdownMenu.Content>
							</DropdownMenu.Root>
						</Table.Cell>
					</Table.Row>
				{:else}
					<Table.Row>
						<Table.Cell colspan={4} class="text-muted-foreground py-10 text-center">
							{query ? 'Aucun workflow ne correspond.' : 'Aucun workflow pour l’instant.'}
						</Table.Cell>
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	</div>
</Page>
