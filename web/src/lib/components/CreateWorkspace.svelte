<script lang="ts">
	import Building2 from '@lucide/svelte/icons/building-2';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { errorMessage } from '$lib/api';
	import { router } from '$lib/router.svelte';
	import { session } from '$lib/session.svelte';

	let { oncreated }: { oncreated?: () => void } = $props();

	let name = $state('');
	let busy = $state(false);
	let error = $state<string | null>(null);

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (!name.trim() || busy) return;
		busy = true;
		error = null;
		try {
			const ws = await session.createWorkspace(name.trim());
			name = '';
			oncreated?.();
			router.go({ name: 'home', ws: ws.id });
		} catch (e) {
			error = errorMessage(e);
		} finally {
			busy = false;
		}
	}
</script>

<form class="space-y-4" onsubmit={submit}>
	<div class="space-y-1.5">
		<Label for="ws-name">Nom du workspace</Label>
		<div class="relative">
			<Building2 class="text-muted-foreground absolute top-1/2 left-2.5 size-4 -translate-y-1/2" />
			<!-- svelte-ignore a11y_autofocus -->
			<Input id="ws-name" bind:value={name} placeholder="Ex. Équipe data" maxlength={80} class="pl-8" autofocus required />
		</div>
		<p class="text-muted-foreground text-xs">Un workspace regroupe vos workflows et les personnes qui y collaborent.</p>
	</div>
	{#if error}<p class="text-destructive text-sm" role="alert">{error}</p>{/if}
	<Button type="submit" class="w-full" disabled={!name.trim() || busy}>
		{#if busy}<LoaderCircle class="animate-spin" />{/if}
		Créer le workspace
	</Button>
</form>
