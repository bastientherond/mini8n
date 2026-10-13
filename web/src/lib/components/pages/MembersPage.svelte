<script lang="ts">
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import UserPlus from '@lucide/svelte/icons/user-plus';
	import { toast } from 'svelte-sonner';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import * as Select from '$lib/components/ui/select';
	import * as Table from '$lib/components/ui/table';
	import Copy from '@lucide/svelte/icons/copy';
	import { api, errorMessage, type Member, type PendingInvitation, type Role } from '$lib/api';
	import { formatRelative } from '$lib/icons';
	import { router } from '$lib/router.svelte';
	import { session } from '$lib/session.svelte';
	import Page from './Page.svelte';

	const ROLE_LABELS: Record<Role, string> = { owner: 'Propriétaire', member: 'Membre' };

	const ws = $derived(session.current!);
	const isOwner = $derived(ws.role === 'owner');

	let members = $state.raw<Member[]>([]);
	let invitations = $state.raw<PendingInvitation[]>([]);
	/** Lien d'invitation à transmettre (pas d'envoi d'e-mail configuré). */
	let shareLink = $state<{ email: string; link: string } | null>(null);
	let loading = $state(true);
	let inviteEmail = $state('');
	let inviteRole = $state<Role>('member');
	let busy = $state(false);

	$effect(() => {
		const id = ws.id;
		loading = true;
		shareLink = null;
		api.members(id)
			.then((m) => (members = m))
			.catch((e) => toast.error(errorMessage(e)))
			.finally(() => (loading = false));
		if (isOwner) void loadInvitations(id);
		else invitations = [];
	});

	async function loadInvitations(id = ws.id) {
		try {
			invitations = await api.invitations(id);
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	async function guarded(action: () => Promise<void>) {
		busy = true;
		try {
			await action();
		} catch (e) {
			toast.error(errorMessage(e));
		} finally {
			busy = false;
		}
	}

	function invite(event: SubmitEvent) {
		event.preventDefault();
		const email = inviteEmail.trim();
		if (!email) return;
		return guarded(async () => {
			const res = await api.addMember(ws.id, email, inviteRole);
			members = res.members;
			inviteEmail = '';
			if (res.invitation) {
				await loadInvitations();
				if (res.invitation.link) shareLink = { email, link: res.invitation.link };
				else toast.success(`Invitation envoyée à ${email}`);
			} else {
				toast.success(`${email} ajouté au workspace`);
				await session.refreshWorkspaces();
			}
		});
	}

	const revoke = (i: PendingInvitation) =>
		guarded(async () => {
			await api.revokeInvitation(ws.id, i.id);
			invitations = invitations.filter((x) => x.id !== i.id);
			toast.success(`Invitation de ${i.email} annulée`);
		});

	async function copyLink(link: string) {
		try {
			await navigator.clipboard.writeText(link);
			toast.success('Lien copié');
		} catch {
			toast.error('Copie impossible : sélectionnez le lien manuellement.');
		}
	}

	const setRole = (m: Member, role: Role) =>
		guarded(async () => {
			members = await api.setRole(ws.id, m.userId, role);
			await session.refreshWorkspaces();
		});

	const remove = (m: Member) =>
		guarded(async () => {
			const self = m.userId === session.user?.id;
			await api.removeMember(ws.id, m.userId);
			await session.refreshWorkspaces();
			if (self) {
				toast.success('Vous avez quitté le workspace');
				router.go({ name: 'root' });
			} else {
				members = members.filter((x) => x.userId !== m.userId);
				toast.success(`${m.name} a été retiré`);
			}
		});
</script>

<Page title="Membres" description="Personnes ayant accès au workspace « {ws.name} ».">
	{#if isOwner}
		<Card.Root class="mb-6">
			<Card.Header>
				<Card.Title class="text-base">Ajouter un membre</Card.Title>
				<Card.Description>
					Une personne au compte vérifié est ajoutée directement ; sinon, elle reçoit une invitation à accepter (valable 7 jours).
				</Card.Description>
			</Card.Header>
			<Card.Content>
				<form class="flex flex-wrap items-end gap-2" onsubmit={invite}>
					<div class="min-w-64 flex-1 space-y-1.5">
						<Label for="invite-email">E-mail</Label>
						<Input id="invite-email" type="email" bind:value={inviteEmail} placeholder="nom@exemple.com" autocomplete="off" />
					</div>
					<div class="space-y-1.5">
						<Label>Rôle</Label>
						<Select.Root type="single" bind:value={inviteRole}>
							<Select.Trigger class="w-40" aria-label="Rôle">{ROLE_LABELS[inviteRole]}</Select.Trigger>
							<Select.Content>
								<Select.Item value="member" label="Membre">Membre</Select.Item>
								<Select.Item value="owner" label="Propriétaire">Propriétaire</Select.Item>
							</Select.Content>
						</Select.Root>
					</div>
					<Button type="submit" disabled={busy || !inviteEmail.trim()}><UserPlus /> Ajouter</Button>
				</form>
				{#if shareLink}
					<div class="border-primary/40 bg-primary/5 mt-4 space-y-2 rounded-lg border p-3 text-sm" role="status">
						<p>
							Aucun envoi d'e-mail n'est configuré : transmettez ce lien à <strong>{shareLink.email}</strong> (valable 7 jours, une seule fois).
						</p>
						<div class="flex items-center gap-2">
							<code class="bg-muted min-w-0 flex-1 truncate rounded px-2 py-1 font-mono text-xs">{shareLink.link}</code>
							<Button size="sm" variant="outline" onclick={() => copyLink(shareLink!.link)}><Copy /> Copier</Button>
						</div>
					</div>
				{/if}
				{#if invitations.length > 0}
					<div class="mt-5">
						<h3 class="text-muted-foreground mb-2 text-xs font-semibold tracking-wide uppercase">Invitations en attente</h3>
						<ul class="divide-y rounded-lg border text-sm">
							{#each invitations as i (i.id)}
								<li class="flex items-center gap-3 px-3 py-2">
									<div class="min-w-0 flex-1">
										<p class="truncate font-medium">{i.email}</p>
										<p class="text-muted-foreground text-xs">
											{ROLE_LABELS[i.role]} · {i.delivered ? 'envoyée par e-mail' : 'lien à transmettre'} · expire {formatRelative(i.expiresAt)}
										</p>
									</div>
									<Button size="sm" variant="ghost" disabled={busy} onclick={() => revoke(i)} aria-label="Annuler l'invitation de {i.email}">
										<Trash2 /> Annuler
									</Button>
								</li>
							{/each}
						</ul>
					</div>
				{/if}
			</Card.Content>
		</Card.Root>
	{/if}

	<div class="rounded-xl border">
		<Table.Root>
			<Table.Header>
				<Table.Row>
					<Table.Head>Nom</Table.Head>
					<Table.Head>Membre depuis</Table.Head>
					<Table.Head>Rôle</Table.Head>
					<Table.Head class="w-12"><span class="sr-only">Actions</span></Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#if loading}
					<Table.Row>
						<Table.Cell colspan={4} class="text-muted-foreground py-8">
							<span class="flex items-center justify-center gap-2"><LoaderCircle class="size-4 animate-spin" /> Chargement…</span>
						</Table.Cell>
					</Table.Row>
				{:else}
					{#each members as m (m.userId)}
						{@const self = m.userId === session.user?.id}
						<Table.Row>
							<Table.Cell>
								<div class="font-medium">{m.name}{self ? ' (vous)' : ''}</div>
								<div class="text-muted-foreground text-xs">{m.email}</div>
							</Table.Cell>
							<Table.Cell class="text-muted-foreground">{formatRelative(m.createdAt)}</Table.Cell>
							<Table.Cell>
								{#if isOwner}
									<Select.Root type="single" value={m.role} onValueChange={(v) => setRole(m, v as Role)} disabled={busy}>
										<Select.Trigger size="sm" class="w-36" aria-label="Rôle de {m.name}">{ROLE_LABELS[m.role]}</Select.Trigger>
										<Select.Content>
											<Select.Item value="member" label="Membre">Membre</Select.Item>
											<Select.Item value="owner" label="Propriétaire">Propriétaire</Select.Item>
										</Select.Content>
									</Select.Root>
								{:else}
									<Badge variant="secondary">{ROLE_LABELS[m.role]}</Badge>
								{/if}
							</Table.Cell>
							<Table.Cell>
								{#if isOwner || self}
									<Button
										variant="ghost"
										size="icon-sm"
										disabled={busy}
										onclick={() => remove(m)}
										aria-label={self ? 'Quitter le workspace' : `Retirer ${m.name}`}
										title={self ? 'Quitter le workspace' : 'Retirer'}
									>
										<Trash2 />
									</Button>
								{/if}
							</Table.Cell>
						</Table.Row>
					{/each}
				{/if}
			</Table.Body>
		</Table.Root>
	</div>
</Page>
