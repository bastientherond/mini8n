<script lang="ts">
	import FileText from '@lucide/svelte/icons/file-text';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { cn } from '$lib/utils';
	import { createAndOpenWorkflow, deleteWorkflow, ui } from '$lib/ui.svelte';
	import CreateWorkspace from '../CreateWorkspace.svelte';

	const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);
	const mod = isMac ? '⌘' : 'Ctrl';

	let workflowName = $state('');
	let template = $state<'empty' | 'example'>('empty');
	let creating = $state(false);

	$effect(() => {
		if (ui.newWorkflow) {
			workflowName = '';
			template = 'empty';
		}
	});

	async function submitWorkflow(event: SubmitEvent) {
		event.preventDefault();
		creating = true;
		try {
			const name = workflowName.trim() || (template === 'example' ? 'Exemple : ventes' : 'Nouveau workflow');
			await createAndOpenWorkflow(name, template === 'example');
			ui.newWorkflow = false;
		} finally {
			creating = false;
		}
	}

	const shortcuts = [
		{ keys: [mod, 'K'], label: 'Ouvrir la palette de commandes' },
		{ keys: [mod, 'Entrée'], label: 'Exécuter le workflow (éditeur)' },
		{ keys: ['Retour arrière'], label: 'Supprimer les nœuds / liens sélectionnés' },
		{ keys: ['Maj', 'glisser'], label: 'Sélection multiple sur le canevas' },
		{ keys: ['Échap'], label: 'Fermer un menu ou un dialogue' },
	];
</script>

<Dialog.Root bind:open={ui.newWorkspace}>
	<Dialog.Content class="sm:max-w-sm">
		<Dialog.Header>
			<Dialog.Title>Nouveau workspace</Dialog.Title>
			<Dialog.Description>Vous en serez le propriétaire.</Dialog.Description>
		</Dialog.Header>
		<CreateWorkspace oncreated={() => (ui.newWorkspace = false)} />
	</Dialog.Content>
</Dialog.Root>

<Dialog.Root bind:open={ui.newWorkflow}>
	<Dialog.Content class="sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>Nouveau workflow</Dialog.Title>
		</Dialog.Header>
		<form class="space-y-4" onsubmit={submitWorkflow}>
			<div class="space-y-1.5">
				<Label for="new-workflow-name">Nom</Label>
				<!-- svelte-ignore a11y_autofocus -->
				<Input id="new-workflow-name" bind:value={workflowName} placeholder="Nouveau workflow" maxlength={120} autofocus />
			</div>
			<div class="grid grid-cols-2 gap-2" role="radiogroup" aria-label="Modèle">
				{#each [{ value: 'empty', label: 'Vide', desc: 'Canevas vierge', icon: FileText }, { value: 'example', label: 'Exemple', desc: 'Ventes : formule, filtre, agrégat, jointure', icon: Sparkles }] as opt (opt.value)}
					{@const Icon = opt.icon}
					<button
						type="button"
						role="radio"
						aria-checked={template === opt.value}
						class={cn(
							'flex flex-col items-start gap-1 rounded-lg border p-3 text-left transition-colors',
							template === opt.value ? 'border-primary ring-primary/20 ring-2' : 'hover:bg-accent',
						)}
						onclick={() => (template = opt.value as 'empty' | 'example')}
					>
						<Icon class="text-muted-foreground size-4" />
						<span class="text-sm font-medium">{opt.label}</span>
						<span class="text-muted-foreground text-xs">{opt.desc}</span>
					</button>
				{/each}
			</div>
			<Dialog.Footer>
				<Button type="submit" disabled={creating}>
					{#if creating}<LoaderCircle class="animate-spin" />{/if}
					Créer et ouvrir
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<Dialog.Root bind:open={ui.shortcuts}>
	<Dialog.Content class="sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>Raccourcis clavier</Dialog.Title>
		</Dialog.Header>
		<ul class="divide-y">
			{#each shortcuts as s (s.label)}
				<li class="flex items-center justify-between gap-4 py-2 text-sm">
					<span>{s.label}</span>
					<span class="flex gap-1">
						{#each s.keys as k (k)}<kbd class="bg-muted rounded border px-1.5 py-0.5 font-mono text-xs">{k}</kbd>{/each}
					</span>
				</li>
			{/each}
		</ul>
	</Dialog.Content>
</Dialog.Root>

<AlertDialog.Root open={ui.confirmDelete !== null} onOpenChange={(o) => !o && (ui.confirmDelete = null)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Supprimer « {ui.confirmDelete?.name} » ?</AlertDialog.Title>
			<AlertDialog.Description>Le workflow sera supprimé pour tous les membres du workspace.</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Annuler</AlertDialog.Cancel>
			<AlertDialog.Action
				class="bg-destructive text-white hover:bg-destructive/90"
				onclick={() => {
					const target = ui.confirmDelete;
					ui.confirmDelete = null;
					if (target) void deleteWorkflow(target.id);
				}}>Supprimer</AlertDialog.Action
			>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
