// État de l'éditeur : graphe (nœuds et liens du canevas), catalogue des nœuds,
// résultats d'exécution et synchronisation avec le serveur.

import { toast } from 'svelte-sonner';
import { ApiError, api, errorMessage, type TriggerInfo, type WorkflowDoc, type WorkflowSummary } from './api';
import { engine } from './engine/client';
import { newToken, toEngineWorkflow } from './engine/convert';
import type { EngineWorkflow, NodeResult, NodeSpec, RunReport } from './engine/types';
import { removeNodesKeepingLinks } from './canvas/graph';
import { sanitizeWorkflow } from './sanitize';

export type StepData = {
	kind: string;
	name: string;
	params: Record<string, unknown>;
	disabled?: boolean;
};

export interface XYPosition {
	x: number;
	y: number;
}

export interface StepNode {
	id: string;
	type?: 'step';
	position: XYPosition;
	data: StepData;
	selected?: boolean;
}

export interface Edge {
	id: string;
	source: string;
	target: string;
	sourceHandle?: string | null;
	/** Entrée de la cible : `in-0`, `in-1`… */
	targetHandle?: string | null;
	selected?: boolean;
}

/** État du graphe mémorisé pour annuler / rétablir. */
interface GraphState {
	nodes: StepNode[];
	edges: Edge[];
}

const HISTORY = 60;

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
	/** Workflow actif : ses déclencheurs automatiques sont en service sur le serveur. */
	active = $state(false);
	triggers = $state.raw<TriggerInfo[]>([]);
	activating = $state(false);
	saveError = $state<string | null>(null);
	loading = $state(false);
	private serverVersion = 0;
	private wsLoad: { id: string; promise: Promise<void> } | null = null;
	private savedSnapshot = '';
	private saveTimer: ReturnType<typeof setTimeout> | undefined;
	private saving: Promise<void> | null = null;

	selected = $derived(this.nodes.find((n) => n.selected) ?? null);
	/** Nœud affiché dans la vue détaillée (modale de paramétrage). */
	openNodeId = $state<string | null>(null);
	openNode = $derived(this.openNodeId ? (this.nodes.find((n) => n.id === this.openNodeId) ?? null) : null);

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

	/** Génère les jetons d'URL publique manquants (webhook, formulaire). */
	withTokens(spec: NodeSpec, params: Record<string, unknown>): Record<string, unknown> {
		const out = { ...params };
		for (const p of spec.params) if (p.type === 'token' && !out[p.name]) out[p.name] = newToken();
		return out;
	}

	isTrigger(kind: string): boolean {
		return this.spec(kind)?.category === 'trigger';
	}

	/** Nouveau nœud (nom unique, paramètres par défaut), sans l'ajouter au graphe. */
	private makeNode(spec: NodeSpec, position: XYPosition, select: boolean): StepNode {
		const taken = new Set(this.nodes.map((n) => n.data.name));
		let name = spec.label;
		for (let i = 2; taken.has(name); i++) name = `${spec.label} ${i}`;
		return {
			id: `${spec.kind}-${uid()}`,
			type: 'step',
			position,
			selected: select,
			data: { kind: spec.kind, name, params: this.withTokens(spec, this.defaults(spec)) },
		};
	}

	/**
	 * Insère un nœud sur un lien : source → nouveau → cible. `moved` déplace d'autres
	 * nœuds pour lui faire de la place. Retourne l'identifiant du nœud créé.
	 */
	insertOnEdge(edgeId: string, kind: string, position: XYPosition, moved: Map<string, XYPosition> = new Map()) {
		const edge = this.edges.find((e) => e.id === edgeId);
		const spec = this.spec(kind);
		if (!edge || !spec || spec.inputs.length === 0) return;
		this.checkpoint();
		const node = this.makeNode(spec, position, true);
		this.nodes = [
			...this.nodes.map((n) => ({ ...n, selected: false, position: moved.get(n.id) ?? n.position })),
			node,
		];
		this.edges = [
			...this.edges.filter((e) => e.id !== edgeId).map((e) => (e.selected ? { ...e, selected: false } : e)),
			{ id: `e-${uid()}`, source: edge.source, target: node.id, sourceHandle: 'out', targetHandle: inputHandle(0) },
			{ id: `e-${uid()}`, source: node.id, target: edge.target, sourceHandle: 'out', targetHandle: edge.targetHandle ?? inputHandle(0) },
		];
		return node.id;
	}

	addNode(kind: string, position: XYPosition, select = true) {
		const spec = this.spec(kind);
		if (!spec) return;
		const node = this.makeNode(spec, position, select);
		this.checkpoint();
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

	/** Lien recréé à la suppression d'un nœud intermédiaire. */
	private bridgeEdge = (source: string, target: string, targetHandle: string): Edge => ({
		id: `e-${uid()}`,
		source,
		target,
		sourceHandle: 'out',
		targetHandle,
	});

	/** Supprime un nœud ; ses voisins restent reliés (a → b → c devient a → c). */
	removeNode(id: string) {
		this.checkpoint();
		this.nodes = this.nodes.filter((n) => n.id !== id);
		this.edges = removeNodesKeepingLinks(this.edges, [id], this.bridgeEdge);
	}

	// --- Graphe : sélection, liens, annulation ------------------------------

	private undoStack: GraphState[] = [];
	private redoStack: GraphState[] = [];
	canUndo = $state(false);
	canRedo = $state(false);

	private graphState(): GraphState {
		return {
			nodes: this.nodes.map((n) => ({ ...n, selected: false })),
			edges: this.edges.map((e) => ({ ...e, selected: false })),
		};
	}

	/** Mémorise l'état actuel avant une modification du graphe. */
	checkpoint() {
		this.undoStack.push(this.graphState());
		if (this.undoStack.length > HISTORY) this.undoStack.shift();
		this.redoStack = [];
		this.canUndo = true;
		this.canRedo = false;
	}

	private restore(from: GraphState[], to: GraphState[]) {
		const state = from.pop();
		if (!state) return;
		to.push(this.graphState());
		this.nodes = state.nodes;
		this.edges = state.edges;
		if (this.openNodeId && !state.nodes.some((n) => n.id === this.openNodeId)) this.openNodeId = null;
		this.canUndo = this.undoStack.length > 0;
		this.canRedo = this.redoStack.length > 0;
	}

	undo() {
		this.restore(this.undoStack, this.redoStack);
	}

	redo() {
		this.restore(this.redoStack, this.undoStack);
	}

	private resetHistory() {
		this.undoStack = [];
		this.redoStack = [];
		this.canUndo = false;
		this.canRedo = false;
	}

	/** Sélection : nœuds et liens (les autres sont désélectionnés). */
	setSelection(nodeIds: Iterable<string>, edgeIds: Iterable<string> = []) {
		const ns = new Set(nodeIds);
		const es = new Set(edgeIds);
		this.nodes = this.nodes.map((n) => (!!n.selected === ns.has(n.id) ? n : { ...n, selected: ns.has(n.id) }));
		this.edges = this.edges.map((e) => (!!e.selected === es.has(e.id) ? e : { ...e, selected: es.has(e.id) }));
	}

	/** Déplace des nœuds (positions absolues). */
	moveNodes(positions: Map<string, XYPosition>) {
		this.nodes = this.nodes.map((n) => {
			const p = positions.get(n.id);
			return p && (p.x !== n.position.x || p.y !== n.position.y) ? { ...n, position: p } : n;
		});
	}

	/** Le lien source → cible créerait-il une boucle ? */
	private createsCycle(source: string, target: string): boolean {
		const stack = [target];
		const seen = new Set<string>();
		while (stack.length) {
			const id = stack.pop()!;
			if (id === source) return true;
			if (seen.has(id)) continue;
			seen.add(id);
			for (const e of this.edges) if (e.source === id) stack.push(e.target);
		}
		return false;
	}

	/** Relie la sortie de `source` à l'entrée `input` de `target`. Retourne une erreur éventuelle. */
	connect(source: string, target: string, input = 0): string | null {
		if (source === target) return 'Un nœud ne peut pas être relié à lui-même.';
		const spec = this.spec(this.nodes.find((n) => n.id === target)?.data.kind ?? '');
		if (!spec || spec.inputs.length === 0) return 'Ce nœud n’a pas d’entrée.';
		if (input >= spec.inputs.length) input = 0;
		const handle = inputHandle(input);
		if (this.edges.some((e) => e.source === source && e.target === target && (e.targetHandle ?? inputHandle(0)) === handle)) {
			return 'Ces nœuds sont déjà reliés.';
		}
		if (this.createsCycle(source, target)) return 'Ce lien créerait une boucle.';
		this.checkpoint();
		this.edges = [
			...this.edges.map((e) => (e.selected ? { ...e, selected: false } : e)),
			{ id: `e-${uid()}`, source, target, sourceHandle: 'out', targetHandle: handle },
		];
		return null;
	}

	/** Supprime les nœuds et liens sélectionnés. */
	deleteSelection(): number {
		const nodes = new Set(this.nodes.filter((n) => n.selected).map((n) => n.id));
		const edges = this.edges.filter((e) => e.selected || nodes.has(e.source) || nodes.has(e.target));
		if (nodes.size === 0 && edges.length === 0) return 0;
		this.checkpoint();
		// Liens sélectionnés supprimés d'abord ; puis les nœuds, en reliant leurs voisins.
		const selectedEdges = this.edges.filter((e) => e.selected && !nodes.has(e.source) && !nodes.has(e.target));
		const gone = new Set(selectedEdges.map((e) => e.id));
		this.nodes = this.nodes.filter((n) => !nodes.has(n.id));
		this.edges = removeNodesKeepingLinks(
			this.edges.filter((e) => !gone.has(e.id)),
			nodes,
			this.bridgeEdge,
		);
		return nodes.size + gone.size;
	}

	removeEdge(id: string) {
		if (!this.edges.some((e) => e.id === id)) return;
		this.checkpoint();
		this.edges = this.edges.filter((e) => e.id !== id);
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
		return toEngineWorkflow({ nodes: this.nodes, edges: this.edges });
	}

	/** Exécute le workflow entier, ou seulement jusqu'au nœud `target`. */
	async run(target?: string): Promise<RunReport | null> {
		if (this.running) return null;
		this.running = true;
		const workflow = this.toEngine();
		this.runningIds = new Set(target ? this.ancestors(target) : workflow.nodes.map((n) => n.id));
		try {
			const { report, data } = await engine.run(workflow, target, this.workspaceId ?? undefined);
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
		this.resetHistory();
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
		// Un chargement en cours pour ce workspace est partagé (éditeur ouvert par lien direct).
		if (this.wsLoad?.id === workspaceId) return this.wsLoad.promise;
		if (this.workspaceId === workspaceId) return this.refreshList();
		const promise = (async () => {
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
		})();
		this.wsLoad = { id: workspaceId, promise };
		try {
			await promise;
		} finally {
			if (this.wsLoad?.promise === promise) this.wsLoad = null;
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
		this.openNodeId = null;
		this.active = false;
		this.triggers = [];
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
		this.active = doc.active;
		this.triggers = [];
		if (doc.active) void this.refreshTriggers();
		this.savedSnapshot = this.snapshot();
		this.saveState = 'saved';
		this.saveError = null;
		try {
			localStorage.setItem(LAST_WORKFLOW_KEY, doc.id);
		} catch {
			// confort uniquement
		}
	}

	async openWorkflow(id: string, workspaceId: string) {
		if (this.workspaceId !== workspaceId || this.wsLoad) await this.loadWorkspace(workspaceId);
		if (this.workflowId === id) return;
		await this.flush();
		this.loading = true;
		try {
			this.adopt(await api.workflow(workspaceId, id));
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
		const clean = this.freshTokens(sanitizeWorkflow(raw, this.catalog));
		return this.createWorkflow(clean.name, clean);
	}

	/** Nouvelles URL publiques pour une copie (une URL ne peut servir qu'un workflow). */
	private freshTokens(saved: SavedWorkflow): SavedWorkflow {
		const nodes = saved.nodes.map((n) => {
			const spec = this.spec(n.data.kind);
			if (!spec?.params.some((p) => p.type === 'token')) return n;
			const params = { ...n.data.params };
			for (const p of spec.params) if (p.type === 'token') params[p.name] = newToken();
			return { ...n, data: { ...n.data, params } };
		});
		return { ...saved, nodes };
	}

	/** Copie un workflow existant du workspace. */
	async duplicateWorkflow(id: string): Promise<string> {
		if (!this.workspaceId) throw new Error('Aucun workspace sélectionné');
		if (id === this.workflowId) await this.flush();
		const doc = await api.workflow(this.workspaceId, id);
		const clean = this.freshTokens(sanitizeWorkflow({ ...(doc.definition as object), name: doc.name }, this.catalog));
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
				if (doc.deactivated) {
					this.active = false;
					this.triggers = [];
					toast.warning(`Workflow désactivé : ${doc.deactivated}`);
				} else if (doc.active) {
					void this.refreshTriggers();
				}
				this.setSummary(workflowId, { active: doc.active, version: doc.version });
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

	private setSummary(id: string, patch: Partial<WorkflowSummary>) {
		this.workflows = this.workflows.map((w) => (w.id === id ? { ...w, ...patch } : w));
	}

	async refreshTriggers() {
		if (!this.workspaceId || !this.workflowId) return;
		const id = this.workflowId;
		try {
			const state = await api.triggers(this.workspaceId, id);
			if (this.workflowId !== id) return;
			this.active = state.active;
			this.triggers = state.triggers;
		} catch {
			// informatif uniquement
		}
	}

	/** Active / désactive les déclencheurs automatiques (après sauvegarde des modifications). */
	async setActive(active: boolean) {
		if (!this.workspaceId || !this.workflowId || this.activating) return;
		this.activating = true;
		try {
			await this.flush();
			if (this.saveState === 'error' || this.saveState === 'conflict') {
				throw new Error("Enregistrez d'abord le workflow");
			}
			const state = await api.setActive(this.workspaceId, this.workflowId, active);
			this.active = state.active;
			this.triggers = state.triggers;
			this.setSummary(this.workflowId, { active: state.active });
			toast.success(state.active ? 'Workflow activé : les déclencheurs sont en service' : 'Workflow désactivé');
		} catch (e) {
			toast.error(errorMessage(e));
		} finally {
			this.activating = false;
		}
	}

	/** Exécute le workflow sur le serveur (enregistré dans l'historique). */
	async runOnServer() {
		if (!this.workspaceId || !this.workflowId) return null;
		await this.flush();
		return api.runOnServer(this.workspaceId, this.workflowId);
	}

	trigger(nodeId: string): TriggerInfo | undefined {
		return this.triggers.find((t) => t.nodeId === nodeId);
	}

	reset() {
		this.close();
		this.workspaceId = null;
		this.workflows = [];
	}

	example(): SavedWorkflow {
		const node = (id: string, kind: string, x: number, y: number, params: Record<string, unknown> = {}, name?: string): SavedWorkflow['nodes'][number] => {
			const spec = this.spec(kind)!;
			return { id, position: { x, y }, data: { kind, name: name ?? spec.label, params: this.withTokens(spec, { ...this.defaults(spec), ...params }) } };
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
