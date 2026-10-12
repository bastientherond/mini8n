import type { Component } from 'svelte';
import ArrowDownWideNarrow from '@lucide/svelte/icons/arrow-down-wide-narrow';
import Box from '@lucide/svelte/icons/box';
import Braces from '@lucide/svelte/icons/braces';
import Columns3 from '@lucide/svelte/icons/columns-3';
import CopyMinus from '@lucide/svelte/icons/copy-minus';
import Funnel from '@lucide/svelte/icons/funnel';
import Globe from '@lucide/svelte/icons/globe';
import Group from '@lucide/svelte/icons/group';
import ListEnd from '@lucide/svelte/icons/list-end';
import Merge from '@lucide/svelte/icons/merge';
import Pencil from '@lucide/svelte/icons/pencil';
import Sheet from '@lucide/svelte/icons/sheet';
import Sigma from '@lucide/svelte/icons/sigma';

// Les noms d'icônes viennent du catalogue Rust (champ `icon`).
const ICONS: Record<string, Component> = {
	'arrow-down-wide-narrow': ArrowDownWideNarrow,
	braces: Braces,
	'columns-3': Columns3,
	'copy-minus': CopyMinus,
	funnel: Funnel,
	globe: Globe,
	group: Group,
	'list-end': ListEnd,
	merge: Merge,
	pencil: Pencil,
	sheet: Sheet,
	sigma: Sigma,
};

export const iconFor = (name: string | undefined): Component => (name && ICONS[name]) || Box;

export const categoryLabels: Record<string, string> = {
	source: 'Sources',
	transform: 'Transformations',
	combine: 'Combinaisons',
};

/** Couleur d'accent par catégorie (classes Tailwind). */
export const categoryAccent: Record<string, string> = {
	source: 'bg-emerald-500/15 text-emerald-600 dark:text-emerald-400',
	transform: 'bg-sky-500/15 text-sky-600 dark:text-sky-400',
	combine: 'bg-violet-500/15 text-violet-600 dark:text-violet-400',
};

export function formatBytes(n: number): string {
	if (n < 1024) return `${n} o`;
	if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} Ko`;
	return `${(n / 1024 / 1024).toFixed(1)} Mo`;
}

export function formatCount(n: number): string {
	return new Intl.NumberFormat('fr-FR').format(n);
}

const RELATIVE = new Intl.RelativeTimeFormat('fr', { numeric: 'auto' });

/** « il y a 5 minutes », « hier »… */
export function formatRelative(iso: string): string {
	const seconds = (new Date(iso).getTime() - Date.now()) / 1000;
	const units: [Intl.RelativeTimeFormatUnit, number][] = [
		['year', 31_536_000],
		['month', 2_592_000],
		['week', 604_800],
		['day', 86_400],
		['hour', 3_600],
		['minute', 60],
	];
	for (const [unit, size] of units) {
		if (Math.abs(seconds) >= size) return RELATIVE.format(Math.round(seconds / size), unit);
	}
	return "à l'instant";
}
