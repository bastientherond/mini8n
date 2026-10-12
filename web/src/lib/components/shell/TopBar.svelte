<script lang="ts">
	import Logo from '../Logo.svelte';
	import Building2 from '@lucide/svelte/icons/building-2';
	import Check from '@lucide/svelte/icons/check';
	import ChevronsUpDown from '@lucide/svelte/icons/chevrons-up-down';
	import CircleHelp from '@lucide/svelte/icons/circle-help';
	import FilePlus from '@lucide/svelte/icons/file-plus';
	import ListTree from '@lucide/svelte/icons/list-tree';
	import Plus from '@lucide/svelte/icons/plus';
	import Search from '@lucide/svelte/icons/search';
	import Settings from '@lucide/svelte/icons/settings';
	import Users from '@lucide/svelte/icons/users';
	import { Button } from '$lib/components/ui/button';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { href, router } from '$lib/router.svelte';
	import { session } from '$lib/session.svelte';
	import { ui } from '$lib/ui.svelte';
	import { wf } from '$lib/workflow.svelte';
	import UserMenu from './UserMenu.svelte';

	const ws = $derived(session.current);
	const roleLabel = $derived(ws?.role === 'owner' ? 'PROPRIÉTAIRE' : 'MEMBRE');
	const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);

	const currentWorkflow = $derived(
		router.route.name === 'editor' ? wf.workflows.find((w) => w.id === wf.workflowId) : undefined,
	);
	const workflowLabel = $derived(currentWorkflow ? wf.name || currentWorkflow.name : null);
</script>

<header class="bg-sidebar/80 supports-[backdrop-filter]:bg-sidebar/70 flex h-12 backdrop-blur shrink-0 items-center gap-1 border-b px-3">
	<a
		href={ws ? href({ name: 'home', ws: ws.id }) : '#/'}
		class="mr-1 shrink-0 rounded-lg transition-transform hover:scale-105"
		aria-label="Accueil mini8n"
	>
		<Logo class="size-7" />
	</a>

	<span class="text-muted-foreground/60 px-1 text-lg font-light select-none">/</span>

	<!-- Workspaces -->
	<DropdownMenu.Root>
		<div class="flex items-center">
			<a
				href={ws ? href({ name: 'home', ws: ws.id }) : '#/'}
				class="hover:bg-accent flex h-8 items-center gap-2 rounded-md px-2 text-sm font-medium"
			>
				<Building2 class="text-muted-foreground size-4" />
				<span class="max-w-48 truncate">{ws?.name ?? 'Workspace'}</span>
				{#if ws}
					<span class="text-muted-foreground rounded-full border px-1.5 py-px text-[10px] font-medium tracking-wide">
						{roleLabel}
					</span>
				{/if}
			</a>
			<DropdownMenu.Trigger>
				{#snippet child({ props })}
					<Button {...props} variant="ghost" size="icon-sm" aria-label="Changer de workspace">
						<ChevronsUpDown class="text-muted-foreground" />
					</Button>
				{/snippet}
			</DropdownMenu.Trigger>
		</div>
		<DropdownMenu.Content align="start" class="w-72">
			<DropdownMenu.Label class="text-muted-foreground text-xs">Workspaces</DropdownMenu.Label>
			<div class="max-h-72 overflow-y-auto">
				{#each session.workspaces as item (item.id)}
					<DropdownMenu.Item onclick={() => router.go({ name: 'home', ws: item.id })}>
						<Building2 class="text-muted-foreground" />
						<span class="flex-1 truncate">{item.name}</span>
						<span class="text-muted-foreground text-xs">{item.memberCount} membre{item.memberCount > 1 ? 's' : ''}</span>
						{#if item.id === session.currentId}<Check />{/if}
					</DropdownMenu.Item>
				{/each}
			</div>
			<DropdownMenu.Separator />
			<DropdownMenu.Item onclick={() => (ui.newWorkspace = true)}><Plus /> Nouveau workspace</DropdownMenu.Item>
			{#if ws}
				<DropdownMenu.Item onclick={() => router.go({ name: 'members', ws: ws.id })}><Users /> Membres</DropdownMenu.Item>
				<DropdownMenu.Item onclick={() => router.go({ name: 'settings', ws: ws.id })}><Settings /> Paramètres du workspace</DropdownMenu.Item>
			{/if}
		</DropdownMenu.Content>
	</DropdownMenu.Root>

	{#if ws}
		<span class="text-muted-foreground/60 px-1 text-lg font-light select-none">/</span>

		<!-- Workflows -->
		<DropdownMenu.Root>
			<DropdownMenu.Trigger>
				{#snippet child({ props })}
					<button {...props} class="hover:bg-accent flex h-8 items-center gap-2 rounded-md px-2 text-sm">
						{#if workflowLabel}
							<span class="max-w-56 truncate font-medium">{workflowLabel}</span>
						{:else}
							<span class="text-muted-foreground">Workflows</span>
						{/if}
						<ChevronsUpDown class="text-muted-foreground size-4" />
					</button>
				{/snippet}
			</DropdownMenu.Trigger>
			<DropdownMenu.Content align="start" class="w-72">
				<DropdownMenu.Label class="text-muted-foreground text-xs">Workflows · {ws.name}</DropdownMenu.Label>
				<div class="max-h-72 overflow-y-auto">
					{#each wf.workflows as item (item.id)}
						<DropdownMenu.Item onclick={() => router.go({ name: 'editor', ws: ws.id, workflow: item.id })}>
							<span class="flex-1 truncate">{item.id === wf.workflowId ? wf.name || item.name : item.name}</span>
							{#if item.id === wf.workflowId && router.route.name === 'editor'}<Check />{/if}
						</DropdownMenu.Item>
					{:else}
						<p class="text-muted-foreground px-2 py-1.5 text-sm">Aucun workflow pour l'instant.</p>
					{/each}
				</div>
				<DropdownMenu.Separator />
				<DropdownMenu.Item onclick={() => (ui.newWorkflow = true)}><FilePlus /> Nouveau workflow</DropdownMenu.Item>
				<DropdownMenu.Item onclick={() => router.go({ name: 'workflows', ws: ws.id })}><ListTree /> Tous les workflows</DropdownMenu.Item>
			</DropdownMenu.Content>
		</DropdownMenu.Root>
	{/if}

	<div class="ml-auto flex items-center gap-2">
		<button
			class="text-muted-foreground hover:bg-accent hover:text-foreground flex h-8 w-56 items-center gap-2 rounded-md border px-2.5 text-sm"
			onclick={() => (ui.palette = true)}
		>
			<Search class="size-4" />
			<span class="flex-1 text-left">Rechercher…</span>
			<kbd class="bg-muted rounded px-1.5 font-mono text-[10px]">{isMac ? '⌘' : 'Ctrl'} K</kbd>
		</button>
		<Button variant="outline" size="icon" class="rounded-full" onclick={() => (ui.shortcuts = true)} aria-label="Aide et raccourcis">
			<CircleHelp />
		</Button>
		<UserMenu />
	</div>
</header>
