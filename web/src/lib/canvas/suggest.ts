// Nœuds proposés quand on tire un lien dans le vide ou qu'on insère un nœud sur un lien
// (testé avec `node --test`) : ceux qui peuvent se brancher à cet endroit, et en tête
// les suites les plus courantes de l'étape voisine.

import type { NodeSpec } from '../engine/types';

/** Étapes qui suivent habituellement un type de nœud. */
const AFTER_KIND: Record<string, string[]> = {
	trigger_webhook: ['filter', 'formula', 'chat_message', 'email_send', 'postgres_write'],
	trigger_form: ['filter', 'formula', 'email_send', 'chat_message', 'postgres_write'],
	trigger_poll: ['distinct', 'filter', 'chat_message', 'email_send'],
	trigger_error: ['email_send', 'chat_message', 'formula'],
	trigger_workflow: ['filter', 'formula', 'aggregate', 'select'],
	filter: ['sort', 'formula', 'aggregate', 'limit', 'email_send'],
	formula: ['filter', 'select', 'aggregate', 'sort'],
	select: ['rename', 'sort', 'file_write', 'email_send'],
	rename: ['select', 'sort', 'file_write', 'postgres_write'],
	sort: ['limit', 'select', 'file_write', 'email_send'],
	limit: ['email_send', 'chat_message', 'file_write', 'select'],
	distinct: ['sort', 'filter', 'aggregate', 'chat_message'],
	aggregate: ['sort', 'formula', 'email_send', 'file_write', 'chat_message'],
	merge: ['filter', 'select', 'formula', 'sort'],
};

/** À défaut, d'après la catégorie. */
const AFTER_CATEGORY: Record<string, string[]> = {
	trigger: ['filter', 'formula', 'subworkflow', 'email_send'],
	source: ['filter', 'formula', 'select', 'sort', 'aggregate', 'merge'],
	integration: ['filter', 'formula', 'select', 'aggregate', 'merge'],
	transform: ['filter', 'formula', 'sort', 'aggregate', 'file_write'],
	combine: ['filter', 'select', 'formula', 'sort'],
	flow: ['filter', 'formula', 'select', 'email_send'],
};

/** Après une écriture ou un envoi : prévenir quelqu'un. */
const AFTER_OUTPUT = ['chat_message', 'email_send'];
const OUTPUTS = /_write$|^email_send$|^chat_message$/;

/** Ce qui alimente habituellement un nœud : une source de données ou un déclencheur. */
const BEFORE = ['csv', 'manual', 'http', 'postgres_query', 'file_read', 'trigger_webhook', 'trigger_form', 'trigger_schedule'];
const BEFORE_KIND: Record<string, string[]> = {
	email_send: ['aggregate', 'filter', 'formula'],
	chat_message: ['aggregate', 'filter', 'formula'],
	file_write: ['select', 'sort', 'aggregate'],
	merge: ['csv', 'postgres_query', 'http', 'file_read', 'manual'],
};

export const MAX_SUGGESTIONS = 5;

export interface Proposal {
	/** Nœuds proposés en premier. */
	suggested: NodeSpec[];
	/** Tous les nœuds qui peuvent se brancher à cet endroit (suggestions comprises). */
	candidates: NodeSpec[];
}

/**
 * `after` : nœuds à brancher sur la sortie de `neighbour` (il leur faut une entrée).
 * `before` : nœuds à brancher sur une entrée de `neighbour` (tous ont une sortie).
 */
export function propose(catalog: NodeSpec[], neighbour: NodeSpec | undefined, direction: 'after' | 'before'): Proposal {
	const candidates =
		direction === 'after' ? catalog.filter((s) => s.category !== 'trigger' && s.inputs.length > 0) : [...catalog];
	let wanted: string[];
	if (direction === 'before') {
		wanted = [...(neighbour ? (BEFORE_KIND[neighbour.kind] ?? []) : []), ...BEFORE];
	} else if (!neighbour) {
		wanted = AFTER_CATEGORY.source;
	} else if (OUTPUTS.test(neighbour.kind)) {
		wanted = AFTER_OUTPUT;
	} else {
		wanted = [...(AFTER_KIND[neighbour.kind] ?? []), ...(AFTER_CATEGORY[neighbour.category] ?? [])];
	}
	const byKind = new Map(candidates.map((s) => [s.kind, s]));
	const suggested = [...new Set(wanted)]
		.filter((k) => direction === 'after' || k !== neighbour?.kind)
		.flatMap((k) => byKind.get(k) ?? [])
		.slice(0, MAX_SUGGESTIONS);
	return { suggested, candidates };
}
