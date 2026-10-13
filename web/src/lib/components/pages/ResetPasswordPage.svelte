<script lang="ts">
	// Lien « mot de passe oublié » reçu par e-mail.
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import AuthLayout from '../AuthLayout.svelte';
	import NewPasswordFields from '../NewPasswordFields.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import { api, errorMessage } from '$lib/api';
	import { router } from '$lib/router.svelte';
	import { session } from '$lib/session.svelte';

	let { token }: { token: string } = $props();

	let password = $state('');
	let valid = $state(false);
	let busy = $state(false);
	let done = $state(false);
	let error = $state<string | null>(null);

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (!valid || busy) return;
		busy = true;
		error = null;
		try {
			await api.resetPassword(token, password);
			done = true;
		} catch (e) {
			error = errorMessage(e);
		} finally {
			busy = false;
			password = '';
		}
	}

	function toLogin() {
		session.notice = 'Mot de passe modifié : connectez-vous avec le nouveau.';
		session.status = 'anonymous';
		router.replace({ name: 'root' });
	}
</script>

<AuthLayout>
	<Card.Root class="gap-5 border-0 bg-transparent py-0 shadow-none ring-0">
		<Card.Header class="px-0">
			<Card.Title class="text-2xl font-semibold tracking-tight">Nouveau mot de passe</Card.Title>
			<Card.Description>Toutes vos sessions seront déconnectées.</Card.Description>
		</Card.Header>
		<Card.Content class="px-0">
			{#if done}
				<div class="space-y-4" role="status">
					<p class="bg-muted rounded-md p-3 text-sm">
						Votre mot de passe a été modifié. Par sécurité, vos sessions et vos jetons d'accès des IA ont été révoqués.
					</p>
					<Button class="w-full" size="lg" onclick={toLogin}>Se connecter</Button>
				</div>
			{:else}
				<form class="space-y-4" onsubmit={submit} novalidate>
					<NewPasswordFields bind:password bind:valid />
					{#if error}
						<p class="text-destructive text-sm" role="alert">{error}</p>
					{/if}
					<Button type="submit" size="lg" class="w-full" disabled={!valid || busy}>
						{#if busy}<LoaderCircle class="animate-spin" />{/if}
						Enregistrer
					</Button>
				</form>
			{/if}
		</Card.Content>
	</Card.Root>
</AuthLayout>
