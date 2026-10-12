<script lang="ts">
	import ArrowRight from '@lucide/svelte/icons/arrow-right';
	import FilePlus from '@lucide/svelte/icons/file-plus';
	import FileText from '@lucide/svelte/icons/file-text';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import Upload from '@lucide/svelte/icons/upload';
	import UserPlus from '@lucide/svelte/icons/user-plus';
	import Users from '@lucide/svelte/icons/users';
	import WorkflowIcon from '@lucide/svelte/icons/workflow';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import { formatRelative } from '$lib/icons';
	import { href } from '$lib/router.svelte';
	import { session } from '$lib/session.svelte';
	import { createAndOpenWorkflow, pickWorkflowFile, ui } from '$lib/ui.svelte';
	import { wf } from '$lib/workflow.svelte';
	import Page from './Page.svelte';

	const ws = $derived(session.current!);
	const recent = $derived(wf.workflows.slice(0, 6));
	const stats = $derived([
		{ label: 'Workflows', value: wf.workflows.length, icon: WorkflowIcon, tone: 'bg-primary/10 text-primary' },
		{ label: 'Membres', value: ws.memberCount, icon: Users, tone: 'bg-sky-500/10 text-sky-600 dark:text-sky-400' },
		{
			label: 'Votre rôle',
			value: ws.role === 'owner' ? 'Propriétaire' : 'Membre',
			icon: ShieldCheck,
			tone: 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400',
		},
	]);
</script>

<Page title={ws.name} description="Vue d'ensemble du workspace">
	{#snippet actions()}
		<Button variant="outline" size="sm" onclick={pickWorkflowFile}><Upload /> Importer</Button>
		<Button size="sm" onclick={() => (ui.newWorkflow = true)}><FilePlus /> Nouveau workflow</Button>
	{/snippet}

	<div class="mb-8 grid gap-3 sm:grid-cols-3">
		{#each stats as stat (stat.label)}
			{@const Icon = stat.icon}
			<Card.Root class="gap-1 py-5 shadow-sm">
				<Card.Content class="flex items-center gap-4 px-5">
					<span class={['flex size-11 items-center justify-center rounded-xl', stat.tone]}><Icon class="size-5" /></span>
					<div>
						<div class="text-muted-foreground text-xs font-medium">{stat.label}</div>
						<div class="text-xl font-semibold tracking-tight tabular-nums">{stat.value}</div>
					</div>
				</Card.Content>
			</Card.Root>
		{/each}
	</div>

	<div class="mb-3 flex items-center justify-between">
		<h2 class="text-base font-semibold tracking-tight">Workflows récents</h2>
		{#if wf.workflows.length > 0}
			<a href={href({ name: 'workflows', ws: ws.id })} class="text-muted-foreground hover:text-foreground flex items-center gap-1 text-sm">
				Tout voir <ArrowRight class="size-3.5" />
			</a>
		{/if}
	</div>

	{#if wf.loading}
		<div class="text-muted-foreground flex items-center gap-2 py-10 text-sm"><LoaderCircle class="size-4 animate-spin" /> Chargement…</div>
	{:else if recent.length === 0}
		<div class="bg-card/50 flex flex-col items-center gap-4 rounded-2xl border border-dashed px-6 py-14 text-center">
			<span class="bg-brand flex size-14 items-center justify-center rounded-2xl text-white shadow-lg shadow-primary/25"><WorkflowIcon class="size-7" /></span>
			<div>
				<p class="font-medium">Aucun workflow dans ce workspace</p>
				<p class="text-muted-foreground text-sm">Partez d'un canevas vide ou de l'exemple fourni.</p>
			</div>
			<div class="flex gap-2">
				<Button variant="outline" onclick={() => createAndOpenWorkflow('Exemple : ventes', true)}><Sparkles /> Depuis l'exemple</Button>
				<Button onclick={() => (ui.newWorkflow = true)}><FilePlus /> Nouveau workflow</Button>
			</div>
		</div>
	{:else}
		<div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
			{#each recent as item (item.id)}
				<a
					href={href({ name: 'editor', ws: ws.id, workflow: item.id })}
					class="group bg-card hover:border-primary/40 flex flex-col gap-4 rounded-xl border p-4 shadow-sm transition-all hover:-translate-y-0.5 hover:shadow-md"
				>
					<div class="flex items-start justify-between gap-2">
						<span class="bg-primary/10 text-primary flex size-9 items-center justify-center rounded-lg"><FileText class="size-4" /></span>
						<ArrowRight class="text-primary size-4 -translate-x-1 opacity-0 transition-all group-hover:translate-x-0 group-hover:opacity-100" />
					</div>
					<div class="min-w-0">
						<div class="truncate font-medium">{item.name}</div>
						<div class="text-muted-foreground truncate text-xs">
							Modifié {formatRelative(item.updatedAt)}{item.updatedBy ? ` par ${item.updatedBy}` : ''}
						</div>
					</div>
				</a>
			{/each}
		</div>
	{/if}

	{#if ws.role === 'owner' && ws.memberCount <= 1}
		<Card.Root class="mt-8">
			<Card.Content class="flex flex-wrap items-center justify-between gap-4">
				<div>
					<p class="font-medium">Travaillez à plusieurs</p>
					<p class="text-muted-foreground text-sm">Ajoutez des personnes déjà inscrites à ce workspace.</p>
				</div>
				<Button variant="outline" href={href({ name: 'members', ws: ws.id })}><UserPlus /> Inviter des membres</Button>
			</Card.Content>
		</Card.Root>
	{/if}
</Page>
