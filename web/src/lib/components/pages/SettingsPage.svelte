<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { api, errorMessage } from '$lib/api';
	import { router } from '$lib/router.svelte';
	import { session } from '$lib/session.svelte';
	import Page from './Page.svelte';

	const ws = $derived(session.current!);
	const isOwner = $derived(ws.role === 'owner');

	let name = $state('');
	let confirmName = $state('');
	let busy = $state(false);

	$effect(() => {
		name = ws.name;
		confirmName = '';
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

	function rename(event: SubmitEvent) {
		event.preventDefault();
		return guarded(async () => {
			await api.renameWorkspace(ws.id, name.trim());
			await session.refreshWorkspaces();
			toast.success('Workspace renommé');
		});
	}

	const destroy = () =>
		guarded(async () => {
			const label = ws.name;
			await api.deleteWorkspace(ws.id);
			await session.refreshWorkspaces();
			toast.success(`Workspace « ${label} » supprimé`);
			router.go({ name: 'root' });
		});

	const leave = () =>
		guarded(async () => {
			await api.removeMember(ws.id, session.user!.id);
			await session.refreshWorkspaces();
			toast.success('Vous avez quitté le workspace');
			router.go({ name: 'root' });
		});
</script>

<Page title="Paramètres" description="Configuration du workspace « {ws.name} ».">
	<div class="space-y-6">
		<Card.Root>
			<Card.Header>
				<Card.Title class="text-base">Général</Card.Title>
				<Card.Description>
					{isOwner ? 'Nom affiché pour tous les membres.' : 'Seuls les propriétaires peuvent modifier le workspace.'}
				</Card.Description>
			</Card.Header>
			<Card.Content>
				<form class="flex max-w-lg items-end gap-2" onsubmit={rename}>
					<div class="flex-1 space-y-1.5">
						<Label for="ws-name">Nom du workspace</Label>
						<Input id="ws-name" bind:value={name} maxlength={80} disabled={!isOwner} />
					</div>
					{#if isOwner}
						<Button type="submit" variant="outline" disabled={busy || !name.trim() || name.trim() === ws.name}>Enregistrer</Button>
					{/if}
				</form>
				<p class="text-muted-foreground mt-3 font-mono text-xs">ID : {ws.id}</p>
			</Card.Content>
		</Card.Root>

		{#if isOwner}
			<Card.Root class="border-destructive/40">
				<Card.Header>
					<Card.Title class="text-destructive text-base">Supprimer le workspace</Card.Title>
					<Card.Description>
						Supprime définitivement le workspace et tous ses workflows, pour tous les membres. Tapez « {ws.name} » pour confirmer.
					</Card.Description>
				</Card.Header>
				<Card.Content class="flex max-w-lg gap-2">
					<Input bind:value={confirmName} placeholder={ws.name} aria-label="Confirmation du nom" />
					<Button variant="destructive" disabled={busy || confirmName !== ws.name} onclick={destroy}>Supprimer</Button>
				</Card.Content>
			</Card.Root>
		{:else}
			<Card.Root>
				<Card.Header>
					<Card.Title class="text-base">Quitter le workspace</Card.Title>
					<Card.Description>Vous perdrez l'accès à ses workflows. Un propriétaire pourra vous ajouter à nouveau.</Card.Description>
				</Card.Header>
				<Card.Content>
					<Button variant="outline" disabled={busy} onclick={leave}>Quitter « {ws.name} »</Button>
				</Card.Content>
			</Card.Root>
		{/if}
	</div>
</Page>
