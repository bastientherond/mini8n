<script lang="ts">
	import CircleCheck from '@lucide/svelte/icons/circle-check';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import Workflow from '@lucide/svelte/icons/workflow';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Switch } from '$lib/components/ui/switch';
	import { Textarea } from '$lib/components/ui/textarea';
	import { ApiError, api, errorMessage, type PublicForm } from '$lib/api';

	// Page publique (sans compte) d'un déclencheur « Formulaire ».
	let { token }: { token: string } = $props();

	let form = $state.raw<PublicForm | null>(null);
	let loadError = $state<string | null>(null);
	let values = $state<Record<string, unknown>>({});
	let busy = $state(false);
	let error = $state<string | null>(null);
	let done = $state<string | null>(null);

	$effect(() => {
		api.publicForm(token)
			.then((f) => {
				form = f;
				values = Object.fromEntries(f.fields.map((field) => [field.key, field.kind === 'case' ? false : '']));
			})
			.catch((e) => {
				loadError = e instanceof ApiError && e.status === 404 ? "Ce formulaire n'existe pas ou n'est plus actif." : errorMessage(e);
			});
	});

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		error = null;
		try {
			done = (await api.submitForm(token, values)).message;
		} catch (e) {
			error = errorMessage(e);
		} finally {
			busy = false;
		}
	}

	const inputType: Record<string, string> = { email: 'email', nombre: 'text', date: 'date', texte: 'text' };
</script>

<div class="bg-muted/40 flex min-h-dvh items-start justify-center p-4 sm:items-center">
	<div class="w-full max-w-lg space-y-4 py-8">
		{#if loadError}
			<Card.Root>
				<Card.Content class="text-muted-foreground py-8 text-center text-sm">{loadError}</Card.Content>
			</Card.Root>
		{:else if !form}
			<div class="text-muted-foreground flex items-center justify-center gap-2 text-sm"><LoaderCircle class="size-4 animate-spin" /> Chargement…</div>
		{:else if done}
			<Card.Root>
				<Card.Content class="flex flex-col items-center gap-3 py-10 text-center">
					<CircleCheck class="size-10 text-emerald-500" />
					<p class="font-medium">{done}</p>
				</Card.Content>
			</Card.Root>
		{:else}
			<Card.Root>
				<Card.Header>
					<Card.Title class="text-xl">{form.title || 'Formulaire'}</Card.Title>
					{#if form.description}<Card.Description class="whitespace-pre-line">{form.description}</Card.Description>{/if}
				</Card.Header>
				<Card.Content>
					<form class="space-y-4" onsubmit={submit}>
						{#each form.fields as field (field.key)}
							<div class="space-y-1.5">
								{#if field.kind === 'case'}
									<div class="flex items-center justify-between gap-3">
										<Label for="f-{field.key}">{field.label}{field.required ? ' *' : ''}</Label>
										<Switch id="f-{field.key}" checked={values[field.key] === true} onCheckedChange={(v) => (values[field.key] = v)} />
									</div>
								{:else}
									<Label for="f-{field.key}">{field.label}{field.required ? ' *' : ''}</Label>
									{#if field.kind === 'texte long'}
										<Textarea id="f-{field.key}" bind:value={values[field.key] as string} required={field.required} maxlength={10000} rows={5} />
									{:else}
										<Input
											id="f-{field.key}"
											type={inputType[field.kind] ?? 'text'}
											inputmode={field.kind === 'nombre' ? 'decimal' : undefined}
											bind:value={values[field.key] as string}
											required={field.required}
											maxlength={10000}
										/>
									{/if}
								{/if}
							</div>
						{/each}
						{#if error}<p class="text-destructive text-sm" role="alert">{error}</p>{/if}
						<Button type="submit" class="w-full" disabled={busy}>
							{#if busy}<LoaderCircle class="animate-spin" />{/if}
							Envoyer
						</Button>
					</form>
				</Card.Content>
			</Card.Root>
		{/if}
		<p class="text-muted-foreground flex items-center justify-center gap-1.5 text-xs">
			<Workflow class="size-3.5" /> Propulsé par mini8n
		</p>
	</div>
</div>
