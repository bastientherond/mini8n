// Client de l'API mini8n (même origine, cookie de session HttpOnly).

export interface User {
	id: string;
	email: string;
	name: string;
}

export type Role = 'owner' | 'member';

export interface WorkspaceSummary {
	id: string;
	name: string;
	role: Role;
	memberCount: number;
	createdAt: string;
}

export interface Me {
	user: User;
	workspaces: WorkspaceSummary[];
	allowRegistration: boolean;
}

export interface Member {
	userId: string;
	email: string;
	name: string;
	role: Role;
	createdAt: string;
}

export interface WorkflowSummary {
	id: string;
	name: string;
	version: number;
	updatedAt: string;
	updatedBy: string | null;
}

export interface WorkflowDoc {
	id: string;
	name: string;
	version: number;
	definition: unknown;
	updatedAt: string;
}

export class ApiError extends Error {
	constructor(
		public status: number,
		message: string,
	) {
		super(message);
	}
}

let onUnauthorized: (() => void) | null = null;

/** Appelé quand une requête authentifiée reçoit 401 (session expirée ou révoquée). */
export function setUnauthorizedHandler(fn: () => void) {
	onUnauthorized = fn;
}

async function request<T>(method: string, path: string, body?: unknown, opts: { keepalive?: boolean } = {}): Promise<T> {
	let res: Response;
	try {
		res = await fetch(`/api${path}`, {
			method,
			credentials: 'same-origin',
			cache: 'no-store',
			keepalive: opts.keepalive,
			headers: body === undefined ? {} : { 'Content-Type': 'application/json' },
			body: body === undefined ? undefined : JSON.stringify(body),
		});
	} catch {
		throw new ApiError(0, 'Serveur injoignable');
	}
	if (res.status === 204) return undefined as T;
	const text = await res.text();
	let data: unknown = null;
	try {
		data = text ? JSON.parse(text) : null;
	} catch {
		data = null;
	}
	if (!res.ok) {
		const message =
			(data as { error?: string } | null)?.error ??
			(res.status === 429 ? 'Trop de requêtes, réessayez plus tard' : `Erreur ${res.status}`);
		if (res.status === 401 && !path.startsWith('/auth/')) onUnauthorized?.();
		throw new ApiError(res.status, message);
	}
	return data as T;
}

const ws = (id: string) => `/workspaces/${encodeURIComponent(id)}`;

export const api = {
	config: () => request<{ allowRegistration: boolean }>('GET', '/auth/config'),
	me: () => request<Me>('GET', '/auth/me'),
	register: (email: string, name: string, password: string) =>
		request<Me>('POST', '/auth/register', { email, name, password }),
	login: (email: string, password: string) => request<Me>('POST', '/auth/login', { email, password }),
	logout: () => request<void>('POST', '/auth/logout'),
	logoutAll: () => request<void>('POST', '/auth/logout-all'),
	changePassword: (currentPassword: string, newPassword: string) =>
		request<void>('POST', '/auth/password', { currentPassword, newPassword }),

	createWorkspace: (name: string) => request<WorkspaceSummary>('POST', '/workspaces', { name }),
	renameWorkspace: (id: string, name: string) => request<WorkspaceSummary>('PATCH', ws(id), { name }),
	deleteWorkspace: (id: string) => request<void>('DELETE', ws(id)),

	members: (id: string) => request<Member[]>('GET', `${ws(id)}/members`),
	addMember: (id: string, email: string, role: Role) =>
		request<Member[]>('POST', `${ws(id)}/members`, { email, role }),
	setRole: (id: string, userId: string, role: Role) =>
		request<Member[]>('PATCH', `${ws(id)}/members/${encodeURIComponent(userId)}`, { role }),
	removeMember: (id: string, userId: string) =>
		request<void>('DELETE', `${ws(id)}/members/${encodeURIComponent(userId)}`),

	workflows: (id: string) => request<WorkflowSummary[]>('GET', `${ws(id)}/workflows`),
	workflow: (id: string, wid: string) => request<WorkflowDoc>('GET', `${ws(id)}/workflows/${encodeURIComponent(wid)}`),
	createWorkflow: (id: string, name: string, definition: unknown) =>
		request<WorkflowDoc>('POST', `${ws(id)}/workflows`, { name, definition }),
	saveWorkflow: (id: string, wid: string, name: string, definition: unknown, version: number, keepalive = false) =>
		request<WorkflowDoc>(
			'PUT',
			`${ws(id)}/workflows/${encodeURIComponent(wid)}`,
			{ name, definition, version },
			{ keepalive },
		),
	deleteWorkflow: (id: string, wid: string) =>
		request<void>('DELETE', `${ws(id)}/workflows/${encodeURIComponent(wid)}`),
};

export const errorMessage = (e: unknown) => (e instanceof Error ? e.message : String(e));
