// Opérations sur le graphe indépendantes de l'interface (testées avec `node --test`).

export interface GraphEdge {
	id: string;
	source: string;
	target: string;
	targetHandle?: string | null;
}

const handleOf = (e: GraphEdge) => e.targetHandle ?? 'in-0';

/**
 * Supprime des nœuds en conservant la continuité du flux : les nœuds qui alimentaient
 * l'entrée principale d'un nœud supprimé sont reliés à ce qu'il alimentait.
 * `trigger → fichier → mail` sans `fichier` donne `trigger → mail`.
 *
 * Les nœuds sont retirés un par un : une chaîne de nœuds supprimés est donc enjambée
 * entièrement. Seule l'entrée principale (`in-0`) est reportée : pour une fusion,
 * relier ses deux sources à la suite changerait le sens du workflow.
 */
export function removeNodesKeepingLinks<E extends GraphEdge>(
	edges: E[],
	removed: Iterable<string>,
	makeEdge: (source: string, target: string, targetHandle: string) => E,
): E[] {
	let out = [...edges];
	for (const id of removed) {
		const sources = out.filter((e) => e.target === id && handleOf(e) === 'in-0').map((e) => e.source);
		const targets = out.filter((e) => e.source === id);
		out = out.filter((e) => e.source !== id && e.target !== id);
		for (const s of sources) {
			for (const t of targets) {
				if (s === t.target) continue;
				const handle = handleOf(t);
				if (out.some((e) => e.source === s && e.target === t.target && handleOf(e) === handle)) continue;
				out.push(makeEdge(s, t.target, handle));
			}
		}
	}
	return out;
}
