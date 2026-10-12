<script lang="ts">
	import Building2 from '@lucide/svelte/icons/building-2';
	import Check from '@lucide/svelte/icons/check';
	import FilePlus from '@lucide/svelte/icons/file-plus';
	import Keyboard from '@lucide/svelte/icons/keyboard';
	import KeyRound from '@lucide/svelte/icons/key-round';
	import LogOut from '@lucide/svelte/icons/log-out';
	import MonitorSmartphone from '@lucide/svelte/icons/monitor-smartphone';
	import Palette from '@lucide/svelte/icons/palette';
	import Plus from '@lucide/svelte/icons/plus';
	import Search from '@lucide/svelte/icons/search';
	import Settings from '@lucide/svelte/icons/settings';
	import Upload from '@lucide/svelte/icons/upload';
	import UserRound from '@lucide/svelte/icons/user-round';
	import Users from '@lucide/svelte/icons/users';
	import { setMode, userPrefersMode } from 'mode-watcher';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { router } from '$lib/router.svelte';
	import { session } from '$lib/session.svelte';
	import { logout, pickWorkflowFile, ui } from '$lib/ui.svelte';

	const initials = $derived(
		(session.user?.name ?? '?')
			.split(/\s+/)
			.map((p) => p[0])
			.join('')
			.slice(0, 2)
			.toUpperCase(),
	);
	const ws = $derived(session.current);

	const themes = [
		{ value: 'light', label: 'Clair' },
		{ value: 'dark', label: 'Sombre' },
		{ value: 'system', label: 'Système' },
	] as const;
</script>

<DropdownMenu.Root>
	<DropdownMenu.Trigger>
		{#snippet child({ props })}
			<button
				{...props}
				class="bg-primary text-primary-foreground ring-offset-background focus-visible:ring-ring hover:ring-border flex size-8 items-center justify-center rounded-full text-xs font-semibold outline-none hover:ring-2 focus-visible:ring-2"
				aria-label="Menu du compte"
			>
				{initials}
			</button>
		{/snippet}
	</DropdownMenu.Trigger>
	<DropdownMenu.Content align="end" class="w-64">
		<div class="flex items-center gap-3 px-2 py-2">
			<span class="bg-primary text-primary-foreground flex size-9 shrink-0 items-center justify-center rounded-full text-xs font-semibold">
				{initials}
			</span>
			<div class="min-w-0">
				<div class="truncate text-sm font-medium">{session.user?.name}</div>
				<div class="text-muted-foreground truncate text-xs">{session.user?.email}</div>
			</div>
		</div>
		<DropdownMenu.Separator />

		<DropdownMenu.Group>
			<DropdownMenu.Item onclick={() => router.go({ name: 'account' })}><UserRound /> Mon compte</DropdownMenu.Item>
			<DropdownMenu.Item onclick={() => router.go({ name: 'account' })}><KeyRound /> Changer le mot de passe</DropdownMenu.Item>
		</DropdownMenu.Group>
		<DropdownMenu.Separator />

		{#if ws}
			<DropdownMenu.Group>
				<DropdownMenu.GroupHeading class="text-muted-foreground text-xs font-normal">{ws.name}</DropdownMenu.GroupHeading>
				<DropdownMenu.Item onclick={() => (ui.newWorkflow = true)}><FilePlus /> Nouveau workflow</DropdownMenu.Item>
				<DropdownMenu.Item onclick={pickWorkflowFile}><Upload /> Importer un workflow…</DropdownMenu.Item>
				<DropdownMenu.Item onclick={() => router.go({ name: 'members', ws: ws.id })}><Users /> Membres</DropdownMenu.Item>
				<DropdownMenu.Item onclick={() => router.go({ name: 'settings', ws: ws.id })}><Settings /> Paramètres du workspace</DropdownMenu.Item>
			</DropdownMenu.Group>
			<DropdownMenu.Separator />
		{/if}

		<DropdownMenu.Sub>
			<DropdownMenu.SubTrigger><Building2 /> Changer de workspace</DropdownMenu.SubTrigger>
			<DropdownMenu.SubContent class="w-56">
				{#each session.workspaces as item (item.id)}
					<DropdownMenu.Item onclick={() => router.go({ name: 'home', ws: item.id })}>
						<span class="flex-1 truncate">{item.name}</span>
						{#if item.id === session.currentId}<Check />{/if}
					</DropdownMenu.Item>
				{/each}
				<DropdownMenu.Separator />
				<DropdownMenu.Item onclick={() => (ui.newWorkspace = true)}><Plus /> Nouveau workspace</DropdownMenu.Item>
			</DropdownMenu.SubContent>
		</DropdownMenu.Sub>
		<DropdownMenu.Sub>
			<DropdownMenu.SubTrigger><Palette /> Thème</DropdownMenu.SubTrigger>
			<DropdownMenu.SubContent>
				<DropdownMenu.RadioGroup
					value={userPrefersMode.current}
					onValueChange={(v) => setMode(v as 'light' | 'dark' | 'system')}
				>
					{#each themes as theme (theme.value)}
						<DropdownMenu.RadioItem value={theme.value}>{theme.label}</DropdownMenu.RadioItem>
					{/each}
				</DropdownMenu.RadioGroup>
			</DropdownMenu.SubContent>
		</DropdownMenu.Sub>
		<DropdownMenu.Item onclick={() => (ui.palette = true)}>
			<Search /> Palette de commandes
			<DropdownMenu.Shortcut>⌘K</DropdownMenu.Shortcut>
		</DropdownMenu.Item>
		<DropdownMenu.Item onclick={() => (ui.shortcuts = true)}><Keyboard /> Raccourcis clavier</DropdownMenu.Item>
		<DropdownMenu.Separator />

		<DropdownMenu.Item onclick={() => logout(true)}><MonitorSmartphone /> Déconnecter tous les appareils</DropdownMenu.Item>
		<DropdownMenu.Item variant="destructive" onclick={() => logout()}><LogOut /> Se déconnecter</DropdownMenu.Item>
	</DropdownMenu.Content>
</DropdownMenu.Root>
