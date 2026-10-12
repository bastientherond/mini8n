/// <reference lib="webworker" />
// Le moteur WebAssembly tourne dans un Web Worker pour ne jamais bloquer l'interface.

import init, * as engine from './pkg/mini8n_engine.js';
import wasmUrl from './pkg/mini8n_engine_bg.wasm?url';
import { toEngineWorkflow } from './convert';
import type { RunReport } from './types';

export type Request =
	| { id: number; method: 'catalog' }
	| { id: number; method: 'version' }
	| { id: number; method: 'run'; workflow: string; target?: string; workspaceId?: string }
	| { id: number; method: 'preview'; bytes: Uint8Array; offset: number; limit: number }
	| { id: number; method: 'csv' | 'json'; bytes: Uint8Array };

const ready = init({ module_or_path: wasmUrl });

/** Même limite que le serveur. */
const MAX_DEPTH = 5;

/**
 * Hôte du moteur dans le navigateur : le nœud « Exécuter un workflow » charge le
 * sous-workflow via l'API (session de l'utilisateur) et l'exécute dans ce même moteur.
 */
function hostFor(workspaceId: string | undefined, depth: number) {
	return {
		/** Intégrations (bases de données, fichiers, e-mail…) : exécutées par l'API, jamais ici. */
		runIntegration: async (nodeJson: string, inputs: Uint8Array[]): Promise<Uint8Array> => {
			if (!workspaceId) throw new Error('Intégration indisponible hors workspace');
			const toBase64 = (bytes: Uint8Array) => {
				let binary = '';
				for (let i = 0; i < bytes.length; i += 0x8000) binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
				return btoa(binary);
			};
			const res = await fetch(`/api/workspaces/${encodeURIComponent(workspaceId)}/integrations/run`, {
				method: 'POST',
				credentials: 'same-origin',
				cache: 'no-store',
				headers: { 'Content-Type': 'application/json' },
				body: JSON.stringify({ node: JSON.parse(nodeJson), inputs: inputs.map(toBase64) }),
			});
			if (!res.ok) {
				const data = await res.json().catch(() => null);
				throw new Error((data as { error?: string } | null)?.error ?? `Erreur ${res.status}`);
			}
			return new Uint8Array(await res.arrayBuffer());
		},
		callWorkflow: async (workflowId: string, input: Uint8Array): Promise<Uint8Array> => {
			if (!workspaceId) throw new Error("L'appel d'un autre workflow nécessite un workspace");
			if (depth + 1 >= MAX_DEPTH) throw new Error(`Trop d'appels imbriqués (${MAX_DEPTH} niveaux max.)`);
			const res = await fetch(
				`/api/workspaces/${encodeURIComponent(workspaceId)}/workflows/${encodeURIComponent(workflowId)}`,
				{ credentials: 'same-origin', cache: 'no-store' },
			);
			if (!res.ok) throw new Error(res.status === 404 ? 'Workflow appelé introuvable' : `Chargement du workflow : ${res.status}`);
			const doc = (await res.json()) as { name: string; definition: Parameters<typeof toEngineWorkflow>[0] };
			const wf = toEngineWorkflow(doc.definition);
			const trigger = wf.nodes.find((n) => n.kind === 'trigger_workflow' && !n.disabled);
			if (!trigger) throw new Error(`Le workflow « ${doc.name} » n'a pas de déclencheur « Appelé par un workflow »`);
			const out = await engine.runWorkflow(
				JSON.stringify(wf),
				null,
				{ nodeId: trigger.id, data: input },
				hostFor(workspaceId, depth + 1),
			);
			const report = JSON.parse(out.report) as RunReport;
			const failed = report.error ?? Object.values(report.results).find((r) => r.status === 'error')?.error;
			if (failed) throw new Error(`Le workflow « ${doc.name} » a échoué : ${failed}`);
			const bytes = report.output ? (out.data as Record<string, Uint8Array>)[report.output] : undefined;
			if (!bytes) throw new Error(`Le workflow « ${doc.name} » n'a produit aucune sortie`);
			return bytes;
		},
	};
}

self.onmessage = async (event: MessageEvent<Request>) => {
	const req = event.data;
	try {
		await ready;
		let result: unknown;
		let transfer: Transferable[] = [];
		switch (req.method) {
			case 'catalog':
				result = JSON.parse(engine.catalog());
				break;
			case 'version':
				result = engine.version();
				break;
			case 'run': {
				const out = await engine.runWorkflow(req.workflow, req.target ?? null, null, hostFor(req.workspaceId, 0));
				const data = out.data as Record<string, Uint8Array>;
				transfer = Object.values(data).map((d) => d.buffer as ArrayBuffer);
				result = { report: JSON.parse(out.report), data };
				break;
			}
			case 'preview':
				result = JSON.parse(engine.preview(req.bytes, req.offset, req.limit));
				break;
			case 'csv':
				result = engine.toCsv(req.bytes);
				break;
			case 'json':
				result = engine.toJson(req.bytes);
				break;
		}
		self.postMessage({ id: req.id, ok: true, result }, { transfer });
	} catch (err) {
		const message = err instanceof Error ? err.message : String(err);
		self.postMessage({ id: req.id, ok: false, error: message });
	}
};
