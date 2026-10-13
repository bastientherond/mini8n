// Collaboration en direct : présence des autres membres sur le canevas (curseur, sélection,
// déplacements, nœud en cours de paramétrage) et réception des sauvegardes, par WebSocket.
// Le serveur relaie et mémorise la présence (voir crates/server/src/live.rs).

import type { WorkflowDoc } from './api';
import type { NodeResult } from './engine/types';

export interface Point {
	x: number;
	y: number;
}

export interface Peer {
	/** Connexion (un même utilisateur peut avoir plusieurs onglets). */
	id: number;
	user: string;
	name: string;
	color: number;
	/** IA connectée par MCP (Claude…). */
	agent?: boolean;
	cursor: Point | null;
	selection: string[];
	focus: string | null;
	/** Nœuds qu'il déplace, ou vient de déplacer en attendant sa sauvegarde. */
	drag: Record<string, Point>;
	dragging: boolean;
	/** Exécution en cours dans son navigateur. */
	run: RunState | null;
}

/** État d'une exécution, partagé en direct avec les autres membres. */
export interface RunState {
	/** Nœuds pas encore terminés. */
	pending: string[];
	/** Nœud en cours d'exécution. */
	active: string | null;
	/** Résultats des nœuds terminés (sans les données). */
	results: Record<string, NodeResult>;
	/** Exécution terminée : bilan. */
	done: boolean;
	ok?: boolean;
	error?: string;
	durationMs?: number;
}

export interface LiveHandlers {
	/** Nouvelle version enregistrée (par n'importe qui, y compris soi-même). */
	onsaved: (doc: WorkflowDoc) => Promise<void> | void;
	onactive: (active: boolean) => void;
	/** Exécution d'un autre membre : une étape, la fin (`done`), ou `null` si abandonnée. */
	onrun: (peer: Peer, run: RunState | null) => void;
	/** Reconnecté après une coupure : des sauvegardes ont pu être manquées. */
	onresync: () => void;
	/** Accès retiré ou workflow supprimé : pas de reconnexion. */
	ongone: (reason: 'forbidden' | 'deleted' | 'full') => void;
}

const PALETTE = ['#e11d48', '#2563eb', '#059669', '#d97706', '#7c3aed', '#0891b2', '#c026d3', '#65a30d'];
export const peerColor = (index: number) => PALETTE[index % PALETTE.length];

/** Intervalle minimal entre deux envois de curseur ou de déplacement. */
const SEND_EVERY_MS = 40;
/** Intervalle minimal entre deux étapes d'exécution envoyées. */
const RUN_EVERY_MS = 80;
/** Position d'un nœud déplacé par un autre, conservée jusqu'à sa sauvegarde (au plus). */
const DRAG_HOLD_MS = 10_000;

const round = (p: Point): Point => ({ x: Math.round(p.x), y: Math.round(p.y) });

/** Envoi limité à un message par intervalle ; le dernier état n'est jamais perdu. */
class Throttle<T> {
	private timer: ReturnType<typeof setTimeout> | undefined;
	private pending: { value: T } | null = null;

	constructor(
		private readonly emit: (value: T) => void,
		private readonly every = SEND_EVERY_MS,
	) {}

	push(value: T) {
		if (this.timer) {
			this.pending = { value };
			return;
		}
		this.emit(value);
		this.timer = setTimeout(() => this.tick(), this.every);
	}

	/** Envoie tout de suite, en remplaçant un envoi en attente. */
	now(value: T) {
		this.cancel();
		this.emit(value);
	}

	cancel() {
		clearTimeout(this.timer);
		this.timer = undefined;
		this.pending = null;
	}

	private tick() {
		this.timer = undefined;
		const p = this.pending;
		this.pending = null;
		if (p) this.push(p.value);
	}
}

type ServerMessage =
	| { t: 'welcome'; you: number; color: number; peers: Peer[] }
	| { t: 'join'; peer: Peer }
	| { t: 'leave'; id: number }
	| { t: 'cursor'; id: number; at: Point | null }
	| { t: 'select'; id: number; nodes: string[] }
	| { t: 'focus'; id: number; node: string | null }
	| { t: 'drag'; id: number; positions: Record<string, Point>; done: boolean }
	| { t: 'run'; id: number; run: RunState | null }
	| { t: 'saved'; by: string; doc: WorkflowDoc }
	| { t: 'active'; active: boolean };

class Live {
	status = $state<'off' | 'connecting' | 'online' | 'offline'>('off');
	/** Les autres connexions au workflow ouvert. */
	peers = $state.raw<Peer[]>([]);
	/** Couleur attribuée à cette connexion. */
	color = $state<number | null>(null);

	/** Positions des nœuds déplacés par les autres, affichées avant leur sauvegarde. */
	ghosts = $derived.by(() => {
		const out = new Map<string, Point>();
		for (const p of this.peers) for (const [id, at] of Object.entries(p.drag)) out.set(id, at);
		return out;
	});

	private socket: WebSocket | null = null;
	private room: { ws: string; wf: string } | null = null;
	private handlers: LiveHandlers | null = null;
	private attempt = 0;
	private welcomed = false;
	private retryTimer: ReturnType<typeof setTimeout> | undefined;
	private holds = new Map<number, ReturnType<typeof setTimeout>>();
	/** État local, renvoyé après chaque reconnexion. */
	private mine: { selection: string[]; focus: string | null } = { selection: [], focus: null };

	private cursorOut = new Throttle<Point | null>((at) => this.send({ t: 'cursor', at }));
	private dragOut = new Throttle<{ positions: Record<string, Point>; done: boolean }>((d) => this.send({ t: 'drag', ...d }));
	private runOut = new Throttle<RunState | null>((run) => this.send({ t: 'run', run }), RUN_EVERY_MS);
	/** Exécution locale en cours, renvoyée après une reconnexion. */
	private myRun: RunState | null = null;

	connect(ws: string, wf: string, handlers: LiveHandlers) {
		this.handlers = handlers;
		if (this.room?.ws === ws && this.room.wf === wf) return;
		this.disconnect();
		this.room = { ws, wf };
		this.attempt = 0;
		this.welcomed = false;
		this.open();
	}

	disconnect() {
		this.room = null;
		clearTimeout(this.retryTimer);
		const socket = this.socket;
		this.socket = null;
		socket?.close(1000);
		this.cursorOut.cancel();
		this.dragOut.cancel();
		this.runOut.cancel();
		this.myRun = null;
		this.mine = { selection: [], focus: null };
		this.reset('off');
	}

	private reset(status: Live['status']) {
		// Les exécutions des autres ne sont plus suivies.
		for (const p of this.peers) if (p.run) this.handlers?.onrun(p, null);
		for (const t of this.holds.values()) clearTimeout(t);
		this.holds.clear();
		this.peers = [];
		this.color = null;
		this.status = status;
	}

	private open() {
		const room = this.room;
		if (!room) return;
		const scheme = location.protocol === 'https:' ? 'wss:' : 'ws:';
		const path = `/api/workspaces/${encodeURIComponent(room.ws)}/workflows/${encodeURIComponent(room.wf)}/live`;
		this.status = 'connecting';
		let socket: WebSocket;
		try {
			socket = new WebSocket(`${scheme}//${location.host}${path}`);
		} catch {
			this.retry();
			return;
		}
		this.socket = socket;
		socket.onmessage = (e) => {
			if (this.socket !== socket || typeof e.data !== 'string') return;
			try {
				this.receive(JSON.parse(e.data) as ServerMessage);
			} catch {
				// message illisible : ignoré
			}
		};
		socket.onclose = (e) => {
			if (this.socket !== socket) return;
			this.socket = null;
			const gone = ({ 4403: 'forbidden', 4410: 'deleted', 4429: 'full' } as const)[e.code as 4403 | 4410 | 4429];
			if (gone) {
				this.room = null;
				this.reset('off');
				this.handlers?.ongone(gone);
				return;
			}
			this.reset('offline');
			this.retry();
		};
	}

	/** Reconnexion avec attente croissante (1 s, 2 s, 4 s… 30 s au plus). */
	private retry() {
		clearTimeout(this.retryTimer);
		const delay = Math.min(30_000, 1000 * 2 ** this.attempt++) * (0.75 + Math.random() / 2);
		this.retryTimer = setTimeout(() => this.open(), delay);
	}

	private send(msg: object) {
		if (this.socket?.readyState === WebSocket.OPEN) this.socket.send(JSON.stringify(msg));
	}

	private update(id: number, patch: Partial<Peer>) {
		this.peers = this.peers.map((p) => (p.id === id ? { ...p, ...patch } : p));
	}

	private receive(msg: ServerMessage) {
		switch (msg.t) {
			case 'welcome': {
				this.peers = msg.peers.map((p) => ({ ...p, dragging: Object.keys(p.drag).length > 0 }));
				this.color = msg.color;
				this.status = 'online';
				this.attempt = 0;
				this.send({ t: 'select', nodes: this.mine.selection });
				this.send({ t: 'focus', node: this.mine.focus });
				if (this.myRun) this.send({ t: 'run', run: this.myRun });
				for (const p of this.peers) if (p.run) this.handlers?.onrun(p, p.run);
				if (this.welcomed) this.handlers?.onresync();
				this.welcomed = true;
				break;
			}
			case 'join':
				this.peers = [...this.peers.filter((p) => p.id !== msg.peer.id), { ...msg.peer, dragging: false }];
				break;
			case 'leave': {
				clearTimeout(this.holds.get(msg.id));
				const gone = this.peers.find((p) => p.id === msg.id);
				this.peers = this.peers.filter((p) => p.id !== msg.id);
				if (gone?.run) this.handlers?.onrun(gone, null);
				break;
			}
			case 'run': {
				const run = msg.run?.done ? null : msg.run;
				this.update(msg.id, { run });
				const peer = this.peers.find((p) => p.id === msg.id);
				if (peer) this.handlers?.onrun(peer, msg.run);
				break;
			}
			case 'cursor':
				this.update(msg.id, { cursor: msg.at });
				break;
			case 'select':
				this.update(msg.id, { selection: msg.nodes });
				break;
			case 'focus':
				this.update(msg.id, { focus: msg.node });
				break;
			case 'drag': {
				clearTimeout(this.holds.get(msg.id));
				this.update(msg.id, { drag: msg.positions, dragging: !msg.done });
				if (msg.done) {
					this.holds.set(
						msg.id,
						setTimeout(() => this.update(msg.id, { drag: {} }), DRAG_HOLD_MS),
					);
				}
				break;
			}
			case 'saved': {
				const by = msg.by;
				void Promise.resolve(this.handlers?.onsaved(msg.doc)).finally(() => {
					// Ses positions font maintenant partie du workflow.
					this.peers = this.peers.map((p) => (p.user === by && !p.dragging && Object.keys(p.drag).length ? { ...p, drag: {} } : p));
				});
				break;
			}
			case 'active':
				this.handlers?.onactive(msg.active);
				break;
		}
	}

	// --- Envois ------------------------------------------------------------------

	/** Position du pointeur sur le canevas (`null` : il en est sorti). */
	cursor(at: Point | null) {
		if (at) this.cursorOut.push(round(at));
		else this.cursorOut.now(null);
	}

	select(nodes: string[]) {
		if (nodes.length === this.mine.selection.length && nodes.every((id, i) => id === this.mine.selection[i])) return;
		this.mine.selection = nodes;
		this.send({ t: 'select', nodes });
	}

	focus(node: string | null) {
		if (node === this.mine.focus) return;
		this.mine.focus = node;
		this.send({ t: 'focus', node });
	}

	/** État de l'exécution locale (`done` à la fin), diffusé aux autres membres. */
	run(state: RunState) {
		this.myRun = state.done ? null : state;
		if (state.done) this.runOut.now(state);
		else this.runOut.push(state);
	}

	/** Nœuds en cours de déplacement ; `done` à la fin du geste. */
	drag(positions: Map<string, Point>, done = false) {
		const payload = { positions: Object.fromEntries([...positions].map(([id, p]) => [id, round(p)])), done };
		if (done) this.dragOut.now(payload);
		else this.dragOut.push(payload);
	}
}

export const live = new Live();
