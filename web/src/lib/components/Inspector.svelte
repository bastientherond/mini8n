<script lang="ts">
	import Copy from '@lucide/svelte/icons/copy';
	import MousePointerClick from '@lucide/svelte/icons/mouse-pointer-click';
	import Play from '@lucide/svelte/icons/play';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import X from '@lucide/svelte/icons/x';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { ScrollArea } from '$lib/components/ui/scroll-area';
	import { Separator } from '$lib/components/ui/separator';
	import { Switch } from '$lib/components/ui/switch';
	import { cn } from '$lib/utils';
	import { categoryAccent, iconFor } from '$lib/icons';
	import { wf } from '$lib/workflow.svelte';
	import ParamField from './ParamField.svelte';

	const node = $derived(wf.selected);
	const spec = $derived(node ? wf.spec(node.data.kind) : undefined);
	const columns = $derived(node ? wf.inputColumns(node.id) : []);
	const Icon = $derived(iconFor(spec?.icon));
</script>

<aside class="bg-sidebar flex h-full w-80 shrink-0 flex-col border-l">
	{#if node && spec}
		<div class="flex items-start gap-3 p-4 pb-3">
			<div class={cn('flex size-9 shrink-0 items-center justify-center rounded-lg', categoryAccent[spec.category])}>
				<Icon class="size-[18px]" />
			</div>
			<div class="min-w-0 flex-1">
				<Input
					value={node.data.name}
					oninput={(e) => wf.updateData(node.id, { name: e.currentTarget.value })}
					class="h-7 border-transparent px-1.5 text-sm font-medium shadow-none hover:border-input"
					aria-label="Nom du nœud"
				/>
				<p class="text-muted-foreground mt-1 px-1.5 text-xs">{spec.description}</p>
			</div>
			<Button variant="ghost" size="icon-sm" onclick={() => wf.select(null)} aria-label="Fermer">
				<X />
			</Button>
		</div>
		<div class="flex gap-2 px-4 pb-3">
			<Button size="sm" class="flex-1" disabled={wf.running} onclick={() => wf.run(node.id)}>
				<Play /> Exécuter jusqu'ici
			</Button>
			<Button size="icon-sm" variant="outline" onclick={() => wf.duplicateNode(node.id)} title="Dupliquer">
				<Copy />
			</Button>
			<Button size="icon-sm" variant="outline" onclick={() => wf.removeNode(node.id)} title="Supprimer">
				<Trash2 />
			</Button>
		</div>
		<Separator />
		<ScrollArea class="min-h-0 flex-1">
			<div class="space-y-5 p-4">
				{#each spec.params as param (param.name)}
					<ParamField
						{param}
						value={node.data.params[param.name]}
						{columns}
						onchange={(v) => wf.setParam(node.id, param.name, v)}
					/>
				{/each}
				{#if spec.params.length > 0}<Separator />{/if}
				<div class="flex items-center justify-between gap-2">
					<div>
						<Label for="node-disabled">Désactivé</Label>
						<p class="text-muted-foreground text-xs">Transmet son entrée sans la modifier.</p>
					</div>
					<Switch
						id="node-disabled"
						checked={!!node.data.disabled}
						onCheckedChange={(v) => wf.updateData(node.id, { disabled: v })}
					/>
				</div>
			</div>
		</ScrollArea>
	{:else}
		<div class="text-muted-foreground flex flex-1 flex-col items-center justify-center gap-3 p-8 text-center text-sm">
			<MousePointerClick class="size-8 opacity-60" />
			<p>Sélectionne un nœud pour modifier ses paramètres.</p>
			<ul class="space-y-1 text-xs">
				<li>Glisse un nœud depuis la palette</li>
				<li>Relie une sortie (à droite) à une entrée (à gauche)</li>
				<li><kbd class="bg-muted rounded px-1">⌘/Ctrl</kbd> + <kbd class="bg-muted rounded px-1">Entrée</kbd> pour exécuter</li>
				<li><kbd class="bg-muted rounded px-1">Retour arrière</kbd> supprime la sélection</li>
			</ul>
		</div>
	{/if}
</aside>
