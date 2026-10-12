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
	import { categoryAccent, formatDateTime, iconFor } from '$lib/icons';
	import { wf } from '$lib/workflow.svelte';
	import ParamField from './ParamField.svelte';

	const node = $derived(wf.selected);
	const spec = $derived(node ? wf.spec(node.data.kind) : undefined);
	const columns = $derived(node ? wf.inputColumns(node.id) : []);
	const Icon = $derived(iconFor(spec?.icon));
	const isTrigger = $derived(spec?.category === 'trigger');
	const automatic = $derived(isTrigger && node?.data.kind !== 'trigger_manual');
	const info = $derived(node ? wf.trigger(node.id) : undefined);
	const curl = $derived.by(() => {
		if (node?.data.kind !== 'trigger_webhook') return '';
		const p = node.data.params;
		const url = `${location.origin}/api/hooks/${encodeURIComponent(String(p.token ?? ''))}`;
		const method = p.method === 'toutes' || !p.method ? 'POST' : String(p.method);
		const secret = p.secret ? ` \\\n  -H 'X-Mini8n-Secret: ${p.secret}'` : '';
		if (method === 'GET') return `curl '${url}?exemple=1'${secret}`;
		return `curl -X ${method} '${url}' \\\n  -H 'Content-Type: application/json'${secret} \\\n  -d '{"exemple": 1}'`;
	});
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
				{#if automatic}
					<div
						class={cn(
							'rounded-lg border p-3 text-xs',
							wf.active ? 'border-emerald-500/30 bg-emerald-500/5' : 'bg-muted/50',
						)}
					>
						{#if !wf.active}
							<p>Le workflow est <strong>inactif</strong> : activez-le (interrupteur « Actif » en haut) pour mettre ce déclencheur en service.</p>
						{:else if info}
							<p class="font-medium text-emerald-700 dark:text-emerald-400">Déclencheur en service</p>
							{#if info.nextRunAt}<p class="text-muted-foreground mt-1">Prochaine exécution : {formatDateTime(info.nextRunAt)}</p>{/if}
							{#if info.lastError}<p class="text-destructive mt-1">Dernière erreur : {info.lastError}</p>{/if}
						{:else}
							<p>Workflow actif — enregistrement en cours…</p>
						{/if}
						{#if node.data.kind === 'trigger_error'}
							<p class="text-muted-foreground mt-1">Se déclenche quand une exécution automatique d'un autre workflow du workspace échoue.</p>
						{:else if node.data.kind === 'trigger_workflow'}
							<p class="text-muted-foreground mt-1">Utilisez le nœud « Exécuter un workflow » dans un autre workflow pour appeler celui-ci (pas besoin d'activation).</p>
						{/if}
					</div>
				{/if}
				{#each spec.params as param (param.name)}
					<ParamField
						{param}
						value={node.data.params[param.name]}
						{columns}
						nodeKind={node.data.kind}
						onchange={(v) => wf.setParam(node.id, param.name, v)}
					/>
				{/each}
				{#if curl}
					<div class="space-y-1.5">
						<p class="text-sm font-medium">Tester le webhook</p>
						<pre class="bg-muted overflow-x-auto rounded-md p-2 font-mono text-[11px] leading-relaxed">{curl}</pre>
					</div>
				{/if}
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
