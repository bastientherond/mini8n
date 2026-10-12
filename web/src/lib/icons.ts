import type { Component } from 'svelte';
import ArrowDownWideNarrow from '@lucide/svelte/icons/arrow-down-wide-narrow';
import ClipboardList from '@lucide/svelte/icons/clipboard-list';
import Clock from '@lucide/svelte/icons/clock';
import CornerDownRight from '@lucide/svelte/icons/corner-down-right';
import MousePointerClick from '@lucide/svelte/icons/mouse-pointer-click';
import Radar from '@lucide/svelte/icons/radar';
import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
import Webhook from '@lucide/svelte/icons/webhook';
import WorkflowIcon from '@lucide/svelte/icons/workflow';
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
	'clipboard-list': ClipboardList,
	clock: Clock,
	'corner-down-right': CornerDownRight,
	'mouse-pointer-click': MousePointerClick,
	radar: Radar,
	'triangle-alert': TriangleAlert,
	webhook: Webhook,
	workflow: WorkflowIcon,
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

/** Ordre d'affichage des catégories dans la palette. */
export const categoryLabels: Record<string, string> = {
	trigger: 'Déclencheurs',
	source: 'Sources',
	transform: 'Transformations',
	combine: 'Combinaisons',
	flow: 'Flux',
};

/** Couleur d'accent par catégorie (classes Tailwind). */
export const categoryAccent: Record<string, string> = {
	source: 'bg-emerald-500/15 text-emerald-600 dark:text-emerald-400',
	transform: 'bg-sky-500/15 text-sky-600 dark:text-sky-400',
	combine: 'bg-violet-500/15 text-violet-600 dark:text-violet-400',
	trigger: 'bg-amber-500/15 text-amber-600 dark:text-amber-400',
	flow: 'bg-rose-500/15 text-rose-600 dark:text-rose-400',
};

export const triggerKindLabels: Record<string, string> = {
	manual: 'Manuel',
	schedule: 'Planification',
	webhook: 'Webhook',
	form: 'Formulaire',
	poll: "Surveillance d'API",
	workflow: 'Appel de workflow',
	error: 'Sur erreur',
};

export function formatDuration(ms: number | null | undefined): string {
	if (ms == null) return '—';
	if (ms < 1000) return `${Math.round(ms)} ms`;
	if (ms < 60_000) return `${(ms / 1000).toFixed(1)} s`;
	return `${Math.floor(ms / 60_000)} min ${Math.round((ms % 60_000) / 1000)} s`;
}

export function formatDateTime(iso: string): string {
	return new Intl.DateTimeFormat('fr-FR', { dateStyle: 'short', timeStyle: 'medium' }).format(new Date(iso));
}

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
