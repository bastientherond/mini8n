<script lang="ts">
	// Légende du calendrier. Les listes de mois et d'années utilisent le Select de
	// l'interface (et non un <select> natif, dont l'apparence dépend du système).
	import { DateFormatter, getLocalTimeZone, today, type DateValue } from "@internationalized/date";
	import * as Select from "$lib/components/ui/select";
	import type Calendar from "./calendar.svelte";
	import type { ComponentProps } from "svelte";

	type CalendarProps = ComponentProps<typeof Calendar>;

	let {
		captionLayout,
		months,
		monthFormat,
		years,
		yearFormat,
		month,
		locale,
		placeholder = $bindable(),
		monthIndex = 0,
	}: {
		captionLayout: CalendarProps["captionLayout"];
		months: CalendarProps["months"];
		monthFormat: CalendarProps["monthFormat"];
		years: CalendarProps["years"];
		yearFormat: CalendarProps["yearFormat"];
		month: DateValue;
		placeholder: DateValue | undefined;
		locale: string;
		monthIndex: number;
	} = $props();

	function formatYear(date: DateValue) {
		const dateObj = date.toDate(getLocalTimeZone());
		if (typeof yearFormat === "function") return yearFormat(dateObj.getFullYear());
		return new DateFormatter(locale, { year: yearFormat ?? "numeric" }).format(dateObj);
	}

	function formatMonth(date: DateValue) {
		const dateObj = date.toDate(getLocalTimeZone());
		if (typeof monthFormat === "function") return monthFormat(dateObj.getMonth() + 1);
		return new DateFormatter(locale, { month: monthFormat ?? "long" }).format(dateObj);
	}

	const monthItems = $derived(
		(months ?? Array.from({ length: 12 }, (_, i) => i + 1)).map((m) => ({
			value: String(m),
			label: formatMonth(month.set({ month: m, day: 1 })),
		})),
	);

	const yearItems = $derived.by(() => {
		const current = today(getLocalTimeZone()).year;
		const list = years ?? Array.from({ length: 111 }, (_, i) => current - 100 + i);
		// L'année affichée doit toujours figurer dans la liste.
		const all = list.includes(month.year) ? list : [...list, month.year].sort((a, b) => a - b);
		return all.map((y) => ({ value: String(y), label: formatYear(month.set({ year: y })) }));
	});

	function setMonth(v: string) {
		if (!placeholder) return;
		placeholder = placeholder.set({ month: Number(v) }).subtract({ months: monthIndex });
	}

	function setYear(v: string) {
		if (!placeholder) return;
		placeholder = placeholder.set({ year: Number(v) });
	}
</script>

{#snippet MonthSelect()}
	<Select.Root type="single" value={String(month.month)} onValueChange={setMonth}>
		<Select.Trigger size="sm" class="h-7 gap-1 px-2 text-sm font-medium capitalize" aria-label="Mois">
			{formatMonth(month)}
		</Select.Trigger>
		<Select.Content class="max-h-64">
			{#each monthItems as item (item.value)}
				<Select.Item value={item.value} label={item.label} class="capitalize">{item.label}</Select.Item>
			{/each}
		</Select.Content>
	</Select.Root>
{/snippet}

{#snippet YearSelect()}
	<Select.Root type="single" value={String(month.year)} onValueChange={setYear}>
		<Select.Trigger size="sm" class="h-7 gap-1 px-2 text-sm font-medium" aria-label="Année">
			{formatYear(month)}
		</Select.Trigger>
		<Select.Content class="max-h-64">
			{#each yearItems as item (item.value)}
				<Select.Item value={item.value} label={item.label}>{item.label}</Select.Item>
			{/each}
		</Select.Content>
	</Select.Root>
{/snippet}

{#if captionLayout === "dropdown"}
	{@render MonthSelect()}
	{@render YearSelect()}
{:else if captionLayout === "dropdown-months"}
	{@render MonthSelect()}
	{#if placeholder}
		{formatYear(placeholder)}
	{/if}
{:else if captionLayout === "dropdown-years"}
	{#if placeholder}
		{formatMonth(placeholder)}
	{/if}
	{@render YearSelect()}
{:else}
	<span class="capitalize">{formatMonth(month)} {formatYear(month)}</span>
{/if}
