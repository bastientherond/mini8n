<script lang="ts">
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import { toast } from 'svelte-sonner';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { api, errorMessage } from '$lib/api';
	import { session } from '$lib/session.svelte';
	import { logout } from '$lib/ui.svelte';
	import Page from './Page.svelte';

	let sending = $state(false);

	async function sendVerification() {
		sending = true;
		try {
			await api.sendVerification();
			toast.success(`Lien de vérification envoyé à ${session.user?.email}`);
		} catch (e) {
			toast.error(errorMessage(e));
		} finally {
			sending = false;
		}
	}

	let current = $state('');
	let next = $state('');
	let confirm = $state('');
	let busy = $state(false);
	let error = $state<string | null>(null);

	const valid = $derived(current !== '' && next.length >= 12 && next === confirm);

	async function changePassword(event: SubmitEvent) {
		event.preventDefault();
		if (!valid) return;
		busy = true;
		error = null;
		try {
			await api.changePassword(current, next);
			current = next = confirm = '';
			toast.success('Mot de passe modifié. Vos autres sessions ont été déconnectées.');
		} catch (e) {
			error = errorMessage(e);
		} finally {
			busy = false;
		}
	}
</script>

<Page title="Mon compte" description="Profil et sécurité de votre compte.">
	<div class="space-y-6">
		<Card.Root>
			<Card.Header>
				<Card.Title class="text-base">Profil</Card.Title>
			</Card.Header>
			<Card.Content class="grid max-w-lg gap-4 sm:grid-cols-2">
				<div class="space-y-1.5">
					<Label for="account-name">Nom</Label>
					<Input id="account-name" value={session.user?.name} readonly />
				</div>
				<div class="space-y-1.5">
					<Label for="account-email">E-mail</Label>
					<Input id="account-email" value={session.user?.email} readonly />
				</div>
				<div class="flex flex-wrap items-center gap-2 text-xs sm:col-span-2">
					{#if session.user?.emailVerified}
						<span class="text-emerald-600 dark:text-emerald-400">✓ Adresse vérifiée</span>
					{:else}
						<span class="text-muted-foreground">Adresse non vérifiée : on vous ajoute à un workspace par invitation.</span>
						{#if session.canVerifyEmail}
							<Button size="sm" variant="outline" disabled={sending} onclick={sendVerification}>Vérifier mon adresse</Button>
						{/if}
					{/if}
				</div>
				<p class="text-muted-foreground text-xs sm:col-span-2">
					Membre de {session.workspaces.length} workspace{session.workspaces.length > 1 ? 's' : ''}.
				</p>
			</Card.Content>
		</Card.Root>

		<Card.Root>
			<Card.Header>
				<Card.Title class="text-base">Mot de passe</Card.Title>
				<Card.Description>12 caractères minimum. Les autres sessions seront déconnectées.</Card.Description>
			</Card.Header>
			<Card.Content>
				<form class="grid max-w-lg gap-3" onsubmit={changePassword}>
					<div class="space-y-1.5">
						<Label for="pw-current">Mot de passe actuel</Label>
						<Input id="pw-current" type="password" bind:value={current} autocomplete="current-password" maxlength={128} />
					</div>
					<div class="space-y-1.5">
						<Label for="pw-new">Nouveau mot de passe</Label>
						<Input id="pw-new" type="password" bind:value={next} autocomplete="new-password" maxlength={128} />
					</div>
					<div class="space-y-1.5">
						<Label for="pw-confirm">Confirmer le nouveau mot de passe</Label>
						<Input id="pw-confirm" type="password" bind:value={confirm} autocomplete="new-password" maxlength={128} />
					</div>
					{#if error}<p class="text-destructive text-sm" role="alert">{error}</p>{/if}
					<div>
						<Button type="submit" disabled={!valid || busy}>
							{#if busy}<LoaderCircle class="animate-spin" />{/if}
							Mettre à jour
						</Button>
					</div>
				</form>
			</Card.Content>
		</Card.Root>

		<Card.Root>
			<Card.Header>
				<Card.Title class="text-base">Sessions</Card.Title>
				<Card.Description>Déconnecte tous vos appareils, y compris celui-ci.</Card.Description>
			</Card.Header>
			<Card.Content>
				<Button variant="outline" onclick={() => logout(true)}>Déconnecter tous les appareils</Button>
			</Card.Content>
		</Card.Root>
	</div>
</Page>
