<script lang="ts">
	// Vue détaillée d'un nœud (comme la « Node Details View » de n8n) : modale plein écran
	// avec l'entrée à gauche, les paramètres au centre et la sortie à droite.
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import Copy from '@lucide/svelte/icons/copy';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import Play from '@lucide/svelte/icons/play';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Label } from '$lib/components/ui/label';
	import { Separator } from '$lib/components/ui/separator';
	import { Switch } from '$lib/components/ui/switch';
	import { cn } from '$lib/utils';
	import { categoryAccent, categoryLabels, formatDateTime, iconFor } from '$lib/icons';
	import { wf } from '$lib/workflow.svelte';
	import DataPanel from './DataPanel.svelte';
	import ParamField from './ParamField.svelte';

	let { onrun }: { onrun: (target: string) => void } = $props();

	const node = $derived(wf.openNode);
	const spec = $derived(node ? wf.spec(node.data.kind) : undefined);
	const Icon = $derived(iconFor(spec?.icon));
	const columns = $derived(node ? wf.inputColumns(node.id) : []);
	const automatic = $derived(spec?.category === 'trigger' && node?.data.kind !== 'trigger_manual');
	const info = $derived(node ? wf.trigger(node.id) : undefined);

	/** Nœuds reliés à chaque entrée (A, B…). */
	const sources = $derived.by(() => {
		if (!node || !spec) return [];
		return spec.inputs.flatMap((label, i) =>
			wf.edges
				.filter((e) => e.target === node.id && (Number(e.targetHandle?.replace('in-', '') ?? 0) || 0) === i)
				.map((e) => ({ label, id: e.source, name: wf.nodes.find((n) => n.id === e.source)?.data.name ?? e.source })),
		);
	});
	let inputTab = $state(0);
	$effect(() => {
		void node?.id;
		inputTab = 0;
	});
	const source = $derived(sources[Math.min(inputTab, Math.max(sources.length - 1, 0))]);

	const curl = $derived.by(() => {
		if (node?.data.kind !== 'trigger_webhook') return '';
		const p = node.data.params;
		const url = `${location.origin}/api/hooks/${encodeURIComponent(String(p.token ?? ''))}`;
		const method = p.method === 'toutes' || !p.method ? 'POST' : String(p.method);
		const secret = p.secret ? ` \\\n  -H 'X-Mini8n-Secret: ${p.secret}'` : '';
		if (method === 'GET') return `curl '${url}?exemple=1'${secret}`;
		return `curl -X ${method} '${url}' \\\n  -H 'Content-Type: application/json'${secret} \\\n  -d '{"exemple": 1}'`;
	});

	function close() {
		wf.openNodeId = null;
	}
</script>

<Dialog.Root open={!!node} onOpenChange={(open) => !open && close()}>
	<Dialog.Content
		showCloseButton={false}
		class="flex h-[calc(100dvh-2rem)] w-[calc(100vw-2rem)] max-w-none flex-col gap-0 overflow-hidden p-0 sm:max-w-none"
	>
		{#if node && spec}
			<!-- En-tête -->
			<div class="flex flex-wrap items-center gap-3 border-b px-4 py-3">
				<Button variant="ghost" size="sm" onclick={close}><ArrowLeft /> Canevas</Button>
				<Separator orientation="vertical" class="h-6" />
				<span class={cn('flex size-9 shrink-0 items-center justify-center rounded-lg', categoryAccent[spec.category])}>
					<Icon class="size-[18px]" />
				</span>
				<div class="min-w-0 flex-1">
					<Dialog.Title class="sr-only">Paramètres de {node.data.name}</Dialog.Title>
					<input
						value={node.data.name}
						oninput={(e) => wf.updateData(node.id, { name: e.currentTarget.value })}
						class="hover:border-input focus-visible:border-ring w-full max-w-md rounded-md border border-transparent bg-transparent px-1.5 py-0.5 text-base font-semibold outline-none"
						aria-label="Nom du nœud"
						maxlength={120}
					/>
					<Dialog.Description class="text-muted-foreground truncate px-1.5 text-xs">
						{spec.label} · {spec.description}
					</Dialog.Description>
				</div>
				<Badge variant="outline">{categoryLabels[spec.category]}</Badge>
				<Button size="sm" disabled={wf.running} onclick={() => onrun(node.id)}>
					{#if wf.running}<LoaderCircle class="animate-spin" />{:else}<Play />{/if}
					Exécuter le nœud
				</Button>
				<Button size="icon-sm" variant="outline" onclick={() => wf.duplicateNode(node.id)} title="Dupliquer" aria-label="Dupliquer">
					<Copy />
				</Button>
				<Button
					size="icon-sm"
					variant="outline"
					onclick={() => {
						const id = node.id;
						close();
						wf.removeNode(id);
					}}
					title="Supprimer"
					aria-label="Supprimer le nœud"
				>
					<Trash2 />
				</Button>
			</div>

			<!-- Entrée | Paramètres | Sortie -->
			<div
				class={cn(
					'grid min-h-0 flex-1 grid-cols-1 overflow-y-auto lg:overflow-hidden',
					spec.inputs.length > 0
						? 'lg:grid-cols-[minmax(0,1fr)_minmax(380px,460px)_minmax(0,1fr)]'
						: 'lg:grid-cols-[minmax(380px,520px)_minmax(0,1fr)]',
				)}
			>
				{#if spec.inputs.length > 0}
					<section class="flex min-h-80 min-w-0 flex-col lg:min-h-0" aria-label="Entrée">
						<div class="flex items-center gap-2 border-b px-4 py-2">
							<h3 class="text-muted-foreground text-xs font-semibold tracking-wide uppercase">Entrée</h3>
							{#if sources.length > 1}
								<div class="ml-auto flex gap-1">
									{#each sources as s, i (s.id + i)}
										<button
											class={cn('rounded-md px-2 py-0.5 text-xs', i === inputTab ? 'bg-accent font-medium' : 'text-muted-foreground hover:bg-accent')}
											onclick={() => (inputTab = i)}>{s.label} · {s.name}</button
										>
									{/each}
								</div>
							{/if}
						</div>
						{#if source}
							<div class="min-h-0 flex-1">
								<DataPanel
									nodeId={source.id}
									label={sources.length > 1 ? `Entrée ${source.label}` : 'Entrée'}
									fieldBase={inputTab === 0 ? '$json' : `$(${JSON.stringify(source.name)}).item.json`}
								>
									{#snippet emptyAction()}
										<Button size="sm" variant="outline" disabled={wf.running} onclick={() => onrun(source.id)}>
											<Play /> Exécuter les nœuds précédents
										</Button>
									{/snippet}
								</DataPanel>
							</div>
						{:else}
							<div class="text-muted-foreground flex flex-1 items-center justify-center p-6 text-center text-sm">
								Aucune entrée connectée : reliez la sortie d'un autre nœud à ce nœud sur le canevas.
							</div>
						{/if}
					</section>
				{/if}

				<section class="bg-sidebar flex min-w-0 flex-col border-y lg:min-h-0 lg:border-x lg:border-y-0" aria-label="Paramètres">
					<div class="border-b px-4 py-2">
						<h3 class="text-muted-foreground text-xs font-semibold tracking-wide uppercase">Paramètres</h3>
					</div>
					<div class="min-h-0 flex-1 space-y-5 overflow-y-auto p-4">
						{#if automatic}
							<div class={cn('rounded-lg border p-3 text-xs', wf.active ? 'border-emerald-500/30 bg-emerald-500/5' : 'bg-muted/50')}>
								{#if !wf.active}
									<p>Le workflow est <strong>inactif</strong> : activez-le (interrupteur « Actif ») pour mettre ce déclencheur en service.</p>
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
									<p class="text-muted-foreground mt-1">Appelé par le nœud « Exécuter un workflow » d'un autre workflow (pas besoin d'activation).</p>
								{/if}
							</div>
						{/if}
						{#each spec.params as param (param.name)}
							<ParamField
								{param}
								value={node.data.params[param.name]}
								{columns}
								nodeId={node.id}
								nodeKind={node.data.kind}
								onchange={(v) => wf.setParam(node.id, param.name, v)}
							/>
						{:else}
							<p class="text-muted-foreground text-sm">Ce nœud n'a pas de paramètre.</p>
						{/each}
						{#if curl}
							<div class="space-y-1.5">
								<p class="text-sm font-medium">Tester le webhook</p>
								<pre class="bg-muted overflow-x-auto rounded-md p-2 font-mono text-[11px] leading-relaxed">{curl}</pre>
							</div>
						{/if}
						<Separator />
						<div class="flex items-center justify-between gap-2">
							<div>
								<Label for="node-disabled">Désactivé</Label>
								<p class="text-muted-foreground text-xs">
									{spec.inputs.length > 0 ? 'Transmet son entrée sans la modifier.' : 'Le nœud et sa branche sont ignorés.'}
								</p>
							</div>
							<Switch id="node-disabled" checked={!!node.data.disabled} onCheckedChange={(v) => wf.updateData(node.id, { disabled: v })} />
						</div>
					</div>
				</section>

				<section class="flex min-h-80 min-w-0 flex-col lg:min-h-0" aria-label="Sortie">
					<div class="border-b px-4 py-2">
						<h3 class="text-muted-foreground text-xs font-semibold tracking-wide uppercase">Sortie</h3>
					</div>
					<div class="min-h-0 flex-1">
						<DataPanel nodeId={node.id}>
							{#snippet emptyAction()}
								<Button size="sm" disabled={wf.running} onclick={() => onrun(node.id)}><Play /> Exécuter le nœud</Button>
							{/snippet}
						</DataPanel>
					</div>
				</section>
			</div>
		{/if}
	</Dialog.Content>
</Dialog.Root>
