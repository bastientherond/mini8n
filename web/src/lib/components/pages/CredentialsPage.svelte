<script lang="ts">
	import Ellipsis from '@lucide/svelte/icons/ellipsis';
	import KeyRound from '@lucide/svelte/icons/key-round';
	import Pencil from '@lucide/svelte/icons/pencil';
	import Plus from '@lucide/svelte/icons/plus';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import Zap from '@lucide/svelte/icons/zap';
	import { toast } from 'svelte-sonner';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as Table from '$lib/components/ui/table';
	import { api, errorMessage, type CredentialInfo } from '$lib/api';
	import { formatRelative } from '$lib/icons';
	import { resources } from '$lib/resources.svelte';
	import { session } from '$lib/session.svelte';
	import CredentialDialog from '../CredentialDialog.svelte';
	import Page from './Page.svelte';

	const ws = $derived(session.current!);

	let dialog = $state(false);
	let editing = $state<CredentialInfo | null>(null);
	let toDelete = $state<CredentialInfo | null>(null);

	function create() {
		editing = null;
		dialog = true;
	}

	function edit(c: CredentialInfo) {
		editing = c;
		dialog = true;
	}

	async function test(c: CredentialInfo) {
		try {
			const r = await api.testCredential(ws.id, c.id);
			if (r.ok) toast.success(r.message);
			else toast.error(r.message);
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	async function remove() {
		const c = toDelete;
		toDelete = null;
		if (!c) return;
		try {
			await api.deleteCredential(ws.id, c.id);
			await resources.refreshCredentials();
			toast.success(`« ${c.name} » supprimé`);
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	const detail = (c: CredentialInfo) =>
		[c.summary.user, c.summary.host && `${c.summary.host}${c.summary.port ? `:${c.summary.port}` : ''}`, c.summary.database ?? c.summary.service]
			.filter(Boolean)
			.join(' · ');
</script>

<Page title="Identifiants" description="Accès aux bases de données, serveurs SMTP et messageries utilisés par les workflows du workspace.">
	{#snippet actions()}
		<Button size="sm" onclick={create}><Plus /> Nouvel identifiant</Button>
	{/snippet}

	<div class="bg-muted/40 mb-4 flex items-start gap-2 rounded-lg border p-3 text-sm">
		<ShieldCheck class="mt-0.5 size-4 shrink-0 text-emerald-600" />
		<p class="text-muted-foreground">
			Les secrets sont chiffrés (AES-256-GCM) sur le serveur et ne sont jamais renvoyés au navigateur. Les workflows ne
			contiennent qu'une référence : un export de workflow ne divulgue aucun mot de passe.
		</p>
	</div>

	<div class="rounded-xl border">
		<Table.Root>
			<Table.Header>
				<Table.Row>
					<Table.Head>Nom</Table.Head>
					<Table.Head>Type</Table.Head>
					<Table.Head>Modifié</Table.Head>
					<Table.Head class="w-12"><span class="sr-only">Actions</span></Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each resources.credentials as c (c.id)}
					<Table.Row>
						<Table.Cell>
							<button class="flex items-center gap-2 font-medium hover:underline" onclick={() => edit(c)}>
								<KeyRound class="text-muted-foreground size-4" />
								{c.name}
							</button>
							{#if detail(c)}<div class="text-muted-foreground ml-6 text-xs">{detail(c)}</div>{/if}
						</Table.Cell>
						<Table.Cell><Badge variant="secondary">{resources.kind(c.kind)?.label ?? c.kind}</Badge></Table.Cell>
						<Table.Cell class="text-muted-foreground">{formatRelative(c.updatedAt)}</Table.Cell>
						<Table.Cell>
							<DropdownMenu.Root>
								<DropdownMenu.Trigger>
									{#snippet child({ props })}
										<Button {...props} variant="ghost" size="icon-sm" aria-label="Actions pour {c.name}"><Ellipsis /></Button>
									{/snippet}
								</DropdownMenu.Trigger>
								<DropdownMenu.Content align="end">
									<DropdownMenu.Item onclick={() => edit(c)}><Pencil /> Modifier</DropdownMenu.Item>
									<DropdownMenu.Item onclick={() => test(c)}><Zap /> Tester la connexion</DropdownMenu.Item>
									<DropdownMenu.Separator />
									<DropdownMenu.Item variant="destructive" onclick={() => (toDelete = c)}><Trash2 /> Supprimer</DropdownMenu.Item>
								</DropdownMenu.Content>
							</DropdownMenu.Root>
						</Table.Cell>
					</Table.Row>
				{:else}
					<Table.Row>
						<Table.Cell colspan={4} class="text-muted-foreground py-10 text-center">
							Aucun identifiant. Créez-en un pour utiliser PostgreSQL, MySQL, Oracle, l'e-mail ou une messagerie.
						</Table.Cell>
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	</div>
</Page>

<CredentialDialog bind:open={dialog} credential={editing} />

<AlertDialog.Root open={toDelete !== null} onOpenChange={(o) => !o && (toDelete = null)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Supprimer « {toDelete?.name} » ?</AlertDialog.Title>
			<AlertDialog.Description>Les nœuds qui l'utilisent échoueront tant qu'un autre identifiant n'est pas choisi.</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Annuler</AlertDialog.Cancel>
			<AlertDialog.Action class="bg-destructive hover:bg-destructive/90 text-white" onclick={remove}>Supprimer</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
