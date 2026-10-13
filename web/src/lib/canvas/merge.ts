// Fusion à trois voies d'un workflow modifié en même temps par plusieurs personnes
// (testée avec `node --test`).
//
// `base` est la dernière version enregistrée connue, `local` l'état de l'éditeur et
// `remote` la nouvelle version enregistrée par quelqu'un d'autre. Chaque nœud, chaque lien
// et le nom sont fusionnés séparément : une modification d'un seul côté est conservée ;
// si les deux côtés ont modifié le même élément, la modification locale l'emporte.

export interface MergeNode {
	id: string;
}

export interface MergeEdge {
	id: string;
	source: string;
	target: string;
	targetHandle?: string | null;
}

export interface MergeGraph<N extends MergeNode, E extends MergeEdge> {
	name: string;
	nodes: N[];
	edges: E[];
}

/** JSON aux clés triées : deux objets égaux donnent la même chaîne. */
export function canonical(value: unknown): string {
	if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
	if (value && typeof value === 'object') {
		const entries = Object.entries(value as Record<string, unknown>)
			.filter(([, v]) => v !== undefined)
			.sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
		return `{${entries.map(([k, v]) => `${JSON.stringify(k)}:${canonical(v)}`).join(',')}}`;
	}
	return JSON.stringify(value) ?? 'null';
}

const same = (a: unknown, b: unknown) => a === b || canonical(a) === canonical(b);

function mergeItems<T extends { id: string }>(base: T[], local: T[], remote: T[]): T[] {
	const b = new Map(base.map((x) => [x.id, x]));
	const l = new Map(local.map((x) => [x.id, x]));
	const r = new Map(remote.map((x) => [x.id, x]));
	const pick = (id: string): T | undefined => {
		const [bx, lx, rx] = [b.get(id), l.get(id), r.get(id)];
		if (same(lx, bx)) return rx; // inchangé ici : la version distante
		if (same(rx, bx)) return lx; // inchangé là-bas : la version locale
		return lx; // modifié des deux côtés : la version locale l'emporte
	};
	// Ordre distant, puis les éléments ajoutés localement.
	const ids = [...r.keys(), ...[...l.keys()].filter((id) => !r.has(id))];
	return ids.map(pick).filter((x): x is T => x !== undefined);
}

export function mergeGraph<N extends MergeNode, E extends MergeEdge>(
	base: MergeGraph<N, E>,
	local: MergeGraph<N, E>,
	remote: MergeGraph<N, E>,
): MergeGraph<N, E> {
	const nodes = mergeItems(base.nodes, local.nodes, remote.nodes);
	const ids = new Set(nodes.map((n) => n.id));
	// Liens vers un nœud supprimé de l'autre côté, en double, ou qui fermeraient une boucle
	// (a → b ajouté d'un côté, b → a de l'autre) : retirés.
	const seen = new Set<string>();
	const next = new Map<string, string[]>();
	const reaches = (from: string, to: string) => {
		const stack = [from];
		const visited = new Set<string>();
		while (stack.length) {
			const id = stack.pop()!;
			if (id === to) return true;
			if (visited.has(id)) continue;
			visited.add(id);
			stack.push(...(next.get(id) ?? []));
		}
		return false;
	};
	const edges = mergeItems(base.edges, local.edges, remote.edges).filter((e) => {
		const key = `${e.source}>${e.target}:${e.targetHandle ?? 'in-0'}`;
		if (!ids.has(e.source) || !ids.has(e.target) || seen.has(key) || reaches(e.target, e.source)) return false;
		seen.add(key);
		next.set(e.source, [...(next.get(e.source) ?? []), e.target]);
		return true;
	});
	const name = local.name === base.name ? remote.name : local.name;
	return { name, nodes, edges };
}
