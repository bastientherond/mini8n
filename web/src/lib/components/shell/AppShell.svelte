<script lang="ts">
	import { SvelteFlowProvider } from '@xyflow/svelte';
	import { toast } from 'svelte-sonner';
	import { untrack } from 'svelte';
	import { errorMessage } from '$lib/api';
	import { router } from '$lib/router.svelte';
	import { session } from '$lib/session.svelte';
	import { ui } from '$lib/ui.svelte';
	import { wf } from '$lib/workflow.svelte';
	import Editor from '../Editor.svelte';
	import AccountPage from '../pages/AccountPage.svelte';
	import ExecutionsPage from '../pages/ExecutionsPage.svelte';
	import HomePage from '../pages/HomePage.svelte';
	import MembersPage from '../pages/MembersPage.svelte';
	import SettingsPage from '../pages/SettingsPage.svelte';
	import WorkflowsPage from '../pages/WorkflowsPage.svelte';
	import CommandPalette from './CommandPalette.svelte';
	import GlobalDialogs from './GlobalDialogs.svelte';
	import Sidebar from './Sidebar.svelte';
	import TopBar from './TopBar.svelte';

	const route = $derived(router.route);
	const routeWs = $derived('ws' in route ? route.ws : null);

	// La route décide du workspace courant ; un workspace inconnu (ou supprimé) redirige.
	$effect(() => {
		const workspaces = session.workspaces;
		const target = routeWs;
		untrack(() => {
			if (target && workspaces.some((w) => w.id === target)) {
				session.select(target);
				wf.loadWorkspace(target).catch((e) => toast.error(errorMessage(e)));
				return;
			}
			if (route.name === 'account') return;
			const fallback = workspaces.find((w) => w.id === session.currentId) ?? workspaces[0];
			if (fallback) router.replace({ name: 'home', ws: fallback.id });
		});
	});

	// En quittant l'éditeur, on enregistre puis on ferme le workflow.
	$effect(() => {
		if (route.name !== 'editor') untrack(() => void wf.flush().then(() => wf.close()));
	});

	function onkeydown(event: KeyboardEvent) {
		if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
			event.preventDefault();
			ui.palette = !ui.palette;
		}
	}
</script>

<svelte:window {onkeydown} />

<div class="flex h-dvh flex-col">
	<TopBar />
	<div class="flex min-h-0 flex-1">
		<Sidebar />
		<main class="bg-background min-w-0 flex-1">
			{#if route.name === 'account'}
				<AccountPage />
			{:else if session.current && routeWs === session.currentId}
				{#if route.name === 'home'}
					<HomePage />
				{:else if route.name === 'workflows'}
					<WorkflowsPage />
				{:else if route.name === 'members'}
					<MembersPage />
				{:else if route.name === 'settings'}
					<SettingsPage />
				{:else if route.name === 'executions'}
					<ExecutionsPage />
				{:else if route.name === 'editor'}
					{#key route.ws}
						<SvelteFlowProvider>
							<Editor workspaceId={route.ws} workflowId={route.workflow} />
						</SvelteFlowProvider>
					{/key}
				{/if}
			{/if}
		</main>
	</div>
</div>

<CommandPalette />
<GlobalDialogs />
