// Session utilisateur : compte connecté et workspaces accessibles.

import { ApiError, api, setUnauthorizedHandler, type Me, type User, type WorkspaceSummary } from './api';

const LAST_WORKSPACE_KEY = 'mini8n.lastWorkspace';

function remember(key: string, value: string | null) {
	try {
		if (value) localStorage.setItem(key, value);
		else localStorage.removeItem(key);
	} catch {
		// Stockage indisponible : simple confort, on ignore.
	}
}

function recall(key: string): string | null {
	try {
		return localStorage.getItem(key);
	} catch {
		return null;
	}
}

class Session {
	status = $state<'loading' | 'anonymous' | 'ready' | 'offline'>('loading');
	user = $state.raw<User | null>(null);
	workspaces = $state.raw<WorkspaceSummary[]>([]);
	currentId = $state<string | null>(null);
	allowRegistration = $state(true);
	/** Message affiché sur l'écran de connexion (ex. session expirée). */
	notice = $state<string | null>(null);

	current = $derived(this.workspaces.find((w) => w.id === this.currentId) ?? null);

	constructor() {
		setUnauthorizedHandler(() => this.expire());
	}

	async init() {
		try {
			this.apply(await api.me());
		} catch (e) {
			if (e instanceof ApiError && e.status === 401) {
				this.status = 'anonymous';
				try {
					this.allowRegistration = (await api.config()).allowRegistration;
				} catch {
					// valeur par défaut
				}
			} else {
				this.status = 'offline';
			}
		}
	}

	private apply(me: Me) {
		this.user = me.user;
		this.workspaces = me.workspaces;
		this.allowRegistration = me.allowRegistration;
		const last = recall(LAST_WORKSPACE_KEY);
		this.currentId = me.workspaces.find((w) => w.id === last)?.id ?? me.workspaces[0]?.id ?? null;
		this.notice = null;
		this.status = 'ready';
	}

	async register(email: string, name: string, password: string) {
		const me = await api.register(email, name, password);
		await this.ensureCookie();
		this.apply(me);
	}

	async login(email: string, password: string) {
		const me = await api.login(email, password);
		await this.ensureCookie();
		this.apply(me);
	}

	/** Vérifie que le navigateur a bien conservé le cookie de session (HttpOnly, donc invisible ici). */
	private async ensureCookie() {
		try {
			await api.me();
		} catch (e) {
			if (e instanceof ApiError && e.status === 401) {
				throw new Error(
					"Identifiants acceptés, mais votre navigateur a refusé le cookie de session. " +
						"Ouvrez le site en HTTPS, ou vérifiez que les cookies ne sont pas bloqués pour ce site.",
				);
			}
			throw e;
		}
	}

	async logout(everywhere = false) {
		try {
			await (everywhere ? api.logoutAll() : api.logout());
		} finally {
			this.reset();
		}
	}

	private reset() {
		this.user = null;
		this.workspaces = [];
		this.currentId = null;
		this.status = 'anonymous';
	}

	expire() {
		if (this.status !== 'ready') return;
		this.reset();
		this.notice = 'Votre session a expiré. Reconnectez-vous.';
	}

	async refreshWorkspaces() {
		const me = await api.me();
		this.workspaces = me.workspaces;
		if (!this.workspaces.some((w) => w.id === this.currentId)) this.select(this.workspaces[0]?.id ?? null);
	}

	async createWorkspace(name: string) {
		const ws = await api.createWorkspace(name);
		this.workspaces = [...this.workspaces, ws];
		this.select(ws.id);
		return ws;
	}

	select(id: string | null) {
		this.currentId = id;
		remember(LAST_WORKSPACE_KEY, id);
	}
}

export const session = new Session();
