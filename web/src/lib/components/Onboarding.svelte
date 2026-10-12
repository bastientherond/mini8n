<script lang="ts">
	import AuthLayout from './AuthLayout.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import { session } from '$lib/session.svelte';
	import CreateWorkspace from './CreateWorkspace.svelte';
</script>

<!-- Étape 2 après l'inscription : créer un premier workspace. -->
<AuthLayout>
		<Card.Root class="gap-5 border-0 bg-transparent py-0 shadow-none ring-0">
			<Card.Header class="px-0">
				<p class="text-primary text-xs font-semibold tracking-wide uppercase">Étape 2 sur 2</p>
				<Card.Title class="text-2xl font-semibold tracking-tight">Créez votre workspace</Card.Title>
				<Card.Description>Bienvenue {session.user?.name} ! Il vous faut un workspace pour commencer.</Card.Description>
			</Card.Header>
			<Card.Content class="px-0">
				<CreateWorkspace />
			</Card.Content>
			<Card.Footer class="justify-center border-t bg-transparent px-0 pt-4">
				<Button variant="link" size="sm" class="text-muted-foreground" onclick={() => session.logout()}>
					Se déconnecter ({session.user?.email})
				</Button>
			</Card.Footer>
		</Card.Root>
</AuthLayout>
