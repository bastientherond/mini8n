<script lang="ts">
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import Plus from '@lucide/svelte/icons/plus';
	import Zap from '@lucide/svelte/icons/zap';
	import { categoryAccent, iconFor } from '$lib/icons';
	import { cn } from '$lib/utils';
	import { onMount, untrack } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { errorMessage } from '$lib/api';
	import { live } from '$lib/live.svelte';
	import { router } from '$lib/router.svelte';
	import { wf } from '$lib/workflow.svelte';
	import EditorToolbar from './EditorToolbar.svelte';
	import NodeDetails from './NodeDetails.svelte';
	import Palette from './Palette.svelte';
	import Canvas from './canvas/Canvas.svelte';
	import { Viewport } from '$lib/canvas/viewport.svelte';

	let { workspaceId, workflowId }: { workspaceId: string; workflowId: string } = $props();

	const viewport = new Viewport();
	let canvas: Canvas | undefined = $state();

	const PALETTE_KEY = 'mini8n.paletteOpen';
	let paletteOpen = $state(readPaletteOpen());

	function readPaletteOpen() {
		try {
			return localStorage.getItem(PALETTE_KEY) !== '0';
		} catch {
			return true;
		}
	}

	function setPalette(open: boolean) {
		paletteOpen = open;
		try {
			localStorage.setItem(PALETTE_KEY, open ? '1' : '0');
		} catch {
			// confort uniquement
		}
	}

	/** Raccourcis pour démarrer un workflow vide. */
	const starters = ['trigger_manual', 'trigger_webhook', 'trigger_schedule', 'trigger_form'];

	const fit = () => setTimeout(() => canvas?.fit(), 50);

	// Ouvre le workflow désigné par l'URL.
	$effect(() => {
		const id = workflowId;
		untrack(() => wf.openWorkflow(id, workspaceId))
			.then(fit)
			.catch((e) => {
				toast.error(`Ouverture impossible : ${errorMessage(e)}`);
				router.replace({ name: 'workflows', ws: workspaceId });
			});
	});

	// Collaboration en direct sur le workflow ouvert.
	$effect(() => {
		const [ws, id] = [wf.workspaceId, wf.workflowId];
		if (!ws || !id) return;
		untrack(() =>
			live.connect(ws, id, {
				onsaved: (doc) => wf.receive(doc),
				onresync: () => void wf.resync(),
				onactive: () => void wf.refreshTriggers(),
				onrun: (peer, run) => wf.showRemoteRun(peer, run),
				ongone: (reason) => {
					if (reason === 'full') {
						toast.info('Trop de participants sur ce workflow : la présence des autres n’est pas affichée.');
						return;
					}
					toast.error(reason === 'deleted' ? 'Ce workflow a été supprimé par un autre membre.' : 'Vous n’avez plus accès à ce workflow.');
					wf.close();
					router.replace({ name: 'workflows', ws });
				},
			}),
		);
		return () => live.disconnect();
	});

	// Ce que les autres voient de moi : sélection et nœud en cours de paramétrage.
	$effect(() => live.select(wf.nodes.filter((n) => n.selected).map((n) => n.id)));
	$effect(() => live.focus(wf.openNodeId));

	onMount(() => {
		// Sauvegarde des modifications en attente quand l'onglet est masqué ou fermé.
		const onHide = () => {
			if (document.visibilityState === 'hidden') void wf.flush(true);
		};
		document.addEventListener('visibilitychange', onHide);
		return () => document.removeEventListener('visibilitychange', onHide);
	});

	// Sauvegarde automatique sur le serveur (debounce dans le store).
	$effect(() => {
		void wf.nodes;
		void wf.edges;
		void wf.name;
		wf.scheduleSave();
	});

	function addAtCenter(kind: string) {
		const center = canvas?.center() ?? { x: 0, y: 0 };
		const jitter = () => Math.round((Math.random() - 0.5) * 60);
		const id = wf.addNode(kind, { x: center.x - 112 + jitter(), y: center.y - 40 + jitter() });
		if (id) wf.openNodeId = id;
	}

	function ondragover(event: DragEvent) {
		event.preventDefault();
		if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
	}

	function ondrop(event: DragEvent) {
		event.preventDefault();
		const kind = event.dataTransfer?.getData('application/mini8n');
		if (!kind) return;
		const pos = canvas?.toCanvas(event.clientX, event.clientY) ?? { x: 0, y: 0 };
		const id = wf.addNode(kind, { x: pos.x - 112, y: pos.y - 30 });
		if (id) wf.openNodeId = id;
	}

	export async function runAll() {
		if (wf.nodes.length === 0) {
			toast.info('Ajoute au moins un nœud avant d’exécuter.');
			return;
		}
		try {
			const report = await wf.run();
			if (!report) return;
			if (report.error) {
				toast.error(report.error);
				return;
			}
			const results = Object.values(report.results);
			const errors = results.filter((r) => r.status === 'error').length;
			if (errors > 0) toast.error(`${errors} nœud${errors > 1 ? 's' : ''} en erreur`);
			else toast.success(`${results.length} nœuds exécutés en ${report.durationMs.toFixed(0)} ms`);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : String(e));
		}
	}

	function onkeydown(event: KeyboardEvent) {
		if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
			event.preventDefault();
			runAll();
			return;
		}
		// Entrée sur un nœud sélectionné : ouvre son paramétrage.
		const target = event.target as HTMLElement | null;
		if (event.key === 'Enter' && !wf.openNodeId && wf.selected && !target?.closest('input, textarea, button, [role="dialog"]')) {
			event.preventDefault();
			wf.openNodeId = wf.selected.id;
		}
	}

	/** Exécution jusqu'à un nœud (depuis la vue détaillée). */
	async function runTo(target: string) {
		try {
			const report = await wf.run(target);
			if (report?.error) toast.error(report.error);
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

</script>

<svelte:window {onkeydown} />

<NodeDetails onrun={runTo} />

<div class="flex h-full min-h-0 flex-col">
	<EditorToolbar onrun={runAll} />
	<div class="flex min-h-0 flex-1">
		{#if paletteOpen}
			<Palette onadd={addAtCenter} onclose={() => setPalette(false)} />
		{/if}
		<main class="flex min-w-0 flex-1 flex-col">
			<div class="relative min-h-0 flex-1" role="presentation" {ondragover} {ondrop}>
				<Canvas bind:this={canvas} {viewport} />
				{#if !paletteOpen}
					<button
						class="bg-primary text-primary-foreground hover:bg-primary/90 absolute top-3 left-3 z-10 flex h-8 items-center gap-1.5 rounded-lg px-3 text-sm font-medium shadow-md shadow-primary/20"
						onclick={() => setPalette(true)}
					>
						<Plus class="size-4" /> Ajouter un nœud
					</button>
				{/if}
				{#if !wf.loading && wf.workflowId && wf.nodes.length === 0}
					<!-- Canevas vide : par où commencer. -->
					<div class="pointer-events-none absolute inset-0 z-10 flex items-center justify-center p-6">
						<div class="bg-card pointer-events-auto w-full max-w-md rounded-2xl border p-6 shadow-xl shadow-black/5">
							<span class="bg-brand mb-3 flex size-10 items-center justify-center rounded-xl text-white shadow-md shadow-primary/30">
								<Zap class="size-5" />
							</span>
							<p class="font-semibold">Commencez par un déclencheur</p>
							<p class="text-muted-foreground mt-1 text-sm">Il définit quand le workflow démarre. Ajoutez ensuite des nœuds à sa suite.</p>
							<div class="mt-4 grid grid-cols-2 gap-2">
								{#each starters as kind (kind)}
									{@const spec = wf.spec(kind)}
									{#if spec}
										{@const Icon = iconFor(spec.icon)}
										<button
											class="hover:border-primary/40 hover:bg-accent flex items-center gap-2 rounded-lg border px-3 py-2 text-left text-sm transition-colors"
											onclick={() => addAtCenter(kind)}
										>
											<span class={cn('flex size-7 shrink-0 items-center justify-center rounded-md', categoryAccent.trigger)}><Icon class="size-4" /></span>
											{spec.label}
										</button>
									{/if}
								{/each}
							</div>
						</div>
					</div>
				{/if}
				{#if wf.loading}
					<div class="bg-background/60 absolute inset-0 z-10 flex items-center justify-center gap-2 text-sm">
						<LoaderCircle class="size-4 animate-spin" /> Chargement du workflow…
					</div>
				{/if}
			</div>
		</main>
	</div>
</div>
