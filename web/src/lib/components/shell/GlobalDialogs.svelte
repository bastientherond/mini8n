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
		{
			title: 'Général',
			items: [
				{ keys: [mod, 'K'], label: 'Palette de commandes' },
				{ keys: [mod, 'Entrée'], label: 'Exécuter le workflow' },
				{ keys: ['/'], label: 'Rechercher un nœud dans la palette' },
				{ keys: ['Échap'], label: 'Fermer un menu, annuler le geste en cours' },
			],
		},
		{
			title: 'Se déplacer sur le canevas',
			items: [
				{ keys: ['Glisser le fond'], label: 'Déplacer la vue (ou molette, ou Espace + glisser)' },
				{ keys: [mod, 'molette'], label: 'Zoomer autour du pointeur' },
				{ keys: ['Maj', '1'], label: 'Tout afficher' },
			],
		},
		{
			title: 'Modifier le workflow',
			items: [
				{ keys: ['Clic'], label: 'Sélectionner un nœud : ses actions apparaissent au-dessus' },
				{ keys: ['Double-clic'], label: 'Paramétrer un nœud (ou Entrée sur la sélection)' },
				{ keys: ['Tirer un port'], label: 'Relier deux nœuds ; lâché dans le vide : ajouter un nœud relié' },
				{ keys: ['Maj', 'glisser'], label: 'Sélectionner plusieurs nœuds' },
				{ keys: [mod, 'clic'], label: 'Ajouter à la sélection' },
				{ keys: [mod, 'A'], label: 'Tout sélectionner' },
				{ keys: ['Alt', 'glisser'], label: 'Déplacer sans aimantation à la grille' },
				{ keys: ['Retour arrière'], label: 'Supprimer la sélection (les voisins restent reliés)' },
				{ keys: ['Maj', 'Alt', 'T'], label: 'Ranger et aligner les nœuds' },
				{ keys: [mod, 'Z'], label: 'Annuler' },
				{ keys: [mod, 'Maj', 'Z'], label: 'Rétablir' },
			],
		},
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
	<Dialog.Content class="flex max-h-[calc(100dvh-2rem)] flex-col sm:max-w-lg">
		<Dialog.Header>
			<Dialog.Title>Gestes et raccourcis</Dialog.Title>
			<Dialog.Description>Comment manipuler le canevas et ses nœuds.</Dialog.Description>
		</Dialog.Header>
		<div class="-mx-6 min-h-0 space-y-4 overflow-y-auto px-6">
			{#each shortcuts as group (group.title)}
				<section>
					<h3 class="text-muted-foreground mb-1 text-xs font-semibold tracking-wide uppercase">{group.title}</h3>
					<ul class="divide-y">
						{#each group.items as s (s.label)}
							<li class="flex items-center justify-between gap-4 py-1.5 text-sm">
								<span>{s.label}</span>
								<span class="flex shrink-0 gap-1">
									{#each s.keys as k (k)}<kbd class="bg-muted rounded border px-1.5 py-0.5 font-mono text-xs whitespace-nowrap">{k}</kbd>{/each}
								</span>
							</li>
						{/each}
					</ul>
				</section>
			{/each}
		</div>
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
