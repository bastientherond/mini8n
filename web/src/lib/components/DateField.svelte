<script lang="ts">
	// Champ date aux couleurs de l'interface (au lieu du sélecteur natif du système).
	// Valeur : texte ISO « aaaa-mm-jj », vide si aucune date.
	import CalendarIcon from '@lucide/svelte/icons/calendar';
	import X from '@lucide/svelte/icons/x';
	import { type DateValue, getLocalTimeZone, parseDate, today } from '@internationalized/date';
	import { Calendar } from '$lib/components/ui/calendar';
	import * as Popover from '$lib/components/ui/popover';
	import { cn } from '$lib/utils';

	let {
		value = $bindable(''),
		id,
		placeholder = 'Choisir une date',
		required = false,
	}: { value?: string; id?: string; placeholder?: string; required?: boolean } = $props();

	let open = $state(false);

	const date = $derived.by((): DateValue | undefined => {
		try {
			return value ? parseDate(value) : undefined;
		} catch {
			return undefined;
		}
	});

	const label = $derived(
		date
			? new Intl.DateTimeFormat('fr-FR', { dateStyle: 'long' }).format(date.toDate(getLocalTimeZone()))
			: placeholder,
	);
</script>

<Popover.Root bind:open>
	<Popover.Trigger
		{id}
		class={cn(
			'border-input dark:bg-input/30 focus-visible:border-ring focus-visible:ring-ring/50 flex h-8 w-full items-center gap-2 rounded-lg border bg-transparent px-2.5 text-left text-sm transition-[color,box-shadow] outline-none focus-visible:ring-3',
			!date && 'text-muted-foreground',
		)}
		aria-required={required || undefined}
	>
		<CalendarIcon class="text-muted-foreground size-4 shrink-0" />
		<span class="flex-1 truncate">{label}</span>
		{#if date && !required}
			<span
				role="button"
				tabindex="-1"
				class="text-muted-foreground hover:text-foreground -mr-1 rounded p-0.5"
				aria-label="Effacer la date"
				onclick={(e) => {
					e.stopPropagation();
					value = '';
				}}
				onkeydown={() => {}}
			>
				<X class="size-3.5" />
			</span>
		{/if}
	</Popover.Trigger>
	<Popover.Content class="w-auto p-0" align="start">
		<Calendar
			type="single"
			value={date}
			placeholder={date ?? today(getLocalTimeZone())}
			captionLayout="dropdown"
			locale="fr-FR"
			weekStartsOn={1}
			onValueChange={(v) => {
				value = v ? v.toString() : '';
				open = false;
			}}
		/>
	</Popover.Content>
</Popover.Root>
