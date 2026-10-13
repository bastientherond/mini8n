<script lang="ts">
	// Création / modification d'un identifiant. Les secrets ne sont jamais relus depuis le
	// serveur : en modification, un champ secret laissé vide conserve sa valeur.
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import { toast } from 'svelte-sonner';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import * as Select from '$lib/components/ui/select';
	import { api, errorMessage, type CredentialInfo } from '$lib/api';
	import { brandOfCredential, brands, chatService } from '$lib/brands';
	import { resources } from '$lib/resources.svelte';
	import BrandLogo from './BrandLogo.svelte';

	let {
		open = $bindable(false),
		kind = '',
		preset = '',
		credential = null,
		onsaved,
	}: {
		open?: boolean;
		/** Type imposé (création depuis un nœud). */
		kind?: string;
		/** Type proposé par défaut, modifiable. */
		preset?: string;
		credential?: CredentialInfo | null;
		onsaved?: (c: CredentialInfo) => void;
	} = $props();

	let name = $state('');
	let selectedKind = $state('');
	let values = $state<Record<string, string>>({});
	let busy = $state(false);
	let testing = $state(false);
	let error = $state<string | null>(null);

	const spec = $derived(resources.kind(selectedKind));
	const editing = $derived(!!credential);
	/** Service de messagerie reconnu : d'après l'URL saisie, sinon celui déjà enregistré. */
	const service = $derived.by(() => {
		if (selectedKind !== 'chat_webhook') return null;
		const typed = values.url?.trim();
		if (typed) return chatService(typed);
		const saved = credential?.summary.service;
		return typeof saved === 'string' && saved in brands ? (saved as keyof typeof brands) : null;
	});
	const brand = $derived(service ?? brandOfCredential(selectedKind));

	$effect(() => {
		if (!open) return;
		error = null;
		selectedKind = credential?.kind ?? (kind || preset || resources.kinds[0]?.kind || '');
		name = credential?.name ?? '';
		const k = resources.kind(selectedKind);
		values = Object.fromEntries(
			(k?.fields ?? []).map((f) => {
				const current = credential?.summary[f.name];
				return [f.name, f.secret ? '' : String(current ?? f.default ?? '')];
			}),
		);
	});

	function changeKind(v: string) {
		selectedKind = v;
		const k = resources.kind(v);
		values = Object.fromEntries((k?.fields ?? []).map((f) => [f.name, String(f.default ?? '')]));
		if (!name) name = k?.label ?? '';
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		if (!resources.workspaceId || !spec) return;
		busy = true;
		error = null;
		try {
			const saved = credential
				? await api.updateCredential(resources.workspaceId, credential.id, name, values)
				: await api.createCredential(resources.workspaceId, name.trim() || spec.label, spec.kind, values);
			await resources.refreshCredentials();
			toast.success(credential ? 'Identifiant mis à jour' : 'Identifiant créé');
			onsaved?.(saved);
			open = false;
		} catch (e) {
			error = errorMessage(e);
		} finally {
			busy = false;
		}
	}

	async function test() {
		if (!resources.workspaceId || !credential) return;
		testing = true;
		try {
			const r = await api.testCredential(resources.workspaceId, credential.id);
			if (r.ok) toast.success(r.message);
			else toast.error(r.message);
		} catch (e) {
			toast.error(errorMessage(e));
		} finally {
			testing = false;
		}
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="sm:max-w-lg">
		<Dialog.Header>
			<div class="flex items-center gap-3">
				{#if spec}<BrandLogo {brand} class="size-10" />{/if}
				<div class="min-w-0">
					<Dialog.Title>{editing ? `Modifier « ${credential?.name} »` : 'Nouvel identifiant'}</Dialog.Title>
					{#if spec}<p class="text-muted-foreground truncate text-sm">{service ?? spec.label}</p>{/if}
				</div>
			</div>
			<Dialog.Description class="flex items-start gap-1.5">
				<ShieldCheck class="mt-0.5 size-4 shrink-0 text-emerald-600" />
				Chiffré sur le serveur ; les mots de passe ne sont jamais renvoyés au navigateur ni enregistrés dans les workflows.
			</Dialog.Description>
		</Dialog.Header>
		<form class="space-y-4" onsubmit={save}>
			{#if !editing && !kind}
				<div class="space-y-1.5">
					<Label>Type</Label>
					<Select.Root type="single" value={selectedKind} onValueChange={changeKind}>
						<Select.Trigger class="w-full">
							{#if spec}<BrandLogo brand={brandOfCredential(spec.kind)} class="size-5 rounded" />{/if}
							{spec?.label ?? 'Choisir…'}
						</Select.Trigger>
						<Select.Content>
							{#each resources.kinds as k (k.kind)}
								<Select.Item value={k.kind} label={k.label}>
									<BrandLogo brand={brandOfCredential(k.kind)} class="size-5 rounded" />
									{k.label}
								</Select.Item>
							{/each}
						</Select.Content>
					</Select.Root>
					{#if spec}<p class="text-muted-foreground text-xs">{spec.description}</p>{/if}
				</div>
			{/if}
			<div class="space-y-1.5">
				<Label for="cred-name">Nom</Label>
				<Input id="cred-name" bind:value={name} placeholder={spec?.label} maxlength={80} />
			</div>
			{#if spec}
				<div class="grid gap-3 sm:grid-cols-2">
					{#each spec.fields as f (f.name)}
						<div class={['space-y-1.5', (f.name === 'host' || f.name === 'url' || f.name === 'from') && 'sm:col-span-2']}>
							<Label for="cred-{f.name}">{f.label}{f.required && !(f.secret && editing) ? ' *' : ''}</Label>
							{#if f.type === 'select'}
								<Select.Root type="single" value={values[f.name]} onValueChange={(v) => (values[f.name] = v)}>
									<Select.Trigger id="cred-{f.name}" class="w-full">{values[f.name]}</Select.Trigger>
									<Select.Content>
										{#each f.options ?? [] as o (o)}
											<Select.Item value={o} label={o}>{o}</Select.Item>
										{/each}
									</Select.Content>
								</Select.Root>
							{:else}
								<Input
									id="cred-{f.name}"
									type={f.type === 'password' ? 'password' : f.type === 'number' ? 'number' : 'text'}
									bind:value={values[f.name]}
									placeholder={f.secret && editing && credential?.summary[`${f.name}_defini`] ? '•••••••• (inchangé)' : (f.placeholder ?? '')}
									autocomplete={f.secret ? 'new-password' : 'off'}
									maxlength={2048}
								/>
							{/if}
							{#if selectedKind === 'chat_webhook' && f.name === 'url'}
								<p class="text-muted-foreground flex items-center gap-1.5 text-xs">
									{#if service}
										<BrandLogo brand={service} class="size-4 rounded" /> {brands[service].label} reconnu.
									{:else if values.url?.trim()}
										Service non reconnu : le nœud « Message » enverra le format du service qu'il indique.
									{:else}
										Slack, Discord, Microsoft Teams, Mattermost ou Google Chat : le service est reconnu d'après l'URL.
									{/if}
								</p>
							{/if}
						</div>
					{/each}
				</div>
			{/if}
			{#if error}<p class="text-destructive text-sm" role="alert">{error}</p>{/if}
			<Dialog.Footer class="gap-2">
				{#if editing}
					<Button type="button" variant="outline" disabled={testing} onclick={test}>
						{#if testing}<LoaderCircle class="animate-spin" />{/if}
						Tester la connexion
					</Button>
				{/if}
				<Button type="submit" disabled={busy || !spec}>
					{#if busy}<LoaderCircle class="animate-spin" />{/if}
					Enregistrer
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
