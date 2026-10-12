// Types partagés avec le moteur Rust (voir crates/engine/src/catalog.rs et workflow.rs).

export type ParamType =
	| 'string'
	| 'text'
	| 'expression'
	| 'json'
	| 'number'
	| 'boolean'
	| 'select'
	| 'token'
	| 'workflow'
	| 'credential'
	| 'file';

export interface ParamSpec {
	name: string;
	label: string;
	type: ParamType;
	default: unknown;
	options?: string[];
	placeholder?: string;
	help?: string;
	/** Type d'identifiant attendu (paramètres `credential`). */
	credential?: string;
}

export type Category = 'trigger' | 'source' | 'transform' | 'combine' | 'flow' | 'integration';

export interface NodeSpec {
	kind: string;
	label: string;
	category: Category;
	description: string;
	icon: string;
	inputs: string[];
	params: ParamSpec[];
}

export interface ColumnInfo {
	name: string;
	type: string;
}

export type NodeStatus = 'success' | 'error' | 'skipped';

export interface NodeResult {
	status: NodeStatus;
	error?: string;
	rows: number;
	columns: ColumnInfo[];
	durationMs: number;
	bytes: number;
}

export interface RunReport {
	ok: boolean;
	error?: string;
	order: string[];
	results: Record<string, NodeResult>;
	durationMs: number;
	/** Nœud de sortie : dernier nœud terminal exécuté avec succès. */
	output?: string;
}

export interface RunOutput {
	report: RunReport;
	/** Sortie de chaque nœud réussi, au format Parquet. */
	data: Record<string, Uint8Array>;
}

export interface Preview {
	columns: ColumnInfo[];
	rows: Record<string, unknown>[];
	total: number;
	offset: number;
}

/** Format de workflow attendu par le moteur. */
export interface EngineWorkflow {
	nodes: { id: string; kind: string; name: string; params: Record<string, unknown>; disabled: boolean }[];
	edges: { source: string; target: string; input: number }[];
}
