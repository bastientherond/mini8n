// Ressources du workspace courant utilisées par les nœuds : identifiants et fichiers.

import { api, type CredentialInfo, type CredentialKind, type FileInfo } from './api';

class Resources {
	workspaceId = $state<string | null>(null);
	credentials = $state.raw<CredentialInfo[]>([]);
	files = $state.raw<FileInfo[]>([]);
	kinds = $state.raw<CredentialKind[]>([]);

	async load(workspaceId: string) {
		this.workspaceId = workspaceId;
		const [credentials, files, kinds] = await Promise.all([
			api.credentials(workspaceId),
			api.files(workspaceId),
			this.kinds.length ? Promise.resolve(this.kinds) : api.credentialTypes(),
		]);
		if (this.workspaceId !== workspaceId) return;
		this.credentials = credentials;
		this.files = files;
		this.kinds = kinds;
	}

	async refreshCredentials() {
		if (this.workspaceId) this.credentials = await api.credentials(this.workspaceId);
	}

	async refreshFiles() {
		if (this.workspaceId) this.files = await api.files(this.workspaceId);
	}

	kind(name: string): CredentialKind | undefined {
		return this.kinds.find((k) => k.kind === name);
	}

	reset() {
		this.workspaceId = null;
		this.credentials = [];
		this.files = [];
	}
}

export const resources = new Resources();
