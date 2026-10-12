<script lang="ts">
	import Workflow from '@lucide/svelte/icons/workflow';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import { session } from '$lib/session.svelte';
	import CreateWorkspace from './CreateWorkspace.svelte';
</script>

<!-- Étape 2 après l'inscription : créer un premier workspace. -->
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
				<p class="text-muted-foreground text-xs font-medium tracking-wide uppercase">Étape 2 sur 2</p>
				<Card.Title class="text-xl">Créez votre workspace</Card.Title>
				<Card.Description>Bienvenue {session.user?.name} ! Il vous faut un workspace pour commencer.</Card.Description>
			</Card.Header>
			<Card.Content>
				<CreateWorkspace />
			</Card.Content>
			<Card.Footer class="justify-center">
				<Button variant="link" size="sm" class="text-muted-foreground" onclick={() => session.logout()}>
					Se déconnecter ({session.user?.email})
				</Button>
			</Card.Footer>
		</Card.Root>
	</div>
</div>
