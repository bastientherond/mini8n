// Client de l'API mini8n (même origine, cookie de session HttpOnly).

export interface User {
	id: string;
	email: string;
	name: string;
	emailVerified?: boolean;
}

/** Configuration publique (écran de connexion). */
export interface PublicConfig {
	allowRegistration: boolean;
	/** Aucun compte : le premier créé sera l'administrateur. */
	firstAccount?: boolean;
	/** Réinitialisation du mot de passe par e-mail disponible. */
	passwordReset?: boolean;
}

/** Invitation créée en ajoutant une personne sans compte vérifié. */
export interface Invitation {
	id: string;
	email: string;
	role: Role;
	expiresAt: string;
	/** Envoyée par e-mail ; sinon `link` est à transmettre. */
	delivered: boolean;
	link?: string;
}

export interface PendingInvitation {
	id: string;
	email: string;
	role: Role;
	invitedBy: string | null;
	delivered: boolean;
	expiresAt: string;
}

/** Invitation vue par la personne invitée. */
export interface InvitationInfo {
	workspace: string;
	email: string;
	role: Role;
	invitedBy: string | null;
	expiresAt: string;
	accountExists: boolean;
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
	/** Un lien de vérification d'adresse peut être envoyé. */
	canVerifyEmail?: boolean;
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
	active: boolean;
	updatedAt: string;
	updatedBy: string | null;
}

/** Jeton d'accès d'une IA (serveur MCP). */
/** Portée d'un jeton d'IA. */
export type TokenScope = 'lecture' | 'test' | 'complet';

export interface ApiToken {
	id: string;
	name: string;
	scope: TokenScope;
	createdAt: string;
	lastUsedAt: string | null;
	expiresAt: string | null;
}

export interface WorkflowDoc {
	id: string;
	name: string;
	version: number;
	active: boolean;
	definition: unknown;
	updatedAt: string;
	/** Motif de désactivation automatique après une sauvegarde. */
	deactivated?: string;
}

export type TriggerKind = 'manual' | 'schedule' | 'webhook' | 'form' | 'poll' | 'workflow' | 'error';

export interface TriggerInfo {
	nodeId: string;
	kind: TriggerKind;
	token: string | null;
	nextRunAt: string | null;
	lastError?: string;
}

export interface ActivationState {
	active: boolean;
	triggers: TriggerInfo[];
}

export type ExecutionStatus = 'running' | 'success' | 'error';

export interface ExecutionSummary {
	id: string;
	workflowId: string | null;
	workflowName: string;
	triggerKind: TriggerKind;
	triggerNode: string | null;
	parentId: string | null;
	status: ExecutionStatus;
	error: string | null;
	startedAt: string;
	finishedAt: string | null;
	durationMs: number | null;
}

export interface ExecutionDetail extends ExecutionSummary {
	report: {
		order?: string[];
		names?: Record<string, string>;
		results?: Record<string, { status: string; error?: string; rows: number; durationMs: number }>;
		output?: string;
	} | null;
	output: { columns: { name: string; type: string }[]; rows: Record<string, unknown>[]; total: number } | null;
}

export interface CredentialField {
	name: string;
	label: string;
	type: 'text' | 'number' | 'password' | 'select';
	secret: boolean;
	required: boolean;
	default: unknown;
	options?: string[];
	placeholder?: string;
}

export interface CredentialKind {
	kind: string;
	label: string;
	description: string;
	fields: CredentialField[];
}

/** Un nœud d'un workflow qui référence un identifiant. */
export interface CredentialUsage {
	workflowId: string;
	workflowName: string;
	nodeId: string;
	nodeName: string;
	nodeKind: string;
}

export interface CredentialInfo {
	id: string;
	name: string;
	kind: string;
	/** Champs non secrets ; `<champ>_defini` indique si un secret est enregistré. */
	summary: Record<string, unknown>;
	updatedAt: string;
	/** Nœuds qui utilisent cet identifiant (renseigné par la liste). */
	usage?: CredentialUsage[];
}

export interface FileInfo {
	id: string;
	name: string;
	contentType: string;
	size: number;
	executionId: string | null;
	createdAt: string;
}

export interface PublicForm {
	title: string;
	description: string;
	fields: { key: string; label: string; kind: string; required: boolean }[];
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
	config: () => request<PublicConfig>('GET', '/auth/config'),
	forgotPassword: (email: string) => request<void>('POST', '/auth/password/forgot', { email }),
	resetPassword: (token: string, password: string) => request<void>('POST', '/auth/password/reset', { token, password }),
	sendVerification: () => request<void>('POST', '/auth/email/send-verification'),
	verifyEmail: (token: string) => request<void>('POST', '/auth/email/verify', { token }),
	invitation: (token: string) => request<InvitationInfo>('GET', `/auth/invitations/${encodeURIComponent(token)}`),
	acceptInvitation: (token: string) =>
		request<{ workspaceId: string }>('POST', `/auth/invitations/${encodeURIComponent(token)}/accept`),
	registerInvited: (token: string, name: string, password: string) =>
		request<{ workspaceId: string }>('POST', `/auth/invitations/${encodeURIComponent(token)}/register`, { name, password }),
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
		request<{ members: Member[]; invitation?: Invitation }>('POST', `${ws(id)}/members`, { email, role }),
	invitations: (id: string) => request<PendingInvitation[]>('GET', `${ws(id)}/invitations`),
	revokeInvitation: (id: string, iid: string) => request<void>('DELETE', `${ws(id)}/invitations/${encodeURIComponent(iid)}`),
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

	triggers: (id: string, wid: string) =>
		request<ActivationState>('GET', `${ws(id)}/workflows/${encodeURIComponent(wid)}/triggers`),
	setActive: (id: string, wid: string, active: boolean) =>
		request<ActivationState>('PUT', `${ws(id)}/workflows/${encodeURIComponent(wid)}/active`, { active }),
	runOnServer: (id: string, wid: string) =>
		request<{ executionId: string; status: ExecutionStatus | 'running'; error?: string }>(
			'POST',
			`${ws(id)}/workflows/${encodeURIComponent(wid)}/run`,
		),

	executions: (id: string, params: { workflow?: string; status?: string; before?: string; limit?: number } = {}) => {
		const q = new URLSearchParams();
		for (const [k, v] of Object.entries(params)) if (v !== undefined && v !== '') q.set(k, String(v));
		const qs = q.toString();
		return request<ExecutionSummary[]>('GET', `${ws(id)}/executions${qs ? `?${qs}` : ''}`);
	},
	execution: (id: string, eid: string) => request<ExecutionDetail>('GET', `${ws(id)}/executions/${encodeURIComponent(eid)}`),

	tokens: (id: string) => request<ApiToken[]>('GET', `${ws(id)}/tokens`),
	createToken: (id: string, name: string, expiresInDays: number | null, scope: TokenScope) =>
		request<ApiToken & { token: string }>('POST', `${ws(id)}/tokens`, { name, expiresInDays, scope }),
	deleteToken: (id: string, tid: string) => request<void>('DELETE', `${ws(id)}/tokens/${encodeURIComponent(tid)}`),

	credentialTypes: () => request<CredentialKind[]>('GET', '/credential-types'),
	credentials: (id: string) => request<CredentialInfo[]>('GET', `${ws(id)}/credentials`),
	createCredential: (id: string, name: string, kind: string, data: Record<string, unknown>) =>
		request<CredentialInfo>('POST', `${ws(id)}/credentials`, { name, kind, data }),
	updateCredential: (id: string, cid: string, name: string, data: Record<string, unknown>) =>
		request<CredentialInfo>('PUT', `${ws(id)}/credentials/${encodeURIComponent(cid)}`, { name, data }),
	deleteCredential: (id: string, cid: string) => request<void>('DELETE', `${ws(id)}/credentials/${encodeURIComponent(cid)}`),
	testCredential: (id: string, cid: string) =>
		request<{ ok: boolean; message: string }>('POST', `${ws(id)}/credentials/${encodeURIComponent(cid)}/test`),

	files: (id: string) => request<FileInfo[]>('GET', `${ws(id)}/files`),
	deleteFile: (id: string, fid: string) => request<void>('DELETE', `${ws(id)}/files/${encodeURIComponent(fid)}`),
	fileUrl: (id: string, fid: string) => `/api${ws(id)}/files/${encodeURIComponent(fid)}/content`,
	uploadFile: async (id: string, file: File): Promise<FileInfo> => {
		const res = await fetch(`/api${ws(id)}/files?name=${encodeURIComponent(file.name)}`, {
			method: 'POST',
			credentials: 'same-origin',
			body: file,
		});
		const data = await res.json().catch(() => null);
		if (!res.ok) throw new ApiError(res.status, (data as { error?: string } | null)?.error ?? `Erreur ${res.status}`);
		return data as FileInfo;
	},

	publicForm: (token: string) => request<PublicForm>('GET', `/forms/${encodeURIComponent(token)}`),
	submitForm: (token: string, values: Record<string, unknown>) =>
		request<{ message: string }>('POST', `/forms/${encodeURIComponent(token)}`, { values }),
};

export const errorMessage = (e: unknown) => (e instanceof Error ? e.message : String(e));
