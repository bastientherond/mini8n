<script lang="ts">
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import { ModeWatcher } from 'mode-watcher';
	import { Button } from '$lib/components/ui/button';
	import { Toaster } from '$lib/components/ui/sonner';
	import { TooltipProvider } from '$lib/components/ui/tooltip';
	import AuthScreen from '$lib/components/AuthScreen.svelte';
	import Onboarding from '$lib/components/Onboarding.svelte';
	import PublicFormPage from '$lib/components/pages/PublicFormPage.svelte';
	import AppShell from '$lib/components/shell/AppShell.svelte';
	import { router } from '$lib/router.svelte';
	import { session } from '$lib/session.svelte';
	import { wf } from '$lib/workflow.svelte';

	let ready = $state<Promise<unknown>>(Promise.all([wf.loadCatalog(), session.init()]));

	// Déconnexion ou expiration : on vide l'éditeur.
	$effect(() => {
		if (session.status !== 'ready') wf.reset();
	});
</script>

<ModeWatcher />
<Toaster position="bottom-right" richColors />

{#snippet centered(text: string)}
	<div class="text-muted-foreground flex h-dvh items-center justify-center gap-2 text-sm">
		<LoaderCircle class="size-4 animate-spin" />
		{text}
	</div>
{/snippet}

<TooltipProvider>
	{#if router.route.name === 'form'}
		<!-- Formulaire public : accessible sans compte. -->
		<PublicFormPage token={router.route.token} />
	{:else}
	{#await ready}
		{@render centered('Chargement…')}
	{:then}
		{#if wf.loadError}
			<div class="flex h-dvh items-center justify-center p-8">
				<p class="text-destructive max-w-lg text-sm">Impossible de charger le moteur : {wf.loadError}</p>
			</div>
		{:else if session.status === 'offline'}
			<div class="flex h-dvh flex-col items-center justify-center gap-3 p-8 text-sm">
				<p>Le serveur mini8n est injoignable.</p>
				<Button variant="outline" size="sm" onclick={() => (ready = session.init())}>Réessayer</Button>
			</div>
		{:else if session.status === 'anonymous'}
			<AuthScreen />
		{:else if session.status === 'ready' && session.workspaces.length === 0}
			<Onboarding />
		{:else if session.status === 'ready'}
			<AppShell />
		{:else}
			{@render centered('Chargement…')}
		{/if}
	{/await}
	{/if}
</TooltipProvider>
