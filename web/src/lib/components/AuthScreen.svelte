<script lang="ts">
	import Eye from '@lucide/svelte/icons/eye';
	import EyeOff from '@lucide/svelte/icons/eye-off';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import Workflow from '@lucide/svelte/icons/workflow';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { errorMessage } from '$lib/api';
	import { session } from '$lib/session.svelte';

	const MIN = 12;

	// Par défaut : création de compte (sauf si les inscriptions sont fermées).
	let mode = $state<'register' | 'login'>(session.allowRegistration && !session.notice ? 'register' : 'login');
	let name = $state('');
	let email = $state('');
	let password = $state('');
	let confirm = $state('');
	let reveal = $state(false);
	let busy = $state(false);
	let error = $state<string | null>(null);

	const registering = $derived(mode === 'register');
	const checks = $derived([
		{ ok: password.length >= MIN, label: `${MIN} caractères minimum` },
		{ ok: new Set(password).size >= 5, label: 'Pas trop répétitif' },
		{ ok: password.length > 0 && password === confirm, label: 'Les deux saisies correspondent' },
	]);
	const canSubmit = $derived(
		!busy && email.trim() !== '' && password !== '' && (!registering || (name.trim() !== '' && checks.every((c) => c.ok))),
	);

	function switchMode(next: 'register' | 'login') {
		mode = next;
		error = null;
		password = '';
		confirm = '';
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

<div class="bg-muted/40 flex min-h-dvh items-center justify-center p-4">
	<div class="w-full max-w-sm space-y-6">
		<div class="flex items-center justify-center gap-2 text-lg font-semibold">
			<span class="bg-primary text-primary-foreground flex size-8 items-center justify-center rounded-lg">
				<Workflow class="size-5" />
			</span>
			mini8n
		</div>

		<Card.Root>
			<Card.Header>
				<Card.Title class="text-xl">{registering ? 'Créer un compte' : 'Se connecter'}</Card.Title>
				<Card.Description>
					{registering
						? 'Commencez par créer votre compte, puis votre premier workspace.'
						: 'Accédez à vos workspaces et workflows.'}
				</Card.Description>
			</Card.Header>
			<Card.Content>
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

					<Button type="submit" class="w-full" disabled={!canSubmit}>
						{#if busy}<LoaderCircle class="animate-spin" />{/if}
						{registering ? 'Créer mon compte' : 'Se connecter'}
					</Button>
				</form>
			</Card.Content>
			<Card.Footer class="justify-center text-sm">
				{#if registering}
					Déjà un compte ?
					<button class="ml-1 font-medium underline underline-offset-4" onclick={() => switchMode('login')}>Se connecter</button>
				{:else if session.allowRegistration}
					Pas encore de compte ?
					<button class="ml-1 font-medium underline underline-offset-4" onclick={() => switchMode('register')}>Créer un compte</button>
				{:else}
					<span class="text-muted-foreground">Les inscriptions sont fermées.</span>
				{/if}
			</Card.Footer>
		</Card.Root>
	</div>
</div>
