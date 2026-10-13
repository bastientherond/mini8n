<script lang="ts">
	// Lien de vérification d'adresse reçu par e-mail.
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import AuthLayout from '../AuthLayout.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import { api, errorMessage } from '$lib/api';
	import { router } from '$lib/router.svelte';
	import { session } from '$lib/session.svelte';

	let { token }: { token: string } = $props();

	// Une seule tentative par lien : le jeton est à usage unique.
	let result = $state<Promise<void>>(new Promise(() => {}));
	$effect(() => {
		result = api.verifyEmail(token).then(async () => {
			if (session.status === 'ready') await session.reload();
		});
	});
</script>

<AuthLayout>
	<Card.Root class="gap-5 border-0 bg-transparent py-0 shadow-none ring-0">
		<Card.Header class="px-0">
			<Card.Title class="text-2xl font-semibold tracking-tight">Vérification de l'adresse</Card.Title>
		</Card.Header>
		<Card.Content class="space-y-4 px-0 text-sm">
			{#await result}
				<p class="text-muted-foreground flex items-center gap-2"><LoaderCircle class="size-4 animate-spin" /> Vérification…</p>
			{:then}
				<p class="bg-muted rounded-md p-3" role="status">Votre adresse e-mail est vérifiée.</p>
			{:catch e}
				<p class="text-destructive" role="alert">{errorMessage(e)}</p>
			{/await}
			<Button variant="outline" class="w-full" onclick={() => router.replace({ name: 'root' })}>Continuer</Button>
		</Card.Content>
	</Card.Root>
</AuthLayout>
