/// <reference lib="webworker" />
// Le moteur WebAssembly tourne dans un Web Worker pour ne jamais bloquer l'interface.

import init, * as engine from './pkg/mini8n_engine.js';
import wasmUrl from './pkg/mini8n_engine_bg.wasm?url';

export type Request =
	| { id: number; method: 'catalog' }
	| { id: number; method: 'version' }
	| { id: number; method: 'run'; workflow: string; target?: string }
	| { id: number; method: 'preview'; bytes: Uint8Array; offset: number; limit: number }
	| { id: number; method: 'csv' | 'json'; bytes: Uint8Array };

const ready = init({ module_or_path: wasmUrl });

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
				const out = await engine.runWorkflow(req.workflow, req.target ?? null);
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
