<script lang="ts">
	import Eye from '@lucide/svelte/icons/eye';
	import EyeOff from '@lucide/svelte/icons/eye-off';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import AuthLayout from './AuthLayout.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { api, errorMessage } from '$lib/api';
	import { passwordChecks } from '$lib/password';
	import { session } from '$lib/session.svelte';


	// Par défaut : création de compte (sauf si les inscriptions sont fermées).
	let mode = $state<'register' | 'login' | 'forgot'>(session.allowRegistration && !session.notice ? 'register' : 'login');
	let name = $state('');
	let email = $state('');
	let password = $state('');
	let confirm = $state('');
	let reveal = $state(false);
	let busy = $state(false);
	let error = $state<string | null>(null);
	/** Demande de réinitialisation envoyée. */
	let sent = $state(false);

	const registering = $derived(mode === 'register');
	const checks = $derived(passwordChecks(password, confirm));
	const canSubmit = $derived(
		!busy && email.trim() !== '' && password !== '' && (!registering || (name.trim() !== '' && checks.every((c) => c.ok))),
	);

	function switchMode(next: 'register' | 'login' | 'forgot') {
		mode = next;
		error = null;
		sent = false;
		password = '';
		confirm = '';
	}

	async function forgot(event: SubmitEvent) {
		event.preventDefault();
		if (busy || !email.trim()) return;
		busy = true;
		error = null;
		try {
			await api.forgotPassword(email.trim());
			sent = true;
		} catch (e) {
			error = errorMessage(e);
		} finally {
			busy = false;
		}
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (!canSubmit) return;
		busy = true;
		error = null;
		try {
			if (registering) await session.register(email, name, password);
			else await session.login(email, password);
		} catch (e) {
			error = errorMessage(e);
		} finally {
			busy = false;
			password = '';
			confirm = '';
		}
	}
</script>

<AuthLayout>
	{#if mode === 'forgot'}
		<Card.Root class="gap-5 border-0 bg-transparent py-0 shadow-none ring-0">
			<Card.Header class="px-0">
				<Card.Title class="text-2xl font-semibold tracking-tight">Mot de passe oublié</Card.Title>
				<Card.Description>Recevez par e-mail un lien pour choisir un nouveau mot de passe.</Card.Description>
			</Card.Header>
			<Card.Content class="px-0">
				{#if sent}
					<p class="bg-muted rounded-md p-3 text-sm" role="status">
						Si un compte utilise cette adresse, un e-mail vient de lui être envoyé. Le lien est valable 30 minutes.
					</p>
				{:else}
					<form class="space-y-4" onsubmit={forgot} novalidate>
						<div class="space-y-1.5">
							<Label for="forgot-email">E-mail</Label>
							<Input id="forgot-email" type="email" bind:value={email} autocomplete="username" maxlength={254} required />
						</div>
						{#if error}
							<p class="text-destructive text-sm" role="alert">{error}</p>
						{/if}
						<Button type="submit" size="lg" class="w-full" disabled={busy || !email.trim()}>
							{#if busy}<LoaderCircle class="animate-spin" />{/if}
							Envoyer le lien
						</Button>
					</form>
				{/if}
			</Card.Content>
			<Card.Footer class="text-muted-foreground justify-center border-t bg-transparent px-0 pt-5 text-sm">
				<button class="text-primary font-medium hover:underline" onclick={() => switchMode('login')}>Retour à la connexion</button>
			</Card.Footer>
		</Card.Root>
	{:else}
		<Card.Root class="gap-5 border-0 bg-transparent py-0 shadow-none ring-0">
			<Card.Header class="px-0">
				{#if registering}<p class="text-primary text-xs font-semibold tracking-wide uppercase">Étape 1 sur 2</p>{/if}
				<Card.Title class="text-2xl font-semibold tracking-tight">{registering ? 'Créer un compte' : 'Bon retour parmi nous'}</Card.Title>
				<Card.Description>
					{registering
						? session.firstAccount
							? 'Premier compte de ce serveur : vous en serez l’administrateur. Les autres personnes rejoindront vos workspaces sur invitation.'
							: 'Commencez par créer votre compte, puis votre premier workspace.'
						: 'Connectez-vous pour accéder à vos workspaces et workflows.'}
				</Card.Description>
			</Card.Header>
			<Card.Content class="px-0">
				<form class="space-y-4" onsubmit={submit} novalidate>
					{#if session.notice && !registering}
						<p class="bg-muted rounded-md p-2.5 text-sm">{session.notice}</p>
					{/if}
					{#if registering}
						<div class="space-y-1.5">
							<Label for="auth-name">Nom</Label>
							<Input id="auth-name" bind:value={name} autocomplete="name" maxlength={80} required />
						</div>
					{/if}
					<div class="space-y-1.5">
						<Label for="auth-email">E-mail</Label>
						<Input
							id="auth-email"
							type="email"
							bind:value={email}
							autocomplete={registering ? 'email' : 'username'}
							maxlength={254}
							required
						/>
					</div>
					<div class="space-y-1.5">
						<Label for="auth-password">Mot de passe</Label>
						<div class="relative">
							<Input
								id="auth-password"
								type={reveal ? 'text' : 'password'}
								bind:value={password}
								autocomplete={registering ? 'new-password' : 'current-password'}
								maxlength={128}
								class="pr-9"
								required
							/>
							<button
								type="button"
								class="text-muted-foreground hover:text-foreground absolute top-1/2 right-2 -translate-y-1/2"
								onclick={() => (reveal = !reveal)}
								aria-label={reveal ? 'Masquer le mot de passe' : 'Afficher le mot de passe'}
							>
								{#if reveal}<EyeOff class="size-4" />{:else}<Eye class="size-4" />{/if}
							</button>
						</div>
						{#if !registering && session.passwordReset}
							<button type="button" class="text-muted-foreground hover:text-primary text-xs hover:underline" onclick={() => switchMode('forgot')}>
								Mot de passe oublié ?
							</button>
						{/if}
					</div>
					{#if registering}
						<div class="space-y-1.5">
							<Label for="auth-confirm">Confirmer le mot de passe</Label>
							<Input
								id="auth-confirm"
								type={reveal ? 'text' : 'password'}
								bind:value={confirm}
								autocomplete="new-password"
								maxlength={128}
								required
							/>
						</div>
						<ul class="space-y-0.5 text-xs" aria-label="Exigences du mot de passe">
							{#each checks as check (check.label)}
								<li class={check.ok ? 'text-emerald-600 dark:text-emerald-400' : 'text-muted-foreground'}>
									{check.ok ? '✓' : '•'} {check.label}
								</li>
							{/each}
						</ul>
					{/if}

					{#if error}
						<p class="text-destructive text-sm" role="alert">{error}</p>
					{/if}

					<Button type="submit" size="lg" class="w-full" disabled={!canSubmit}>
						{#if busy}<LoaderCircle class="animate-spin" />{/if}
						{registering ? 'Créer mon compte' : 'Se connecter'}
					</Button>
				</form>
			</Card.Content>
			<Card.Footer class="text-muted-foreground justify-center border-t bg-transparent px-0 pt-5 text-sm">
				{#if registering}
					Déjà un compte ?
					<button class="text-primary ml-1 font-medium hover:underline" onclick={() => switchMode('login')}>Se connecter</button>
				{:else if session.allowRegistration}
					Pas encore de compte ?
					<button class="text-primary ml-1 font-medium hover:underline" onclick={() => switchMode('register')}>Créer un compte</button>
				{:else}
					<span class="text-muted-foreground">Pas de compte ? Demandez une invitation à un membre.</span>
				{/if}
			</Card.Footer>
		</Card.Root>
	{/if}
</AuthLayout>
