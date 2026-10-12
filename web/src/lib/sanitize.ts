// Validation stricte des workflows venant de l'extérieur (fichier importé, serveur).
// On reconstruit des objets neufs : aucune clé inattendue (ex. __proto__) n'est conservée.

import type { NodeSpec } from './engine/types';
import type { SavedWorkflow, StepData } from './workflow.svelte';

const ID_RE = /^[A-Za-z0-9_-]{1,64}$/;
const HANDLE_RE = /^in-[0-9]$/;
const MAX_NODES = 500;
const MAX_EDGES = 2000;
const MAX_TEXT = 20 * 1024 * 1024;

const isObject = (v: unknown): v is Record<string, unknown> =>
	typeof v === 'object' && v !== null && !Array.isArray(v);

const finite = (v: unknown) => (typeof v === 'number' && Number.isFinite(v) ? v : 0);

function cleanText(v: unknown, max: number): string {
	return typeof v === 'string' ? v.slice(0, max) : '';
}

function cleanParam(v: unknown): unknown {
	if (typeof v === 'string') return v.slice(0, MAX_TEXT);
	if (typeof v === 'number') return Number.isFinite(v) ? v : null;
	if (typeof v === 'boolean' || v === null) return v;
	return null;
}

export class InvalidWorkflow extends Error {}

export function sanitizeWorkflow(raw: unknown, catalog: NodeSpec[]): SavedWorkflow {
	if (!isObject(raw) || !Array.isArray(raw.nodes) || !Array.isArray(raw.edges)) {
		throw new InvalidWorkflow('Fichier de workflow invalide');
	}
	if (raw.nodes.length > MAX_NODES || raw.edges.length > MAX_EDGES) {
		throw new InvalidWorkflow(`Workflow trop grand (${MAX_NODES} nœuds max.)`);
	}
	const specs = new Map(catalog.map((s) => [s.kind, s]));
	const ids = new Set<string>();
	const nodes: SavedWorkflow['nodes'] = [];
	for (const n of raw.nodes) {
		if (!isObject(n) || typeof n.id !== 'string' || !ID_RE.test(n.id) || ids.has(n.id)) {
			throw new InvalidWorkflow('Nœud invalide dans le workflow');
		}
		const data = isObject(n.data) ? n.data : {};
		const spec = typeof data.kind === 'string' ? specs.get(data.kind) : undefined;
		if (!spec) throw new InvalidWorkflow(`Type de nœud inconnu : ${String(data.kind).slice(0, 40)}`);
		const rawParams = isObject(data.params) ? data.params : {};
		const params: Record<string, unknown> = {};
		for (const p of spec.params) {
			params[p.name] = Object.hasOwn(rawParams, p.name) ? cleanParam(rawParams[p.name]) : p.default;
		}
		const position = isObject(n.position) ? n.position : {};
		const clean: StepData = {
			kind: spec.kind,
			name: cleanText(data.name, 120) || spec.label,
			params,
			disabled: data.disabled === true,
		};
		ids.add(n.id);
		nodes.push({ id: n.id, position: { x: finite(position.x), y: finite(position.y) }, data: clean });
	}
	const edges: SavedWorkflow['edges'] = [];
	for (const e of raw.edges) {
		if (!isObject(e) || typeof e.source !== 'string' || typeof e.target !== 'string') continue;
		if (!ids.has(e.source) || !ids.has(e.target) || e.source === e.target) continue;
		const handle = typeof e.targetHandle === 'string' && HANDLE_RE.test(e.targetHandle) ? e.targetHandle : 'in-0';
		const id = typeof e.id === 'string' && ID_RE.test(e.id) ? e.id : `e-${e.source}-${e.target}-${handle}`;
		edges.push({ id, source: e.source, target: e.target, targetHandle: handle });
	}
	return { version: 1, name: cleanText(raw.name, 120) || 'Workflow importé', nodes, edges };
}

const SECRET_HEADER = /(auth|token|key|secret|cookie|password|session)/i;

/** Masque les en-têtes sensibles (nœuds HTTP) avant un export de fichier. */
export function redactSecrets(wf: SavedWorkflow): { workflow: SavedWorkflow; redacted: number } {
	let redacted = 0;
	const nodes = wf.nodes.map((n) => {
		const headers = n.data.params.headers;
		if (typeof headers !== 'string' || !headers.trim()) return n;
		const lines = headers.split('\n').map((line) => {
			const i = line.indexOf(':');
			if (i > 0 && SECRET_HEADER.test(line.slice(0, i))) {
				redacted++;
				return `${line.slice(0, i)}: ***`;
			}
			return line;
		});
		return { ...n, data: { ...n.data, params: { ...n.data.params, headers: lines.join('\n') } } };
	});
	return { workflow: { ...wf, nodes }, redacted };
}
