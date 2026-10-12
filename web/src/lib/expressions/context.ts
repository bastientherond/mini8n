// Données disponibles pour les expressions d'un nœud : colonnes en entrée ($json),
// sorties des autres nœuds ($('Nom')) et variables propres au paramètre.

import { engine } from '$lib/engine/client';
import type { ColumnInfo, ParamSpec } from '$lib/engine/types';
import { wf } from '$lib/workflow.svelte';

export interface Table {
	/** Nom du nœud qui a produit la table. */
	name: string;
	columns: ColumnInfo[];
	rows: number;
	bytes?: Uint8Array;
}

export interface ExprContext {
	/** Table en entrée (`$json`) ; `null` si le nœud n'a pas d'entrée. */
	input: Table | null;
	/** Une entrée est reliée mais n'a pas encore été exécutée. */
	pendingInput: boolean;
	/** `$('Nom')` est-il évalué pour ce nœud ? (pas pour les intégrations exécutées sur le serveur) */
	allowNodes: boolean;
	/** Autres nœuds exécutés. */
	nodes: Table[];
	/** Variables propres au paramètre (ex. `nb_lignes`) avec une valeur d'exemple. */
	vars: [string, unknown][];
}

export const emptyContext: ExprContext = { input: null, pendingInput: false, allowNodes: false, nodes: [], vars: [] };

/** Type d'une colonne Arrow pour l'autocomplétion. */
export function kindOf(arrowType: string | undefined): 'string' | 'number' | 'boolean' | 'date' | 'any' {
	if (!arrowType) return 'any';
	if (/^(U?Int|Float|Decimal)/.test(arrowType)) return 'number';
	if (/^(Utf8|LargeUtf8|Utf8View)/.test(arrowType)) return 'string';
	if (arrowType === 'Boolean') return 'boolean';
	if (/^(Timestamp|Date)/.test(arrowType)) return 'date';
	return 'any';
}

export const kindLabels: Record<string, string> = {
	string: 'texte',
	number: 'nombre',
	boolean: 'booléen',
	date: 'date',
	list: 'liste',
	object: 'objet',
	null: 'null',
	any: 'valeur',
};

export function buildContext(nodeId: string, param: ParamSpec): ExprContext {
	const node = wf.nodes.find((n) => n.id === nodeId);
	const spec = node ? wf.spec(node.data.kind) : undefined;
	if (!node || !spec) return emptyContext;
	const table = (id: string): Table => {
		const n = wf.nodes.find((x) => x.id === id);
		const r = wf.results[id];
		return {
			name: n?.data.name ?? id,
			columns: r?.status === 'success' ? r.columns : [],
			rows: r?.status === 'success' ? r.rows : 0,
			bytes: wf.data[id],
		};
	};
	const edge = wf.edges.find((e) => e.target === nodeId && (e.targetHandle ?? 'in-0') === 'in-0');
	const input = spec.inputs.length > 0 && edge ? table(edge.source) : null;
	const allowNodes = spec.category !== 'integration' && spec.category !== 'trigger';
	const nodes = allowNodes
		? wf.nodes.filter((n) => n.id !== nodeId && wf.data[n.id]).map((n) => table(n.id))
		: [];
	const vars: [string, unknown][] = (param.vars ?? []).map((name) => [
		name,
		name === 'nb_lignes' ? (input?.rows ?? 0) : name === 'tableau' ? '(tableau des 20 premières lignes)' : '',
	]);
	return { input, pendingInput: !!input && !input.bytes, allowNodes, nodes, vars };
}

/** Première ligne d'une table (exemples de valeurs dans l'autocomplétion). */
const samples = new WeakMap<Uint8Array, Promise<Record<string, unknown>>>();

export function sampleRow(bytes: Uint8Array | undefined): Promise<Record<string, unknown>> {
	if (!bytes) return Promise.resolve({});
	let p = samples.get(bytes);
	if (!p) {
		p = engine
			.preview(bytes, 0, 1)
			.then((preview) => (preview.rows[0] as Record<string, unknown>) ?? {})
			.catch(() => ({}));
		samples.set(bytes, p);
	}
	return p;
}

const IDENT = /^[\p{L}_$][\p{L}\p{N}_$]*$/u;

/** Accès à une colonne : `$json.prix`, `$json.adresse.ville` ou `$json["code postal"]`. */
export function columnRef(name: string, base = '$json'): string {
	return name.split('.').every((p) => IDENT.test(p)) ? `${base}.${name}` : `${base}[${JSON.stringify(name)}]`;
}

/** Valeur courte affichée à côté d'une colonne. */
export function shortValue(v: unknown, max = 40): string {
	if (v === undefined) return '';
	if (v === null) return 'null';
	const s = typeof v === 'string' ? JSON.stringify(v) : typeof v === 'object' ? JSON.stringify(v) : String(v);
	return s.length > max ? `${s.slice(0, max - 1)}…` : s;
}
