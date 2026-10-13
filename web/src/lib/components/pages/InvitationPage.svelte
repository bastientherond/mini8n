<script lang="ts">
	// Lien d'invitation : création du compte, ou acceptation avec le compte connecté.
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import AuthLayout from '../AuthLayout.svelte';
	import AuthScreen from '../AuthScreen.svelte';
	import NewPasswordFields from '../NewPasswordFields.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { api, errorMessage, type InvitationInfo } from '$lib/api';
	import { router } from '$lib/router.svelte';
	import { session } from '$lib/session.svelte';

	let { token }: { token: string } = $props();

	let info = $state<InvitationInfo | null>(null);
	let loadError = $state<string | null>(null);
	let name = $state('');
	let password = $state('');
	let valid = $state(false);
	let busy = $state(false);
	let error = $state<string | null>(null);

	$effect(() => {
		info = null;
		loadError = null;
		api.invitation(token)
			.then((i) => (info = i))
			.catch((e) => (loadError = errorMessage(e)));
	});

	const wrongAccount = $derived(
		!!info && session.status === 'ready' && session.user?.email.toLowerCase() !== info.email,
	);

	async function joined(workspaceId: string) {
		await session.reload();
		session.select(workspaceId);
		router.replace({ name: 'home', ws: workspaceId });
	}

	async function run(action: () => Promise<{ workspaceId: string }>) {
		busy = true;
		error = null;
		try {
			await joined((await action()).workspaceId);
		} catch (e) {
			error = errorMessage(e);
		} finally {
			busy = false;
		}
	}

	function register(event: SubmitEvent) {
		event.preventDefault();
		if (!valid || !name.trim() || busy) return;
		void run(() => api.registerInvited(token, name.trim(), password));
	}
</script>

{#if info && session.status !== 'ready' && info.accountExists}
	<!-- Compte existant : connexion, puis acceptation sur cette même page. -->
	<AuthScreen />
{:else}
	<AuthLayout>
		<Card.Root class="gap-5 border-0 bg-transparent py-0 shadow-none ring-0">
			<Card.Header class="px-0">
				<Card.Title class="text-2xl font-semibold tracking-tight">Invitation</Card.Title>
				{#if info}
					<Card.Description>
						{info.invitedBy ?? 'Un membre'} vous invite à rejoindre le workspace « {info.workspace} » ({info.email}).
					</Card.Description>
				{/if}
			</Card.Header>
			<Card.Content class="space-y-4 px-0 text-sm">
				{#if loadError}
					<p class="text-destructive" role="alert">{loadError}</p>
					<Button variant="outline" class="w-full" onclick={() => router.replace({ name: 'root' })}>Continuer</Button>
				{:else if !info}
					<p class="text-muted-foreground flex items-center gap-2"><LoaderCircle class="size-4 animate-spin" /> Chargement…</p>
				{:else if session.status === 'ready'}
					{#if wrongAccount}
						<p class="text-destructive" role="alert">
							Vous êtes connecté avec {session.user?.email} : cette invitation est destinée à {info.email}.
						</p>
						<Button variant="outline" class="w-full" disabled={busy} onclick={() => session.logout()}>
							Se déconnecter pour l'accepter
						</Button>
					{:else}
						<Button class="w-full" size="lg" disabled={busy} onclick={() => run(() => api.acceptInvitation(token))}>
							{#if busy}<LoaderCircle class="animate-spin" />{/if}
							Rejoindre « {info.workspace} »
						</Button>
					{/if}
				{:else}
					<form class="space-y-4" onsubmit={register} novalidate>
						<p class="text-muted-foreground">Créez votre compte pour accepter l'invitation.</p>
						<div class="space-y-1.5">
							<Label for="invite-name">Nom</Label>
							<Input id="invite-name" bind:value={name} autocomplete="name" maxlength={80} required />
						</div>
						<div class="space-y-1.5">
							<Label for="invite-email-ro">E-mail</Label>
							<Input id="invite-email-ro" value={info.email} readonly />
						</div>
						<NewPasswordFields bind:password bind:valid id="invite-password" />
						<Button type="submit" size="lg" class="w-full" disabled={!valid || !name.trim() || busy}>
							{#if busy}<LoaderCircle class="animate-spin" />{/if}
							Créer mon compte et rejoindre
						</Button>
					</form>
				{/if}
				{#if error}
					<p class="text-destructive" role="alert">{error}</p>
				{/if}
			</Card.Content>
		</Card.Root>
	</AuthLayout>
{/if}
