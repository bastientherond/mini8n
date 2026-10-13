<script lang="ts">
	import { Bot, Check, Copy, Trash2 } from '@lucide/svelte';
	import { toast } from 'svelte-sonner';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import * as Select from '$lib/components/ui/select';
	import { api, errorMessage, type ApiToken } from '$lib/api';
	import { formatDateTime } from '$lib/icons';

	let { workspaceId }: { workspaceId: string } = $props();

	const VALIDITY: Record<string, string> = { '7': '7 jours', '30': '30 jours', '90': '90 jours', '365': '1 an', never: 'Sans expiration' };

	let tokens = $state<ApiToken[]>([]);
	let name = $state('Claude');
	let validity = $state('30');
	let busy = $state(false);
	/** Jeton tout juste créé : affiché une seule fois. */
	let created = $state<string | null>(null);
	let copied = $state('');

	const endpoint = `${location.origin}/api/mcp`;
	const command = $derived(
		`claude mcp add --transport http mini8n ${endpoint} --header "Authorization: Bearer ${created ?? '<jeton>'}"`,
	);

	async function load() {
		try {
			tokens = await api.tokens(workspaceId);
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	$effect(() => {
		void workspaceId;
		created = null;
		void load();
	});

	async function create(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		try {
			const t = await api.createToken(workspaceId, name.trim(), validity === 'never' ? null : Number(validity));
			created = t.token;
			await load();
		} catch (e) {
			toast.error(errorMessage(e));
		} finally {
			busy = false;
		}
	}

	async function revoke(t: ApiToken) {
		try {
			await api.deleteToken(workspaceId, t.id);
			toast.success(`Jeton « ${t.name} » révoqué`);
			await load();
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	async function copy(text: string, what: string) {
		try {
			await navigator.clipboard.writeText(text);
			copied = what;
			setTimeout(() => (copied = ''), 1500);
		} catch {
			toast.error('Copie impossible : sélectionnez le texte manuellement.');
		}
	}
</script>

<Card.Root>
	<Card.Header>
		<Card.Title class="flex items-center gap-2 text-base"><Bot class="size-4" /> Accès des IA (MCP)</Card.Title>
		<Card.Description>
			Connectez Claude (Claude Code, Claude Desktop) ou une autre IA compatible MCP à ce workspace : elle peut lire les workflows,
			les exécuter, voir les données de chaque nœud et les corriger. Ses modifications et exécutions apparaissent en direct dans
			l'éditeur. Elle agit en votre nom : les nœuds d'envoi (e-mails, messages, écritures en base) agissent réellement.
		</Card.Description>
	</Card.Header>
	<Card.Content class="space-y-5">
		<form class="flex max-w-xl flex-wrap items-end gap-2" onsubmit={create}>
			<div class="min-w-48 flex-1 space-y-1.5">
				<Label for="token-name">Nom du jeton</Label>
				<Input id="token-name" bind:value={name} maxlength={60} placeholder="Claude" />
			</div>
			<div class="space-y-1.5">
				<Label>Validité</Label>
				<Select.Root type="single" bind:value={validity}>
					<Select.Trigger class="w-40">{VALIDITY[validity]}</Select.Trigger>
					<Select.Content>
						{#each Object.entries(VALIDITY) as [value, label] (value)}
							<Select.Item {value} {label}>{label}</Select.Item>
						{/each}
					</Select.Content>
				</Select.Root>
			</div>
			<Button type="submit" disabled={busy || !name.trim()}>Créer un jeton</Button>
		</form>

		{#if created}
			<div class="border-primary/40 bg-primary/5 space-y-3 rounded-lg border p-3 text-sm">
				<p class="font-medium">Copiez ce jeton maintenant : il ne sera plus affiché.</p>
				<div class="flex items-center gap-2">
					<code class="bg-muted min-w-0 flex-1 truncate rounded px-2 py-1 font-mono text-xs">{created}</code>
					<Button size="sm" variant="outline" onclick={() => copy(created!, 'token')}>
						{#if copied === 'token'}<Check />{:else}<Copy />{/if} Copier
					</Button>
				</div>
				<p class="text-muted-foreground">Dans un terminal, pour Claude Code :</p>
				<div class="flex items-start gap-2">
					<pre class="bg-muted min-w-0 flex-1 overflow-x-auto rounded px-2 py-1.5 font-mono text-[11px] leading-relaxed">{command}</pre>
					<Button size="sm" variant="outline" onclick={() => copy(command, 'command')}>
						{#if copied === 'command'}<Check />{:else}<Copy />{/if}
					</Button>
				</div>
				<p class="text-muted-foreground">
					Autres clients MCP : serveur HTTP « {endpoint} » avec l'en-tête <code>Authorization: Bearer &lt;jeton&gt;</code>.
				</p>
			</div>
		{/if}

		{#if tokens.length > 0}
			<ul class="divide-y rounded-lg border text-sm">
				{#each tokens as t (t.id)}
					<li class="flex items-center gap-3 px-3 py-2">
						<div class="min-w-0 flex-1">
							<p class="truncate font-medium">{t.name}</p>
							<p class="text-muted-foreground text-xs">
								Créé le {formatDateTime(t.createdAt)} · {t.lastUsedAt ? `utilisé le ${formatDateTime(t.lastUsedAt)}` : 'jamais utilisé'} ·
								{t.expiresAt ? `expire le ${formatDateTime(t.expiresAt)}` : 'sans expiration'}
							</p>
						</div>
						<Button size="sm" variant="ghost" aria-label="Révoquer « {t.name} »" onclick={() => revoke(t)}><Trash2 /> Révoquer</Button>
					</li>
				{/each}
			</ul>
		{:else}
			<p class="text-muted-foreground text-sm">Aucun jeton pour l'instant.</p>
		{/if}
	</Card.Content>
</Card.Root>
