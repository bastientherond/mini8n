<script lang="ts">
	import Download from '@lucide/svelte/icons/download';
	import FileText from '@lucide/svelte/icons/file-text';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import Upload from '@lucide/svelte/icons/upload';
	import Workflow from '@lucide/svelte/icons/workflow';
	import { toast } from 'svelte-sonner';
	import { Button } from '$lib/components/ui/button';
	import * as Table from '$lib/components/ui/table';
	import { api, errorMessage, type FileInfo } from '$lib/api';
	import { formatBytes, formatRelative } from '$lib/icons';
	import { resources } from '$lib/resources.svelte';
	import { session } from '$lib/session.svelte';
	import Page from './Page.svelte';

	const ws = $derived(session.current!);
	const total = $derived(resources.files.reduce((n, f) => n + f.size, 0));

	let uploading = $state(false);
	let input: HTMLInputElement | null = $state(null);

	async function upload(event: Event) {
		const files = [...((event.currentTarget as HTMLInputElement).files ?? [])];
		if (input) input.value = '';
		if (!files.length) return;
		uploading = true;
		try {
			for (const f of files) await api.uploadFile(ws.id, f);
			await resources.refreshFiles();
			toast.success(files.length > 1 ? `${files.length} fichiers téléversés` : `« ${files[0].name} » téléversé`);
		} catch (e) {
			toast.error(errorMessage(e));
		} finally {
			uploading = false;
		}
	}

	async function remove(f: FileInfo) {
		try {
			await api.deleteFile(ws.id, f.id);
			await resources.refreshFiles();
			toast.success(`« ${f.name} » supprimé`);
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}
</script>

<Page title="Fichiers" description="Fichiers du workspace : créés par le nœud « Créer un fichier » ou téléversés pour « Lire un fichier ».">
	{#snippet actions()}
		<Button size="sm" disabled={uploading} onclick={() => input?.click()}>
			{#if uploading}<LoaderCircle class="animate-spin" />{:else}<Upload />{/if}
			Téléverser
		</Button>
		<input bind:this={input} type="file" multiple class="hidden" accept=".csv,.json,.parquet,.xlsx,.xlsm,.xls,.ods,.txt" onchange={upload} />
	{/snippet}

	<p class="text-muted-foreground mb-3 text-sm">{resources.files.length} fichier(s) · {formatBytes(total)} sur 1 Go · 50 Mo max. par fichier</p>

	<div class="rounded-xl border">
		<Table.Root>
			<Table.Header>
				<Table.Row>
					<Table.Head>Nom</Table.Head>
					<Table.Head>Origine</Table.Head>
					<Table.Head>Créé</Table.Head>
					<Table.Head class="text-right">Taille</Table.Head>
					<Table.Head class="w-24"><span class="sr-only">Actions</span></Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each resources.files as f (f.id)}
					<Table.Row>
						<Table.Cell>
							<span class="flex items-center gap-2 font-medium"><FileText class="text-muted-foreground size-4" />{f.name}</span>
						</Table.Cell>
						<Table.Cell class="text-muted-foreground">
							{#if f.executionId}<span class="flex items-center gap-1"><Workflow class="size-3.5" /> Workflow</span>{:else}Téléversé{/if}
						</Table.Cell>
						<Table.Cell class="text-muted-foreground">{formatRelative(f.createdAt)}</Table.Cell>
						<Table.Cell class="text-muted-foreground text-right tabular-nums">{formatBytes(f.size)}</Table.Cell>
						<Table.Cell>
							<div class="flex justify-end gap-1">
								<Button variant="ghost" size="icon-sm" href={api.fileUrl(ws.id, f.id)} download={f.name} aria-label="Télécharger {f.name}">
									<Download />
								</Button>
								<Button variant="ghost" size="icon-sm" onclick={() => remove(f)} aria-label="Supprimer {f.name}"><Trash2 /></Button>
							</div>
						</Table.Cell>
					</Table.Row>
				{:else}
					<Table.Row>
						<Table.Cell colspan={5} class="text-muted-foreground py-10 text-center">Aucun fichier pour l'instant.</Table.Cell>
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	</div>
</Page>
