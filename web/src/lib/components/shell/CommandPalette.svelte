<script lang="ts">
	import type { Component } from 'svelte';
	import Building2 from '@lucide/svelte/icons/building-2';
	import FilePlus from '@lucide/svelte/icons/file-plus';
	import FileText from '@lucide/svelte/icons/file-text';
	import History from '@lucide/svelte/icons/history';
	import House from '@lucide/svelte/icons/house';
	import Keyboard from '@lucide/svelte/icons/keyboard';
	import ListTree from '@lucide/svelte/icons/list-tree';
	import LogOut from '@lucide/svelte/icons/log-out';
	import Moon from '@lucide/svelte/icons/moon';
	import Play from '@lucide/svelte/icons/play';
	import Plus from '@lucide/svelte/icons/plus';
	import Search from '@lucide/svelte/icons/search';
	import Settings from '@lucide/svelte/icons/settings';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import Upload from '@lucide/svelte/icons/upload';
	import UserRound from '@lucide/svelte/icons/user-round';
	import Users from '@lucide/svelte/icons/users';
	import { Command } from 'bits-ui';
	import { toggleMode } from 'mode-watcher';
	import * as Dialog from '$lib/components/ui/dialog';
	import { router } from '$lib/router.svelte';
	import { session } from '$lib/session.svelte';
	import { createAndOpenWorkflow, logout, pickWorkflowFile, ui } from '$lib/ui.svelte';
	import { wf } from '$lib/workflow.svelte';

	type Entry = { id: string; label: string; icon: Component; keywords?: string[]; hint?: string; run: () => void };

	const ws = $derived(session.currentId);

	const groups = $derived.by((): { heading: string; items: Entry[] }[] => {
		const out: { heading: string; items: Entry[] }[] = [];
		if (ws) {
			out.push({
				heading: 'Navigation',
				items: [
					{ id: 'nav-home', label: 'Accueil', icon: House, run: () => router.go({ name: 'home', ws }) },
					{ id: 'nav-workflows', label: 'Tous les workflows', icon: ListTree, run: () => router.go({ name: 'workflows', ws }) },
					{ id: 'nav-executions', label: 'Exécutions', icon: History, keywords: ['historique', 'logs', 'déclencheurs'], run: () => router.go({ name: 'executions', ws }) },
					{ id: 'nav-members', label: 'Membres', icon: Users, keywords: ['équipe', 'inviter'], run: () => router.go({ name: 'members', ws }) },
					{ id: 'nav-settings', label: 'Paramètres du workspace', icon: Settings, run: () => router.go({ name: 'settings', ws }) },
					{ id: 'nav-account', label: 'Mon compte', icon: UserRound, keywords: ['mot de passe', 'profil'], run: () => router.go({ name: 'account' }) },
				],
			});
			if (wf.workflows.length > 0) {
				out.push({
					heading: 'Workflows',
					items: wf.workflows.map((w) => ({
						id: `wf-${w.id}`,
						label: w.id === wf.workflowId ? wf.name || w.name : w.name,
						icon: FileText,
						keywords: ['ouvrir', 'workflow'],
						run: () => router.go({ name: 'editor', ws, workflow: w.id }),
					})),
				});
			}
		}
		if (session.workspaces.length > 1) {
			out.push({
				heading: 'Workspaces',
				items: session.workspaces
					.filter((w) => w.id !== ws)
					.map((w) => ({
						id: `ws-${w.id}`,
						label: w.name,
						icon: Building2,
						keywords: ['workspace', 'changer'],
						run: () => router.go({ name: 'home', ws: w.id }),
					})),
			});
		}
		const actions: Entry[] = [];
		if (ws) {
			if (router.route.name === 'editor') {
				actions.push({ id: 'act-run', label: 'Exécuter le workflow', icon: Play, hint: '⌘↵', run: () => void wf.run() });
			}
			actions.push(
				{ id: 'act-new', label: 'Nouveau workflow', icon: FilePlus, keywords: ['créer'], run: () => (ui.newWorkflow = true) },
				{ id: 'act-example', label: "Nouveau workflow depuis l'exemple", icon: Sparkles, run: () => createAndOpenWorkflow('Exemple : ventes', true) },
				{ id: 'act-import', label: 'Importer un workflow (.json)', icon: Upload, run: pickWorkflowFile },
			);
		}
		actions.push(
			{ id: 'act-ws', label: 'Nouveau workspace', icon: Plus, keywords: ['créer'], run: () => (ui.newWorkspace = true) },
			{ id: 'act-theme', label: 'Basculer le thème clair / sombre', icon: Moon, keywords: ['dark', 'mode'], run: toggleMode },
			{ id: 'act-keys', label: 'Raccourcis clavier', icon: Keyboard, run: () => (ui.shortcuts = true) },
			{ id: 'act-logout', label: 'Se déconnecter', icon: LogOut, run: () => logout() },
		);
		out.push({ heading: 'Actions', items: actions });
		return out;
	});

	function select(entry: Entry) {
		ui.palette = false;
		// Laisse le dialogue se fermer avant de naviguer ou d'en ouvrir un autre.
		setTimeout(entry.run, 0);
	}
</script>

<Dialog.Root bind:open={ui.palette}>
	<Dialog.Content class="overflow-hidden p-0 sm:max-w-xl" showCloseButton={false}>
		<Dialog.Title class="sr-only">Palette de commandes</Dialog.Title>
		<Dialog.Description class="sr-only">Rechercher une page, un workflow ou une action</Dialog.Description>
		<Command.Root class="flex max-h-[70vh] flex-col" loop>
			<div class="flex items-center gap-2 border-b px-3">
				<Search class="text-muted-foreground size-4 shrink-0" />
				<Command.Input
					class="placeholder:text-muted-foreground h-11 w-full bg-transparent text-sm outline-none"
					placeholder="Rechercher une page, un workflow, une action…"
				/>
			</div>
			<Command.List class="overflow-y-auto p-1.5">
				<Command.Viewport>
					<Command.Empty class="text-muted-foreground py-6 text-center text-sm">Aucun résultat.</Command.Empty>
					{#each groups as group (group.heading)}
						<Command.Group value={group.heading}>
							<Command.GroupHeading class="text-muted-foreground px-2 pt-2 pb-1 text-xs font-medium">
								{group.heading}
							</Command.GroupHeading>
							<Command.GroupItems>
								{#each group.items as entry (entry.id)}
									{@const Icon = entry.icon}
									<Command.Item
										value={`${entry.label} ${entry.id}`}
										keywords={entry.keywords}
										onSelect={() => select(entry)}
										class="data-selected:bg-accent data-selected:text-accent-foreground flex h-9 cursor-pointer items-center gap-2.5 rounded-md px-2 text-sm outline-none"
									>
										<Icon class="text-muted-foreground size-4" />
										<span class="flex-1 truncate">{entry.label}</span>
										{#if entry.hint}<kbd class="text-muted-foreground text-xs">{entry.hint}</kbd>{/if}
									</Command.Item>
								{/each}
							</Command.GroupItems>
						</Command.Group>
					{/each}
				</Command.Viewport>
			</Command.List>
		</Command.Root>
	</Dialog.Content>
</Dialog.Root>
