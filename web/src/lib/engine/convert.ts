// Conversion d'une définition de workflow (format de l'éditeur / du serveur) vers le moteur.

import type { EngineWorkflow } from './types';

interface DefinitionNode {
	id: string;
	data: { kind: string; name?: string; params?: Record<string, unknown>; disabled?: boolean };
}

interface DefinitionEdge {
	source: string;
	target: string;
	targetHandle?: string | null;
}

export function toEngineWorkflow(def: { nodes: DefinitionNode[]; edges: DefinitionEdge[] }): EngineWorkflow {
	return {
		nodes: def.nodes.map((n) => ({
			id: n.id,
			kind: n.data.kind,
			name: n.data.name ?? '',
			params: n.data.params ?? {},
			disabled: !!n.data.disabled,
		})),
		edges: def.edges.map((e) => ({
			source: e.source,
			target: e.target,
			input: Number(e.targetHandle?.replace('in-', '') ?? 0) || 0,
		})),
	};
}

/** Jeton aléatoire d'URL publique (webhook, formulaire) : 32 caractères base64url. */
export function newToken(): string {
	const bytes = crypto.getRandomValues(new Uint8Array(24));
	return btoa(String.fromCharCode(...bytes)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}
