<script lang="ts">
	import {
		Background,
		BackgroundVariant,
		Controls,
		MiniMap,
		SvelteFlow,
		useSvelteFlow,
		type Connection,
		type Edge,
		type NodeTypes,
	} from '@xyflow/svelte';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import { mode } from 'mode-watcher';
	import { onMount, untrack } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { errorMessage } from '$lib/api';
	import { router } from '$lib/router.svelte';
	import { wf } from '$lib/workflow.svelte';
	import DataPanel from './DataPanel.svelte';
	import EditorToolbar from './EditorToolbar.svelte';
	import Inspector from './Inspector.svelte';
	import Palette from './Palette.svelte';
	import StepNode from './StepNode.svelte';

	let { workspaceId, workflowId }: { workspaceId: string; workflowId: string } = $props();

	const nodeTypes: NodeTypes = { step: StepNode };
	const { screenToFlowPosition, fitView } = useSvelteFlow();

	let canvas: HTMLDivElement | undefined = $state();
	let panelHeight = $state(280);

	const fit = () => setTimeout(() => fitView({ padding: 0.2 }), 50);

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
		const rect = canvas?.getBoundingClientRect();
		const center = rect
			? screenToFlowPosition({ x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 })
			: { x: 0, y: 0 };
		const jitter = () => Math.round((Math.random() - 0.5) * 60);
		wf.addNode(kind, { x: center.x - 112 + jitter(), y: center.y - 40 + jitter() });
	}

	function ondragover(event: DragEvent) {
		event.preventDefault();
		if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
	}

	function ondrop(event: DragEvent) {
		event.preventDefault();
		const kind = event.dataTransfer?.getData('application/mini8n');
		if (!kind) return;
		const pos = screenToFlowPosition({ x: event.clientX, y: event.clientY });
		wf.addNode(kind, { x: pos.x - 112, y: pos.y - 30 });
	}

	function isValidConnection(c: Connection | Edge) {
		return c.source !== c.target;
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
		}
	}

	function startResize(event: PointerEvent) {
		const startY = event.clientY;
		const startH = panelHeight;
		const move = (e: PointerEvent) => {
			panelHeight = Math.min(window.innerHeight - 200, Math.max(120, startH + startY - e.clientY));
		};
		const up = () => {
			window.removeEventListener('pointermove', move);
			window.removeEventListener('pointerup', up);
		};
		window.addEventListener('pointermove', move);
		window.addEventListener('pointerup', up);
	}
</script>

<svelte:window {onkeydown} />

<div class="flex h-full min-h-0 flex-col">
	<EditorToolbar onrun={runAll} />
	<div class="flex min-h-0 flex-1">
		<Palette onadd={addAtCenter} />
		<main class="flex min-w-0 flex-1 flex-col">
			<div class="relative min-h-0 flex-1" bind:this={canvas} role="application" {ondragover} {ondrop}>
				<SvelteFlow
					bind:nodes={wf.nodes}
					bind:edges={wf.edges}
					{nodeTypes}
					{isValidConnection}
					colorMode={mode.current ?? 'light'}
					defaultEdgeOptions={{ animated: false }}
					fitView
					fitViewOptions={{ padding: 0.2 }}
					minZoom={0.2}
					snapGrid={[16, 16]}
					deleteKey={['Backspace', 'Delete']}
				>
					<Background variant={BackgroundVariant.Dots} gap={16} />
					<Controls showLock={false} />
					<MiniMap pannable zoomable class="hidden lg:block" />
				</SvelteFlow>
				{#if wf.loading}
					<div class="bg-background/60 absolute inset-0 z-10 flex items-center justify-center gap-2 text-sm">
						<LoaderCircle class="size-4 animate-spin" /> Chargement du workflow…
					</div>
				{/if}
			</div>
			<div
				class="hover:bg-primary/20 h-1.5 shrink-0 cursor-row-resize border-t transition-colors"
				role="separator"
				aria-orientation="horizontal"
				aria-label="Redimensionner le panneau de données"
				onpointerdown={startResize}
			></div>
			<div class="shrink-0" style="height: {panelHeight}px">
				<DataPanel />
			</div>
		</main>
		<Inspector />
	</div>
</div>
