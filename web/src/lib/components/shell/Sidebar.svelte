<script lang="ts">
	import type { Component } from 'svelte';
	import FolderOpen from '@lucide/svelte/icons/folder-open';
	import History from '@lucide/svelte/icons/history';
	import KeyRound from '@lucide/svelte/icons/key-round';
	import House from '@lucide/svelte/icons/house';
	import ListTree from '@lucide/svelte/icons/list-tree';
	import PanelLeftClose from '@lucide/svelte/icons/panel-left-close';
	import PanelLeftOpen from '@lucide/svelte/icons/panel-left-open';
	import Settings from '@lucide/svelte/icons/settings';
	import SquarePen from '@lucide/svelte/icons/square-pen';
	import UserRound from '@lucide/svelte/icons/user-round';
	import Users from '@lucide/svelte/icons/users';
	import { cn } from '$lib/utils';
	import { href, router, type Route } from '$lib/router.svelte';
	import { session } from '$lib/session.svelte';
	import { wf } from '$lib/workflow.svelte';

	const PIN_KEY = 'mini8n.sidebarPinned';

	let pinned = $state(readPinned());
	let hovered = $state(false);
	const expanded = $derived(pinned || hovered);

	function readPinned() {
		try {
			return localStorage.getItem(PIN_KEY) === '1';
		} catch {
			return false;
		}
	}

	function togglePin() {
		pinned = !pinned;
		hovered = false;
		try {
			localStorage.setItem(PIN_KEY, pinned ? '1' : '0');
		} catch {
			// confort uniquement
		}
	}

	type Item = { label: string; icon: Component; route: Route | null; active: boolean };

	const ws = $derived(session.currentId);
	const name = $derived(router.route.name);

	const editorTarget = $derived.by((): Route | null => {
		if (!ws) return null;
		if (router.route.name === 'editor') return router.route;
		const id = wf.workflowId ?? wf.lastWorkflowId();
		return id ? { name: 'editor', ws, workflow: id } : null;
	});

	const groups = $derived.by((): Item[][] => {
		if (!ws) return [];
		return [
			[
				{ label: 'Accueil', icon: House, route: { name: 'home', ws }, active: name === 'home' },
				{ label: 'Éditeur', icon: SquarePen, route: editorTarget, active: name === 'editor' },
				{ label: 'Workflows', icon: ListTree, route: { name: 'workflows', ws }, active: name === 'workflows' },
				{ label: 'Exécutions', icon: History, route: { name: 'executions', ws }, active: name === 'executions' },
			],
			[
				{ label: 'Identifiants', icon: KeyRound, route: { name: 'credentials', ws }, active: name === 'credentials' },
				{ label: 'Fichiers', icon: FolderOpen, route: { name: 'files', ws }, active: name === 'files' },
			],
			[{ label: 'Membres', icon: Users, route: { name: 'members', ws }, active: name === 'members' }],
			[
				{ label: 'Paramètres', icon: Settings, route: { name: 'settings', ws }, active: name === 'settings' },
				{ label: 'Mon compte', icon: UserRound, route: { name: 'account' }, active: name === 'account' },
			],
		];
	});
</script>

<!-- La largeur réservée reste fixe : en mode survol, le panneau se superpose au contenu. -->
<div class={cn('relative shrink-0 transition-[width] duration-200', pinned ? 'w-52' : 'w-14')}>
	<nav
		class={cn(
			'bg-sidebar absolute inset-y-0 left-0 z-30 flex flex-col border-r transition-[width,box-shadow] duration-200',
			expanded ? 'w-52' : 'w-14',
			hovered && !pinned && 'shadow-xl',
		)}
		aria-label="Navigation principale"
		onmouseenter={() => (hovered = true)}
		onmouseleave={() => (hovered = false)}
	>
		<div class="flex flex-1 flex-col gap-1 overflow-hidden py-2">
			{#each groups as group, gi (gi)}
				{#if gi > 0}<div class="bg-border mx-3 my-1.5 h-px"></div>{/if}
				{#each group as item (item.label)}
					{@const Icon = item.icon}
					{#if item.route}
						<a
							href={href(item.route)}
							class={cn(
								'relative mx-2 flex h-9 items-center gap-3 rounded-lg px-2.5 text-sm whitespace-nowrap transition-colors',
								item.active
									? 'bg-sidebar-accent text-sidebar-accent-foreground font-medium'
									: 'text-muted-foreground hover:bg-sidebar-accent/60 hover:text-foreground',
							)}
							aria-current={item.active ? 'page' : undefined}
							title={expanded ? undefined : item.label}
						>
							{#if item.active}<span class="bg-primary absolute top-2 bottom-2 -left-2 w-[3px] rounded-r-full"></span>{/if}
							<Icon class={cn('size-[18px] shrink-0', item.active && 'text-primary')} />
							<span class={cn('transition-opacity', expanded ? 'opacity-100' : 'opacity-0')}>{item.label}</span>
						</a>
					{:else}
						<span
							class="text-muted-foreground/50 mx-2 flex h-9 cursor-not-allowed items-center gap-3 rounded-md px-2.5 text-sm whitespace-nowrap"
							title="Aucun workflow à ouvrir"
						>
							<Icon class="size-[18px] shrink-0" />
							<span class={cn('transition-opacity', expanded ? 'opacity-100' : 'opacity-0')}>{item.label}</span>
						</span>
					{/if}
				{/each}
			{/each}
		</div>
		<button
			class="text-muted-foreground hover:bg-accent/60 hover:text-foreground mx-2 mb-2 flex h-9 items-center gap-3 rounded-md px-2.5 text-sm whitespace-nowrap"
			onclick={togglePin}
			aria-label={pinned ? 'Réduire la barre latérale' : 'Épingler la barre latérale'}
			title={pinned ? 'Réduire' : 'Épingler'}
		>
			{#if pinned}<PanelLeftClose class="size-[18px] shrink-0" />{:else}<PanelLeftOpen class="size-[18px] shrink-0" />{/if}
			<span class={cn('transition-opacity', expanded ? 'opacity-100' : 'opacity-0')}>
				{pinned ? 'Réduire' : 'Épingler'}
			</span>
		</button>
	</nav>
</div>
