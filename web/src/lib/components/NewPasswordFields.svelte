<script lang="ts">
	// Nouveau mot de passe + confirmation, avec les règles affichées pendant la saisie.
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { passwordChecks } from '$lib/password';

	let {
		password = $bindable(''),
		valid = $bindable(false),
		id = 'new-password',
	}: { password?: string; valid?: boolean; id?: string } = $props();

	let confirm = $state('');
	const checks = $derived(passwordChecks(password, confirm));
	$effect(() => {
		valid = checks.every((c) => c.ok);
	});
</script>

<div class="space-y-1.5">
	<Label for={id}>Mot de passe</Label>
	<Input {id} type="password" bind:value={password} autocomplete="new-password" maxlength={128} required />
</div>
<div class="space-y-1.5">
	<Label for="{id}-confirm">Confirmer le mot de passe</Label>
	<Input id="{id}-confirm" type="password" bind:value={confirm} autocomplete="new-password" maxlength={128} required />
</div>
<ul class="space-y-0.5 text-xs" aria-label="Exigences du mot de passe">
	{#each checks as check (check.label)}
		<li class={check.ok ? 'text-emerald-600 dark:text-emerald-400' : 'text-muted-foreground'}>
			{check.ok ? '✓' : '•'} {check.label}
		</li>
	{/each}
</ul>
