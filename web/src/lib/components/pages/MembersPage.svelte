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
	import { api, errorMessage, type Member, type Role } from '$lib/api';
	import { formatRelative } from '$lib/icons';
	import { router } from '$lib/router.svelte';
	import { session } from '$lib/session.svelte';
	import Page from './Page.svelte';

	const ROLE_LABELS: Record<Role, string> = { owner: 'Propriétaire', member: 'Membre' };

	const ws = $derived(session.current!);
	const isOwner = $derived(ws.role === 'owner');

	let members = $state.raw<Member[]>([]);
	let loading = $state(true);
	let inviteEmail = $state('');
	let inviteRole = $state<Role>('member');
	let busy = $state(false);

	$effect(() => {
		const id = ws.id;
		loading = true;
		api.members(id)
			.then((m) => (members = m))
			.catch((e) => toast.error(errorMessage(e)))
			.finally(() => (loading = false));
	});

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
			members = await api.addMember(ws.id, email, inviteRole);
			toast.success(`${email} ajouté au workspace`);
			inviteEmail = '';
			await session.refreshWorkspaces();
		});
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
				<Card.Description>La personne doit déjà avoir un compte mini8n.</Card.Description>
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
