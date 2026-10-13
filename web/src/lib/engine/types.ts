// Types partagés avec le moteur Rust (voir crates/engine/src/catalog.rs et workflow.rs).

export type ParamType =
	| 'string'
	| 'text'
	| 'expression'
	| 'sql'
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
	/** Le texte accepte des expressions `{{ … }}`. */
	template?: boolean;
	/** Variables propres à ce paramètre (ex. `nb_lignes`). */
	vars?: string[];
	/** Affiché seulement si le paramètre nommé vaut l'une de ces valeurs. */
	showIf?: [string, string[]];
}

export type Category = 'trigger' | 'source' | 'transform' | 'combine' | 'flow' | 'integration';

export interface NodeSpec {
	kind: string;
	label: string;
	category: Category;
	description: string;
	icon: string;
	inputs: string[];
	/** L'entrée peut rester non reliée (lecture de données). */
	optionalInput?: boolean;
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

/** Entrée de la documentation du langage d'expressions (voir crates/engine/src/expr/docs.rs). */
export interface ExprDoc {
	name: string;
	kind: 'variable' | 'function' | 'method' | 'property' | 'constant';
	/** global, string, number, list, object, date, any, Math, JSON, Object, input, item, legacy. */
	on: string;
	args?: string;
	returns: string;
	info: string;
	example: string;
	result?: string;
}

/** Données disponibles pour essayer une expression. */
export interface TrialContext {
	/** Table en entrée (Parquet). */
	input?: Uint8Array | null;
	/** Sorties des autres nœuds, par nom. */
	nodes?: Record<string, Uint8Array>;
	vars?: [string, unknown][];
	row?: number;
}

export interface Trial {
	ok: boolean;
	value?: unknown;
	/** string, number, boolean, list, object, date, null. */
	kind?: string;
	error?: string;
	/** Position de l'erreur (UTF-16). */
	from?: number;
	to?: number;
	rows: number;
}
