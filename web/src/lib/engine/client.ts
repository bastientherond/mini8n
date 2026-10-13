// Client RPC vers le Worker qui héberge le moteur WebAssembly.
// Si le moteur plante ou dépasse le délai imparti, le Worker est recréé.

import type { EngineWorkflow, ExprDoc, NodeSpec, Preview, RunOutput, Trial, TrialContext } from './types';
import type { Progress, Request } from './worker';

export type { Progress };

type Pending = {
	resolve: (v: unknown) => void;
	reject: (e: Error) => void;
	timer: ReturnType<typeof setTimeout>;
	onProgress?: (p: Progress) => void;
};

// Distribue une requête sans son identifiant.
type DistributiveOmit<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;

const RUN_TIMEOUT_MS = 120_000;
const CALL_TIMEOUT_MS = 30_000;

const pending = new Map<number, Pending>();
let seq = 0;
let worker = spawn();

function spawn(): Worker {
	const w = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module', name: 'mini8n-engine' });
	w.onmessage = (event: MessageEvent<{ id: number; ok: boolean; result?: unknown; error?: string; progress?: Progress }>) => {
		const { id, ok, result, error, progress } = event.data;
		const p = pending.get(id);
		if (!p) return;
		if (progress) {
			p.onProgress?.(progress);
			return;
		}
		clearTimeout(p.timer);
		pending.delete(id);
		if (ok) p.resolve(result);
		else p.reject(new Error(error));
	};
	w.onerror = (event) => {
		event.preventDefault();
		restart(new Error(event.message || 'Le moteur WebAssembly a planté'));
	};
	return w;
}

/** Abandonne les appels en cours et repart d'un Worker neuf. */
function restart(reason: Error) {
	worker.terminate();
	for (const p of pending.values()) {
		clearTimeout(p.timer);
		p.reject(reason);
	}
	pending.clear();
	worker = spawn();
}

function call<T>(
	req: DistributiveOmit<Request, 'id'>,
	timeoutMs = CALL_TIMEOUT_MS,
	onProgress?: (p: Progress) => void,
): Promise<T> {
	const id = ++seq;
	return new Promise<T>((resolve, reject) => {
		const timer = setTimeout(() => {
			restart(new Error(`Délai dépassé (${Math.round(timeoutMs / 1000)} s) : exécution interrompue`));
		}, timeoutMs);
		pending.set(id, { resolve: resolve as (v: unknown) => void, reject, timer, onProgress });
		worker.postMessage({ ...req, id });
	});
}

/** Clés des tables Parquet transmises au Worker pour les essais d'expressions. */
const tableKeys = new WeakMap<Uint8Array, string>();
const sentTables = new Set<string>();
let tableSeq = 0;

function keyFor(bytes: Uint8Array): string {
	let key = tableKeys.get(bytes);
	if (!key) {
		key = `t${++tableSeq}`;
		tableKeys.set(bytes, key);
	}
	return key;
}

let docs: Promise<ExprDoc[]> | null = null;

async function tryExpression(src: string, template: boolean, ctx: TrialContext): Promise<Trial> {
	const byKey = new Map<string, Uint8Array>();
	const input = ctx.input ? keyFor(ctx.input) : null;
	if (input && ctx.input) byKey.set(input, ctx.input);
	const nodes: Record<string, string> = {};
	for (const [name, bytes] of Object.entries(ctx.nodes ?? {})) {
		const key = keyFor(bytes);
		nodes[name] = key;
		byKey.set(key, bytes);
	}
	const context = JSON.stringify({ input, nodes, vars: ctx.vars ?? [], row: ctx.row ?? 0 });
	const keys = [...byKey.keys()];
	const send = (force: string[]) => {
		const tables: Record<string, Uint8Array> = {};
		for (const [k, b] of byKey) if (force.includes(k) || !sentTables.has(k)) tables[k] = b;
		for (const k of Object.keys(tables)) sentTables.add(k);
		return call<Trial | { missing: string[] }>({ method: 'tryExpression', src, template, context, tables, keys });
	};
	let out = await send([]);
	if ('missing' in out) out = await send(out.missing);
	if ('missing' in out) throw new Error('Données indisponibles');
	return out;
}

export const engine = {
	catalog: () => call<NodeSpec[]>({ method: 'catalog' }),
	version: () => call<string>({ method: 'version' }),
	/** `onProgress` : début et fin de chaque nœud, au fil de l'exécution. */
	run: (workflow: EngineWorkflow, target?: string, workspaceId?: string, onProgress?: (p: Progress) => void) =>
		call<RunOutput>({ method: 'run', workflow: JSON.stringify(workflow), target, workspaceId }, RUN_TIMEOUT_MS, onProgress),
	preview: (bytes: Uint8Array, offset: number, limit: number) =>
		call<Preview>({ method: 'preview', bytes, offset, limit }),
	toCsv: (bytes: Uint8Array) => call<string>({ method: 'csv', bytes }),
	toJson: (bytes: Uint8Array) => call<string>({ method: 'json', bytes }),
	/** Documentation du langage d'expressions (chargée une fois). */
	expressionDocs: () => (docs ??= call<ExprDoc[]>({ method: 'expressionDocs' }).catch((e) => {
		docs = null;
		throw e;
	})),
	tryExpression,
};
