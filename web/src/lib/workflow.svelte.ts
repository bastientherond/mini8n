// État de l'éditeur : graphe Svelte Flow, catalogue des nœuds, résultats d'exécution
// et synchronisation avec le serveur (workflows du workspace courant).

import type { Edge, Node, XYPosition } from '@xyflow/svelte';
import { ApiError, api, errorMessage, type WorkflowDoc, type WorkflowSummary } from './api';
import { engine } from './engine/client';
import type { EngineWorkflow, NodeResult, NodeSpec, RunReport } from './engine/types';
import { sanitizeWorkflow } from './sanitize';

export type StepData = {
	kind: string;
	name: string;
	params: Record<string, unknown>;
	disabled?: boolean;
};

export type StepNode = Node<StepData, 'step'>;

export interface SavedWorkflow {
	version: 1;
	name: string;
	nodes: { id: string; position: XYPosition; data: StepData }[];
	edges: { id: string; source: string; target: string; targetHandle?: string | null }[];
}

export type SaveState = 'idle' | 'saving' | 'saved' | 'error' | 'conflict';

const LAST_WORKFLOW_KEY = 'mini8n.lastWorkflow';
const SAVE_DELAY_MS = 800;

export const inputHandle = (index: number) => `in-${index}`;
const inputIndex = (handle: string | null | undefined) => Number(handle?.replace('in-', '') ?? 0) || 0;

const uid = () => crypto.randomUUID().slice(0, 8);

class WorkflowState {
	name = $state('');
	nodes = $state.raw<StepNode[]>([]);
	edges = $state.raw<Edge[]>([]);
	catalog = $state.raw<NodeSpec[]>([]);
	/** Résultats du dernier lancement, par nœud. */
	results = $state.raw<Record<string, NodeResult>>({});
	/** Sortie Parquet de chaque nœud réussi. */
	data = $state.raw<Record<string, Uint8Array>>({});
	lastRun = $state.raw<RunReport | null>(null);
	running = $state(false);
	runningIds = $state.raw<Set<string>>(new Set());
	loadError = $state<string | null>(null);

	// --- Serveur ---
	workspaceId = $state<string | null>(null);
	workflowId = $state<string | null>(null);
	workflows = $state.raw<WorkflowSummary[]>([]);
	saveState = $state<SaveState>('idle');
	saveError = $state<string | null>(null);
	loading = $state(false);
	private serverVersion = 0;
	private savedSnapshot = '';
	private saveTimer: ReturnType<typeof setTimeout> | undefined;
	private saving: Promise<void> | null = null;

	selected = $derived(this.nodes.find((n) => n.selected) ?? null);

	async loadCatalog() {
		try {
			this.catalog = await engine.catalog();
		} catch (e) {
			this.loadError = errorMessage(e);
		}
	}

	spec(kind: string): NodeSpec | undefined {
		return this.catalog.find((s) => s.kind === kind);
	}

	defaults(spec: NodeSpec): Record<string, unknown> {
		return Object.fromEntries(spec.params.map((p) => [p.name, p.default]));
	}

	addNode(kind: string, position: XYPosition, select = true) {
		const spec = this.spec(kind);
		if (!spec) return;
		const taken = new Set(this.nodes.map((n) => n.data.name));
		let name = spec.label;
		for (let i = 2; taken.has(name); i++) name = `${spec.label} ${i}`;
		const node: StepNode = {
			id: `${kind}-${uid()}`,
			type: 'step',
			position,
			selected: select,
			data: { kind, name, params: this.defaults(spec) },
		};
		const others = select ? this.nodes.map((n) => (n.selected ? { ...n, selected: false } : n)) : this.nodes;
		this.nodes = [...others, node];
		return node.id;
	}

	updateData(id: string, patch: Partial<StepData>) {
		this.nodes = this.nodes.map((n) => (n.id === id ? { ...n, data: { ...n.data, ...patch } } : n));
	}

	setParam(id: string, name: string, value: unknown) {
		const node = this.nodes.find((n) => n.id === id);
		if (!node) return;
		this.updateData(id, { params: { ...node.data.params, [name]: value } });
	}

	removeNode(id: string) {
		this.nodes = this.nodes.filter((n) => n.id !== id);
		this.edges = this.edges.filter((e) => e.source !== id && e.target !== id);
	}

	duplicateNode(id: string) {
		const node = this.nodes.find((n) => n.id === id);
		if (!node) return;
		const newId = this.addNode(node.data.kind, { x: node.position.x + 40, y: node.position.y + 60 });
		if (newId) this.updateData(newId, { params: structuredClone(node.data.params) });
	}

	select(id: string | null) {
		this.nodes = this.nodes.map((n) => ({ ...n, selected: n.id === id }));
	}

	/** Colonnes disponibles en entrée d'un nœud (d'après le dernier lancement). */
	inputColumns(id: string): string[] {
		const names = new Set<string>();
		for (const e of this.edges.filter((e) => e.target === id)) {
			for (const c of this.results[e.source]?.columns ?? []) names.add(c.name);
		}
		return [...names];
	}

	toEngine(): EngineWorkflow {
		return {
			nodes: this.nodes.map((n) => ({
				id: n.id,
				kind: n.data.kind,
				name: n.data.name,
				params: n.data.params,
				disabled: !!n.data.disabled,
			})),
			edges: this.edges.map((e) => ({ source: e.source, target: e.target, input: inputIndex(e.targetHandle) })),
		};
	}

	/** Exécute le workflow entier, ou seulement jusqu'au nœud `target`. */
	async run(target?: string): Promise<RunReport | null> {
		if (this.running) return null;
		this.running = true;
		const workflow = this.toEngine();
		this.runningIds = new Set(target ? this.ancestors(target) : workflow.nodes.map((n) => n.id));
		try {
			const { report, data } = await engine.run(workflow, target);
			// Une exécution partielle conserve les résultats des autres nœuds.
			this.results = target ? { ...this.results, ...report.results } : report.results;
			const merged = target ? { ...this.data, ...data } : { ...data };
			for (const [id, r] of Object.entries(report.results)) {
				if (r.status !== 'success') delete merged[id];
			}
			this.data = merged;
			this.lastRun = report;
			return report;
		} finally {
			this.running = false;
			this.runningIds = new Set();
		}
	}

	private ancestors(id: string): string[] {
		const seen = new Set<string>();
		const stack = [id];
		while (stack.length) {
			const cur = stack.pop()!;
			if (seen.has(cur)) continue;
			seen.add(cur);
			for (const e of this.edges) if (e.target === cur) stack.push(e.source);
		}
		return [...seen];
	}

	// --- Sérialisation ------------------------------------------------------

	serialize(): SavedWorkflow {
		return {
			version: 1,
			name: this.name,
			nodes: this.nodes.map((n) => ({ id: n.id, position: n.position, data: n.data })),
			edges: this.edges.map((e) => ({ id: e.id, source: e.source, target: e.target, targetHandle: e.targetHandle })),
		};
	}

	private apply(saved: SavedWorkflow) {
		this.name = saved.name;
		this.nodes = saved.nodes.map((n) => ({ id: n.id, type: 'step', position: n.position, data: n.data }));
		this.edges = saved.edges.map((e) => ({ ...e, sourceHandle: 'out', targetHandle: e.targetHandle ?? inputHandle(0) }));
		this.results = {};
		this.data = {};
		this.lastRun = null;
	}

	private snapshot(): string {
		const { name, nodes, edges } = this.serialize();
		return JSON.stringify({ name, nodes, edges });
	}

	// --- Synchronisation serveur --------------------------------------------

	/** Sélectionne un workspace : charge la liste de ses workflows (sans en ouvrir). */
	async loadWorkspace(workspaceId: string) {
		if (this.workspaceId === workspaceId) return this.refreshList();
		await this.flush();
		this.close();
		this.workspaceId = workspaceId;
		this.workflows = [];
		this.loading = true;
		try {
			this.workflows = await api.workflows(workspaceId);
		} finally {
			this.loading = false;
		}
	}

	/** Dernier workflow ouvert dans ce workspace (pour le raccourci « Éditeur »). */
	lastWorkflowId(): string | null {
		let last: string | null = null;
		try {
			last = localStorage.getItem(LAST_WORKFLOW_KEY);
		} catch {
			last = null;
		}
		return (this.workflows.find((w) => w.id === last) ?? this.workflows[0])?.id ?? null;
	}

	/** Ferme le workflow ouvert (après sauvegarde des modifications). */
	close() {
		clearTimeout(this.saveTimer);
		this.workflowId = null;
		this.apply({ version: 1, name: '', nodes: [], edges: [] });
		this.saveState = 'idle';
		this.saveError = null;
	}

	async refreshList() {
		if (this.workspaceId) this.workflows = await api.workflows(this.workspaceId);
	}

	private adopt(doc: WorkflowDoc) {
		const saved = sanitizeWorkflow({ ...(doc.definition as object), name: doc.name }, this.catalog);
		this.apply(saved);
		this.workflowId = doc.id;
		this.serverVersion = doc.version;
		this.savedSnapshot = this.snapshot();
		this.saveState = 'saved';
		this.saveError = null;
		try {
			localStorage.setItem(LAST_WORKFLOW_KEY, doc.id);
		} catch {
			// confort uniquement
		}
	}

	async openWorkflow(id: string) {
		if (!this.workspaceId || this.workflowId === id) return;
		await this.flush();
		this.loading = true;
		try {
			this.adopt(await api.workflow(this.workspaceId, id));
		} finally {
			this.loading = false;
		}
	}

	/** Crée un workflow dans le workspace courant et renvoie son identifiant (sans l'ouvrir). */
	async createWorkflow(name: string, definition?: SavedWorkflow): Promise<string> {
		if (!this.workspaceId) throw new Error('Aucun workspace sélectionné');
		const { nodes, edges } = definition ?? { nodes: [], edges: [] };
		const doc = await api.createWorkflow(this.workspaceId, name, { version: 1, nodes, edges });
		await this.refreshList();
		return doc.id;
	}

	/** Importe un fichier : validé puis créé comme nouveau workflow du workspace. */
	async importWorkflow(raw: unknown): Promise<string> {
		const clean = sanitizeWorkflow(raw, this.catalog);
		return this.createWorkflow(clean.name, clean);
	}

	/** Copie un workflow existant du workspace. */
	async duplicateWorkflow(id: string): Promise<string> {
		if (!this.workspaceId) throw new Error('Aucun workspace sélectionné');
		if (id === this.workflowId) await this.flush();
		const doc = await api.workflow(this.workspaceId, id);
		const clean = sanitizeWorkflow({ ...(doc.definition as object), name: doc.name }, this.catalog);
		return this.createWorkflow(`${doc.name} (copie)`.slice(0, 120), clean);
	}

	async deleteWorkflow(id: string) {
		if (!this.workspaceId) return;
		if (id === this.workflowId) this.close();
		await api.deleteWorkflow(this.workspaceId, id);
		await this.refreshList();
	}

	/** Version exportable (en-têtes sensibles masqués) d'un workflow du workspace. */
	async exportable(id: string): Promise<SavedWorkflow> {
		if (id === this.workflowId) return this.serialize();
		const doc = await api.workflow(this.workspaceId!, id);
		return sanitizeWorkflow({ ...(doc.definition as object), name: doc.name }, this.catalog);
	}

	async reloadCurrent() {
		if (this.workflowId) {
			this.savedSnapshot = '';
			this.adopt(await api.workflow(this.workspaceId!, this.workflowId));
		}
	}

	/** Programme une sauvegarde si le contenu a changé depuis la dernière. */
	scheduleSave() {
		if (!this.workflowId || this.saveState === 'conflict') return;
		if (this.snapshot() === this.savedSnapshot) return;
		clearTimeout(this.saveTimer);
		this.saveTimer = setTimeout(() => void this.save(), SAVE_DELAY_MS);
	}

	/** Sauvegarde immédiate des modifications en attente. */
	async flush(keepalive = false) {
		clearTimeout(this.saveTimer);
		if (this.saving) await this.saving;
		if (this.workflowId && this.saveState !== 'conflict' && this.snapshot() !== this.savedSnapshot) {
			await this.save(keepalive);
		}
	}

	private async save(keepalive = false) {
		if (!this.workspaceId || !this.workflowId) return;
		if (this.saving) {
			await this.saving;
			return this.scheduleSave();
		}
		const snapshot = this.snapshot();
		if (snapshot === this.savedSnapshot) return;
		const { name, nodes, edges } = this.serialize();
		const workflowId = this.workflowId;
		this.saveState = 'saving';
		this.saving = (async () => {
			try {
				const doc = await api.saveWorkflow(
					this.workspaceId!,
					workflowId,
					name.trim() || 'Sans titre',
					{ version: 1, nodes, edges },
					this.serverVersion,
					keepalive && snapshot.length < 60_000,
				);
				if (this.workflowId !== workflowId) return;
				this.serverVersion = doc.version;
				this.savedSnapshot = snapshot;
				this.saveState = 'saved';
				this.saveError = null;
				const summary = this.workflows.find((w) => w.id === workflowId);
				if (summary && summary.name !== doc.name) {
					this.workflows = this.workflows.map((w) => (w.id === workflowId ? { ...w, name: doc.name } : w));
				}
			} catch (e) {
				this.saveState = e instanceof ApiError && e.status === 409 ? 'conflict' : 'error';
				this.saveError = errorMessage(e);
			} finally {
				this.saving = null;
			}
		})();
		await this.saving;
	}

	reset() {
		this.close();
		this.workspaceId = null;
		this.workflows = [];
	}

	example(): SavedWorkflow {
		const node = (id: string, kind: string, x: number, y: number, params: Record<string, unknown> = {}, name?: string): SavedWorkflow['nodes'][number] => {
			const spec = this.spec(kind)!;
			return { id, position: { x, y }, data: { kind, name: name ?? spec.label, params: { ...this.defaults(spec), ...params } } };
		};
		const edge = (source: string, target: string, input = 0) => ({
			id: `e-${source}-${target}-${input}`,
			source,
			target,
			targetHandle: inputHandle(input),
		});
		return {
			version: 1,
			name: 'Exemple : ventes',
			nodes: [
				node('ventes', 'manual', 0, 120),
				node('total', 'formula', 300, 40),
				node('filtre', 'filter', 600, 40, { condition: 'total > 50' }),
				node('tri', 'sort', 900, 40, { by: 'total desc' }),
				node('stats', 'aggregate', 300, 240),
				node(
					'cat',
					'manual',
					300,
					420,
					{ data: '[\n  {"categorie": "info", "rayon": "Informatique"},\n  {"categorie": "livres", "rayon": "Librairie"}\n]' },
					'Rayons',
				),
				node('jointure', 'merge', 600, 300, { mode: 'jointure gauche', left_key: 'categorie' }),
			],
			edges: [
				edge('ventes', 'total'),
				edge('total', 'filtre'),
				edge('filtre', 'tri'),
				edge('ventes', 'stats'),
				edge('stats', 'jointure', 0),
				edge('cat', 'jointure', 1),
			],
		};
	}
}

export const wf = new WorkflowState();
