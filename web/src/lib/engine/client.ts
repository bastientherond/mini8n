// Client RPC vers le Worker qui héberge le moteur WebAssembly.
// Si le moteur plante ou dépasse le délai imparti, le Worker est recréé.

import type { EngineWorkflow, NodeSpec, Preview, RunOutput } from './types';
import type { Request } from './worker';

type Pending = { resolve: (v: unknown) => void; reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> };

// Distribue une requête sans son identifiant.
type DistributiveOmit<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;

const RUN_TIMEOUT_MS = 120_000;
const CALL_TIMEOUT_MS = 30_000;

const pending = new Map<number, Pending>();
let seq = 0;
let worker = spawn();

function spawn(): Worker {
	const w = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module', name: 'mini8n-engine' });
	w.onmessage = (event: MessageEvent<{ id: number; ok: boolean; result?: unknown; error?: string }>) => {
		const { id, ok, result, error } = event.data;
		const p = pending.get(id);
		if (!p) return;
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

function call<T>(req: DistributiveOmit<Request, 'id'>, timeoutMs = CALL_TIMEOUT_MS): Promise<T> {
	const id = ++seq;
	return new Promise<T>((resolve, reject) => {
		const timer = setTimeout(() => {
			restart(new Error(`Délai dépassé (${Math.round(timeoutMs / 1000)} s) : exécution interrompue`));
		}, timeoutMs);
		pending.set(id, { resolve: resolve as (v: unknown) => void, reject, timer });
		worker.postMessage({ ...req, id });
	});
}

export const engine = {
	catalog: () => call<NodeSpec[]>({ method: 'catalog' }),
	version: () => call<string>({ method: 'version' }),
	run: (workflow: EngineWorkflow, target?: string, workspaceId?: string) =>
		call<RunOutput>({ method: 'run', workflow: JSON.stringify(workflow), target, workspaceId }, RUN_TIMEOUT_MS),
	preview: (bytes: Uint8Array, offset: number, limit: number) =>
		call<Preview>({ method: 'preview', bytes, offset, limit }),
	toCsv: (bytes: Uint8Array) => call<string>({ method: 'csv', bytes }),
	toJson: (bytes: Uint8Array) => call<string>({ method: 'json', bytes }),
};
