// Réorganisation automatique (« Ranger ») : nœuds en colonnes de gauche à droite selon
// leur profondeur dans le graphe, ordre limitant les croisements, chaque nœud aligné
// sur ses voisins (une chaîne forme une ligne droite) et positions sur la grille.

import type { Point } from './router';

export interface LayoutNode {
	id: string;
	w: number;
	h: number;
	/** Position actuelle (sert à départager les ordres). */
	y: number;
}

export interface LayoutEdge {
	source: string;
	target: string;
	/** Entrée de la cible (0 = en haut) : départage l'ordre des prédécesseurs. */
	input?: number;
}

export function layered(nodes: LayoutNode[], edges: LayoutEdge[], gapX = 120, gapY = 60, grid = 10): Map<string, Point> {
	const ids = new Set(nodes.map((n) => n.id));
	const valid = edges.filter((e) => ids.has(e.source) && ids.has(e.target) && e.source !== e.target);
	const preds = new Map<string, string[]>(nodes.map((n) => [n.id, []]));
	const succs = new Map<string, string[]>(nodes.map((n) => [n.id, []]));
	for (const e of valid) {
		preds.get(e.target)!.push(e.source);
		succs.get(e.source)!.push(e.target);
	}

	// Colonne = plus long chemin depuis une source (ordre topologique ; cycles ignorés).
	const layer = new Map<string, number>();
	const indegree = new Map(nodes.map((n) => [n.id, preds.get(n.id)!.length]));
	const queue = nodes.filter((n) => indegree.get(n.id) === 0).map((n) => n.id);
	for (const id of queue) layer.set(id, 0);
	while (queue.length) {
		const id = queue.shift()!;
		for (const t of succs.get(id)!) {
			layer.set(t, Math.max(layer.get(t) ?? 0, layer.get(id)! + 1));
			indegree.set(t, indegree.get(t)! - 1);
			if (indegree.get(t) === 0) queue.push(t);
		}
	}
	for (const n of nodes) if (!layer.has(n.id)) layer.set(n.id, 0);

	const columns: string[][] = [];
	for (const n of [...nodes].sort((a, b) => a.y - b.y)) {
		const l = layer.get(n.id)!;
		(columns[l] ??= []).push(n.id);
	}

	// Ordre dans chaque colonne : barycentre des voisins (quelques passes aller-retour).
	const position = new Map<string, number>();
	const reindex = () => columns.forEach((col) => col?.forEach((id, i) => position.set(id, i)));
	reindex();
	// Vers les successeurs, l'entrée visée compte : l'entrée A est alimentée par le nœud du haut.
	const inputOf = new Map(valid.map((e) => [`${e.source}>${e.target}`, e.input ?? 0]));
	const bary = (id: string, side: Map<string, string[]>) => {
		const ns = side.get(id)!.filter((x) => position.has(x));
		if (!ns.length) return position.get(id)!;
		const toSuccs = side === succs;
		return ns.reduce((s, x) => s + position.get(x)! + (toSuccs ? (inputOf.get(`${id}>${x}`) ?? 0) * 0.1 : 0), 0) / ns.length;
	};
	for (let pass = 0; pass < 4; pass++) {
		const forward = pass % 2 === 0;
		const order = forward ? columns.keys() : [...columns.keys()].reverse();
		for (const l of order) {
			const col = columns[l];
			if (!col) continue;
			const side = forward ? preds : succs;
			col.sort((a, b) => bary(a, side) - bary(b, side));
			reindex();
		}
	}

	const size = new Map(nodes.map((n) => [n.id, n]));
	const colWidth = columns.map((col) => Math.max(0, ...(col ?? []).map((id) => size.get(id)!.w)));
	const snap = (v: number) => Math.round(v / grid) * grid;

	// Centre vertical de chaque nœud.
	const center = new Map<string, number>();

	/** Place une colonne : chaque nœud au plus près de sa hauteur souhaitée, sans chevauchement. */
	const place = (col: string[], desired: (id: string, fallback: number) => number) => {
		let stackTop = 0;
		const wanted = col.map((id) => {
			const h = size.get(id)!.h;
			const fallback = stackTop + h / 2;
			stackTop += h + gapY;
			return desired(id, fallback);
		});
		// Descente : on respecte l'ordre et l'écart minimal.
		const actual: number[] = [];
		col.forEach((id, i) => {
			const h = size.get(id)!.h;
			let c = wanted[i];
			if (i > 0) {
				const prev = col[i - 1];
				const minC = actual[i - 1] + size.get(prev)!.h / 2 + gapY + h / 2;
				c = Math.max(c, minC);
			}
			actual.push(c);
		});
		// Recentrage global : l'écart moyen avec les hauteurs souhaitées est annulé.
		const shift = wanted.reduce((s, w, i) => s + (w - actual[i]), 0) / col.length;
		col.forEach((id, i) => center.set(id, actual[i] + shift));
	};

	const avg = (ids: string[]) => {
		const known = ids.filter((x) => center.has(x));
		return known.length ? known.reduce((s, x) => s + center.get(x)!, 0) / known.length : null;
	};

	// 1. De gauche à droite : chaque nœud à la hauteur moyenne de ses prédécesseurs.
	columns.forEach((col) => col && place(col, (id, fallback) => avg(preds.get(id)!) ?? fallback));
	// 2. De droite à gauche : les nœuds sans prédécesseur s'alignent sur leurs successeurs.
	[...columns.keys()].reverse().forEach((l) => {
		const col = columns[l];
		if (!col) return;
		place(col, (id) => (preds.get(id)!.length ? center.get(id)! : (avg(succs.get(id)!) ?? center.get(id)!)));
	});

	const out = new Map<string, Point>();
	let x = 0;
	columns.forEach((col, l) => {
		if (!col) return;
		for (const id of col) {
			const h = size.get(id)!.h;
			out.set(id, { x: snap(x), y: snap(center.get(id)! - h / 2) });
		}
		x += colWidth[l] + gapX;
	});
	return out;
}
