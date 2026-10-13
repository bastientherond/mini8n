<script lang="ts">
	import Ellipsis from '@lucide/svelte/icons/ellipsis';
	import Pencil from '@lucide/svelte/icons/pencil';
	import Plus from '@lucide/svelte/icons/plus';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import Workflow from '@lucide/svelte/icons/workflow';
	import Zap from '@lucide/svelte/icons/zap';
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { Button } from '$lib/components/ui/button';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as Table from '$lib/components/ui/table';
	import { api, errorMessage, type CredentialInfo, type CredentialUsage } from '$lib/api';
	import { brandOfCredential, brands } from '$lib/brands';
	import { formatRelative } from '$lib/icons';
	import { resources } from '$lib/resources.svelte';
	import { href } from '$lib/router.svelte';
	import { session } from '$lib/session.svelte';
	import BrandLogo from '../BrandLogo.svelte';
	import CredentialDialog from '../CredentialDialog.svelte';
	import Page from './Page.svelte';

	const ws = $derived(session.current!);

	// L'utilisation par les workflows change hors de cette page : liste rafraîchie à l'ouverture.
	onMount(() => void resources.refreshCredentials().catch((e) => toast.error(errorMessage(e))));

	let dialog = $state(false);
	let editing = $state<CredentialInfo | null>(null);
	/** Type proposé à la création (choix d'un service dans la grille). */
	let preset = $state('');
	let toDelete = $state<CredentialInfo | null>(null);

	function create(kind = '') {
		editing = null;
		preset = kind;
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

	/** Détail non secret : utilisateur, hôte, base… ou service reconnu pour un webhook. */
	function detail(c: CredentialInfo): string {
		const s = c.summary;
		if (c.kind === 'chat_webhook') return typeof s.service === 'string' ? s.service : 'Service non reconnu';
		return [s.user, s.host && `${s.host}${s.port ? `:${s.port}` : ''}`, s.database ?? s.service ?? s.from].filter(Boolean).join(' · ');
	}

	/** Workflows qui utilisent l'identifiant, avec leurs nœuds. */
	function workflowsOf(c: CredentialInfo): { id: string; name: string; nodes: CredentialUsage[] }[] {
		const out = new Map<string, { id: string; name: string; nodes: CredentialUsage[] }>();
		for (const u of c.usage ?? []) {
			const w = out.get(u.workflowId) ?? { id: u.workflowId, name: u.workflowName, nodes: [] };
			w.nodes.push(u);
			out.set(u.workflowId, w);
		}
		return [...out.values()];
	}

	const nodeList = (nodes: CredentialUsage[]) => nodes.map((n) => n.nodeName || n.nodeKind).join(', ');
</script>

<Page title="Identifiants" description="Accès aux bases de données, serveurs SMTP et messageries utilisés par les workflows du workspace.">
	{#snippet actions()}
		<Button size="sm" onclick={() => create()}><Plus /> Nouvel identifiant</Button>
	{/snippet}

	<div class="bg-muted/40 mb-6 flex items-start gap-2 rounded-lg border p-3 text-sm">
		<ShieldCheck class="mt-0.5 size-4 shrink-0 text-emerald-600" />
		<p class="text-muted-foreground">
			Les secrets sont chiffrés (AES-256-GCM) sur le serveur et ne sont jamais renvoyés au navigateur. Les workflows ne
			contiennent qu'une référence : un export de workflow ne divulgue aucun mot de passe.
		</p>
	</div>

	{#if resources.credentials.length > 0}
		<div class="rounded-xl border">
			<Table.Root>
				<Table.Header>
					<Table.Row>
						<Table.Head>Identifiant</Table.Head>
						<Table.Head>Type</Table.Head>
						<Table.Head>Utilisé dans</Table.Head>
						<Table.Head>Modifié</Table.Head>
						<Table.Head class="w-12"><span class="sr-only">Actions</span></Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each resources.credentials as c (c.id)}
						{@const brand = brandOfCredential(c.kind, c.summary)}
						{@const used = workflowsOf(c)}
						<Table.Row>
							<Table.Cell>
								<button class="group flex items-center gap-3 text-left" onclick={() => edit(c)}>
									<BrandLogo {brand} class="size-9" />
									<span class="min-w-0">
										<span class="block truncate font-medium group-hover:underline">{c.name}</span>
										<span class="text-muted-foreground block truncate text-xs">{detail(c)}</span>
									</span>
								</button>
							</Table.Cell>
							<Table.Cell class="text-muted-foreground text-sm whitespace-nowrap">{resources.kind(c.kind)?.label ?? c.kind}</Table.Cell>
							<Table.Cell>
								{#if used.length === 0}
									<span class="text-muted-foreground text-xs">Aucun workflow</span>
								{:else}
									<div class="flex max-w-md flex-wrap gap-1">
										{#each used as w (w.id)}
											<a
												href={href({ name: 'editor', ws: ws.id, workflow: w.id })}
												class="hover:bg-accent hover:border-primary/40 flex max-w-60 items-center gap-1.5 rounded-full border py-0.5 pr-2 pl-1.5 text-xs transition-colors"
												title="{w.name} — {nodeList(w.nodes)}"
											>
												<Workflow class="text-muted-foreground size-3.5 shrink-0" />
												<span class="truncate">{w.name}</span>
												{#if w.nodes.length > 1}<span class="text-muted-foreground tabular-nums">×{w.nodes.length}</span>{/if}
											</a>
										{/each}
									</div>
								{/if}
							</Table.Cell>
							<Table.Cell class="text-muted-foreground whitespace-nowrap">{formatRelative(c.updatedAt)}</Table.Cell>
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
					{/each}
				</Table.Body>
			</Table.Root>
		</div>
	{/if}

	<!-- Services disponibles : un clic ouvre la création avec le type déjà choisi. -->
	<section class={resources.credentials.length > 0 ? 'mt-8' : ''} aria-labelledby="services-title">
		<h2 id="services-title" class="text-sm font-semibold">
			{resources.credentials.length > 0 ? 'Connecter un autre service' : 'Quel service voulez-vous connecter ?'}
		</h2>
		<p class="text-muted-foreground mt-1 text-sm">
			Chaque identifiant est réutilisable par tous les nœuds du workspace qui en ont besoin.
		</p>
		<div class="mt-3 grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
			{#each resources.kinds as k (k.kind)}
				<button
					type="button"
					class="hover:border-primary/40 hover:bg-accent/60 flex items-center gap-3 rounded-xl border p-3 text-left transition-colors"
					onclick={() => create(k.kind)}
				>
					<BrandLogo brand={brandOfCredential(k.kind)} class="size-10" />
					<span class="min-w-0 flex-1">
						<span class="block truncate text-sm font-medium">{k.label}</span>
						<span class="text-muted-foreground block truncate text-xs">{k.description}</span>
					</span>
					<Plus class="text-muted-foreground size-4 shrink-0" />
				</button>
			{/each}
		</div>
		{#if resources.kinds.some((k) => k.kind === 'chat_webhook')}
			<p class="text-muted-foreground mt-3 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs">
				Messageries reconnues :
				{#each ['Slack', 'Discord', 'Microsoft Teams', 'Mattermost', 'Google Chat'] as const as s (s)}
					<span class="flex items-center gap-1"><BrandLogo brand={s} class="size-4 rounded" /> {brands[s].label}</span>
				{/each}
			</p>
		{/if}
	</section>
</Page>

<CredentialDialog bind:open={dialog} credential={editing} {preset} />

<AlertDialog.Root open={toDelete !== null} onOpenChange={(o) => !o && (toDelete = null)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Supprimer « {toDelete?.name} » ?</AlertDialog.Title>
			<AlertDialog.Description>
				{#if toDelete && workflowsOf(toDelete).length > 0}
					{@const used = workflowsOf(toDelete)}
					Cet identifiant est utilisé par {used.length} workflow{used.length > 1 ? 's' : ''} ({used.map((w) => w.name).join(', ')}) :
					ses nœuds échoueront tant qu'un autre identifiant n'est pas choisi.
				{:else}
					Aucun workflow ne l'utilise actuellement.
				{/if}
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Annuler</AlertDialog.Cancel>
			<AlertDialog.Action class="bg-destructive hover:bg-destructive/90 text-white" onclick={remove}>Supprimer</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
