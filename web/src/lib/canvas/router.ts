// Routage des liens du canevas : chemins orthogonaux qui contournent les nœuds,
// liens superposés écartés en couloirs, petits ponts aux croisements.
//
// 1. Une grille « clairsemée » est construite à partir des bords des nœuds (agrandis
//    d'une marge) et des points de départ / d'arrivée : les liens ne peuvent suivre
//    que ces lignes, sans jamais traverser un nœud.
// 2. Chaque lien est un plus court chemin (A*) sur cette grille, chaque virage étant
//    pénalisé : on obtient des tracés simples, à angles droits.
// 3. Les segments qui se superposent sont écartés les uns des autres (couloirs).
// 4. Le tracé SVG arrondit les virages et dessine un pont là où un lien horizontal
//    en croise un vertical.
//
// Module sans dépendance (testé avec `node --test`).

export interface Point {
	x: number;
	y: number;
}

export interface Rect {
	x: number;
	y: number;
	w: number;
	h: number;
}

export interface RouteRequest {
	id: string;
	from: Point;
	to: Point;
}

export interface RouteOptions {
	/** Distance minimale entre un lien et un nœud. */
	pad?: number;
	/** Longueur du segment horizontal à la sortie et à l'entrée d'un port. */
	stub?: number;
	/** Coût d'un virage (en pixels de détour acceptés pour l'éviter). */
	bend?: number;
	/** Écart entre deux liens parallèles. */
	gap?: number;
}

interface Box {
	x: number;
	y: number;
	x2: number;
	y2: number;
}

const DIRS = [
	[1, 0],
	[0, 1],
	[-1, 0],
	[0, -1],
] as const;
const RIGHT = 0;

/** Au-delà, la grille serait trop coûteuse : tracé simple sans évitement. */
const MAX_GRID = 400_000;

function uniqueSorted(values: number[]): number[] {
	const sorted = values.map((v) => Math.round(v * 2) / 2).sort((a, b) => a - b);
	const out: number[] = [];
	for (const v of sorted) if (out.length === 0 || v - out[out.length - 1] > 0.25) out.push(v);
	return out;
}

/** Ajoute le milieu de chaque intervalle : les liens passent au centre des couloirs. */
function withMidpoints(values: number[]): number[] {
	const out: number[] = [];
	for (let i = 0; i < values.length; i++) {
		out.push(values[i]);
		if (i + 1 < values.length && values[i + 1] - values[i] > 8) out.push((values[i] + values[i + 1]) / 2);
	}
	return out;
}

function indexOf(sorted: number[], v: number): number {
	const r = Math.round(v * 2) / 2;
	let lo = 0;
	let hi = sorted.length - 1;
	while (lo <= hi) {
		const mid = (lo + hi) >> 1;
		if (Math.abs(sorted[mid] - r) <= 0.25) return mid;
		if (sorted[mid] < r) lo = mid + 1;
		else hi = mid - 1;
	}
	return -1;
}

/** File de priorité (tas binaire) sur des entiers. */
class Heap {
	private keys: number[] = [];
	private items: number[] = [];

	get size() {
		return this.items.length;
	}

	push(item: number, key: number) {
		const { keys, items } = this;
		let i = items.length;
		items.push(item);
		keys.push(key);
		while (i > 0) {
			const p = (i - 1) >> 1;
			if (keys[p] <= key) break;
			items[i] = items[p];
			keys[i] = keys[p];
			i = p;
		}
		items[i] = item;
		keys[i] = key;
	}

	pop(): number {
		const { keys, items } = this;
		const top = items[0];
		const lastItem = items.pop()!;
		const lastKey = keys.pop()!;
		if (items.length > 0) {
			let i = 0;
			const n = items.length;
			for (;;) {
				const l = 2 * i + 1;
				if (l >= n) break;
				const r = l + 1;
				const c = r < n && keys[r] < keys[l] ? r : l;
				if (keys[c] >= lastKey) break;
				items[i] = items[c];
				keys[i] = keys[c];
				i = c;
			}
			items[i] = lastItem;
			keys[i] = lastKey;
		}
		return top;
	}
}

class Grid {
	xs: number[];
	ys: number[];
	nx: number;
	ny: number;
	/** Segment horizontal (ligne j, entre les colonnes i et i+1) interdit. */
	hBlocked: Uint8Array;
	/** Segment vertical (colonne i, entre les lignes j et j+1) interdit. */
	vBlocked: Uint8Array;

	constructor(xs: number[], ys: number[], boxes: Box[]) {
		this.xs = xs;
		this.ys = ys;
		this.nx = xs.length;
		this.ny = ys.length;
		this.hBlocked = new Uint8Array(Math.max(0, (this.nx - 1) * this.ny));
		this.vBlocked = new Uint8Array(Math.max(0, this.nx * (this.ny - 1)));
		for (const b of boxes) this.block(b);
	}

	private block(b: Box) {
		const ix0 = indexOf(this.xs, b.x);
		const ix1 = indexOf(this.xs, b.x2);
		const iy0 = indexOf(this.ys, b.y);
		const iy1 = indexOf(this.ys, b.y2);
		if (ix0 < 0 || ix1 < 0 || iy0 < 0 || iy1 < 0) return;
		// Les bords restent praticables, l'intérieur non.
		for (let j = iy0 + 1; j < iy1; j++) {
			for (let i = ix0; i < ix1; i++) this.hBlocked[j * (this.nx - 1) + i] = 1;
		}
		for (let i = ix0 + 1; i < ix1; i++) {
			for (let j = iy0; j < iy1; j++) this.vBlocked[i * (this.ny - 1) + j] = 1;
		}
	}

	/** Voisin dans la direction `d`, ou -1 si le segment est interdit. */
	step(i: number, j: number, d: number): number {
		switch (d) {
			case 0:
				return i + 1 < this.nx && !this.hBlocked[j * (this.nx - 1) + i] ? j * this.nx + i + 1 : -1;
			case 2:
				return i > 0 && !this.hBlocked[j * (this.nx - 1) + i - 1] ? j * this.nx + i - 1 : -1;
			case 1:
				return j + 1 < this.ny && !this.vBlocked[i * (this.ny - 1) + j] ? (j + 1) * this.nx + i : -1;
			default:
				return j > 0 && !this.vBlocked[i * (this.ny - 1) + j - 1] ? (j - 1) * this.nx + i : -1;
		}
	}
}

/** Plus court chemin avec pénalité de virage, du point s (sortant vers la droite) au point t (entrant vers la droite). */
function search(grid: Grid, s: Point, t: Point, bend: number): Point[] | null {
	const si = indexOf(grid.xs, s.x);
	const sj = indexOf(grid.ys, s.y);
	const ti = indexOf(grid.xs, t.x);
	const tj = indexOf(grid.ys, t.y);
	if (si < 0 || sj < 0 || ti < 0 || tj < 0) return null;
	const n = grid.nx * grid.ny * 4;
	const cost = new Float64Array(n).fill(Infinity);
	const prev = new Int32Array(n).fill(-1);
	const heap = new Heap();
	const h = (i: number, j: number) => Math.abs(grid.xs[i] - t.x) + Math.abs(grid.ys[j] - t.y);
	const start = (sj * grid.nx + si) * 4 + RIGHT;
	cost[start] = 0;
	heap.push(start, h(si, sj));
	const target = tj * grid.nx + ti;
	let best = -1;
	let bestCost = Infinity;
	while (heap.size) {
		const state = heap.pop();
		const c = cost[state];
		const point = state >> 2;
		const dir = state & 3;
		const i = point % grid.nx;
		const j = (point - i) / grid.nx;
		if (c + h(i, j) >= bestCost) break;
		if (point === target) {
			// On doit finir en allant vers la droite (entrée du port).
			const total = c + (dir === RIGHT ? 0 : bend * (dir === 2 ? 2 : 1));
			if (total < bestCost) {
				bestCost = total;
				best = state;
			}
			continue;
		}
		for (let d = 0; d < 4; d++) {
			if (d === ((dir + 2) & 3)) continue; // pas de demi-tour
			const next = grid.step(i, j, d);
			if (next < 0) continue;
			const ni = next % grid.nx;
			const nj = (next - ni) / grid.nx;
			const len = Math.abs(grid.xs[ni] - grid.xs[i]) + Math.abs(grid.ys[nj] - grid.ys[j]);
			const nc = c + len + (d === dir ? 0 : bend);
			const ns = next * 4 + d;
			if (nc < cost[ns]) {
				cost[ns] = nc;
				prev[ns] = state;
				heap.push(ns, nc + h(ni, nj));
			}
		}
	}
	if (best < 0) return null;
	const points: Point[] = [];
	for (let st = best; st >= 0; st = prev[st]) {
		const p = st >> 2;
		const i = p % grid.nx;
		points.push({ x: grid.xs[i], y: grid.ys[(p - i) / grid.nx] });
	}
	return points.reverse();
}

/** Retire les points intermédiaires alignés. */
export function simplify(points: Point[]): Point[] {
	const out: Point[] = [];
	for (const p of points) {
		if (out.length && Math.abs(out[out.length - 1].x - p.x) < 0.01 && Math.abs(out[out.length - 1].y - p.y) < 0.01) continue;
		if (out.length >= 2) {
			const a = out[out.length - 2];
			const b = out[out.length - 1];
			const colinear = (Math.abs(a.x - b.x) < 0.01 && Math.abs(b.x - p.x) < 0.01) || (Math.abs(a.y - b.y) < 0.01 && Math.abs(b.y - p.y) < 0.01);
			if (colinear) {
				out[out.length - 1] = p;
				continue;
			}
		}
		out.push({ ...p });
	}
	return out;
}

/** Tracé de secours sans évitement (grille trop grande ou aucun chemin). */
function fallback(from: Point, to: Point, stub: number): Point[] {
	const s = { x: from.x + stub, y: from.y };
	const t = { x: to.x - stub, y: to.y };
	if (t.x >= s.x) {
		const mx = (s.x + t.x) / 2;
		return simplify([from, { x: mx, y: from.y }, { x: mx, y: to.y }, to]);
	}
	const my = (from.y + to.y) / 2;
	return simplify([from, s, { x: s.x, y: my }, { x: t.x, y: my }, t, to]);
}

function inside(b: Box, p: Point) {
	return p.x > b.x && p.x < b.x2 && p.y > b.y && p.y < b.y2;
}

/** Calcule le tracé de chaque lien (points d'une ligne brisée orthogonale). */
export function routeEdges(requests: RouteRequest[], obstacles: Rect[], options: RouteOptions = {}): Map<string, Point[]> {
	const pad = options.pad ?? 16;
	const stub = options.stub ?? pad + 14;
	const bend = options.bend ?? 28;
	const gap = options.gap ?? 7;
	const routes = new Map<string, Point[]>();
	if (requests.length === 0) return routes;
	// Extrémités alignées sur la grille (demi-pixel) : segments parfaitement droits.
	const half = (p: Point): Point => ({ x: Math.round(p.x * 2) / 2, y: Math.round(p.y * 2) / 2 });
	requests = requests.map((r) => ({ ...r, from: half(r.from), to: half(r.to) }));

	const boxes: Box[] = obstacles.map((r) => ({ x: r.x - pad, y: r.y - pad, x2: r.x + r.w + pad, y2: r.y + r.h + pad }));
	const ends = requests.map((r) => ({ s: { x: r.from.x + stub, y: r.from.y }, t: { x: r.to.x - stub, y: r.to.y } }));

	const xs0: number[] = [];
	const ys0: number[] = [];
	for (const b of boxes) xs0.push(b.x, b.x2), ys0.push(b.y, b.y2);
	for (const e of ends) xs0.push(e.s.x, e.t.x), ys0.push(e.s.y, e.t.y);
	const minX = Math.min(...xs0) - 3 * pad;
	const maxX = Math.max(...xs0) + 3 * pad;
	const minY = Math.min(...ys0) - 3 * pad;
	const maxY = Math.max(...ys0) + 3 * pad;
	const xs = uniqueSorted(withMidpoints(uniqueSorted([...xs0, minX, maxX])));
	const ys = uniqueSorted(withMidpoints(uniqueSorted([...ys0, minY, maxY])));

	if (xs.length * ys.length > MAX_GRID) {
		for (const r of requests) routes.set(r.id, fallback(r.from, r.to, stub));
		return routes;
	}
	const grid = new Grid(xs, ys, boxes);
	let looseGrid: Grid | null = null;

	requests.forEach((r, k) => {
		const { s, t } = ends[k];
		let path = search(grid, s, t, bend);
		if (!path) {
			// Port collé à un autre nœud : on ignore les obstacles qui l'englobent.
			const free = boxes.filter((b) => !inside(b, s) && !inside(b, t));
			if (free.length !== boxes.length) {
				looseGrid = new Grid(xs, ys, free);
				path = search(looseGrid, s, t, bend);
			}
		}
		routes.set(r.id, path ? simplify([r.from, ...path, r.to]) : fallback(r.from, r.to, stub));
	});

	separate(routes, gap, pad);
	return routes;
}

interface Seg {
	route: Point[];
	/** Indice du premier point du segment. */
	k: number;
	/** Coordonnée fixe (x d'un vertical, y d'un horizontal). */
	at: number;
	lo: number;
	hi: number;
	/** Ordre de couloir (croissant : à gauche / en haut). */
	order: number;
}

/** Écarte les segments intérieurs qui se superposent (même ligne, intervalles qui se chevauchent). */
function separate(routes: Map<string, Point[]>, gap: number, pad: number) {
	for (const vertical of [true, false]) {
		const groups = new Map<number, Seg[]>();
		for (const route of routes.values()) {
			// Ni le premier ni le dernier segment : ils sont attachés aux ports.
			for (let k = 1; k < route.length - 2; k++) {
				const a = route[k];
				const b = route[k + 1];
				const isVertical = Math.abs(a.x - b.x) < 0.01;
				if (isVertical !== vertical) continue;
				const at = vertical ? a.x : a.y;
				const lo = vertical ? Math.min(a.y, b.y) : Math.min(a.x, b.x);
				const hi = vertical ? Math.max(a.y, b.y) : Math.max(a.x, b.x);
				// Ordre des couloirs : un segment qui tourne plus tôt se place du côté
				// vers lequel il tourne, pour ne pas croiser ses voisins.
				const after = route[k + 2];
				const main = vertical ? Math.sign(b.y - a.y) : Math.sign(b.x - a.x);
				const turn = vertical ? Math.sign(after.x - b.x) : Math.sign(after.y - b.y);
				const order = -main * turn * (vertical ? b.y : b.x);
				const key = Math.round(at * 2) / 2;
				const list = groups.get(key) ?? [];
				list.push({ route, k, at, lo, hi, order });
				groups.set(key, list);
			}
		}
		for (const list of groups.values()) {
			if (list.length < 2) continue;
			list.sort((a, b) => a.lo - b.lo);
			// Groupes de segments qui se chevauchent de proche en proche.
			let cluster: Seg[] = [];
			let end = -Infinity;
			const flush = () => {
				if (cluster.length > 1) spread(cluster, vertical, gap, pad);
				cluster = [];
			};
			for (const seg of list) {
				if (seg.lo >= end - 1) flush();
				cluster.push(seg);
				end = Math.max(end, seg.hi);
			}
			flush();
		}
	}
}

function spread(cluster: Seg[], vertical: boolean, gap: number, pad: number) {
	// Couloirs : un segment par couloir tant qu'ils se chevauchent.
	cluster.sort((a, b) => a.order - b.order || a.lo - b.lo);
	const lanes: number[] = [];
	const laneOf = new Map<Seg, number>();
	for (const seg of cluster) {
		let lane = lanes.findIndex((end) => end <= seg.lo - 1);
		if (lane < 0) {
			lane = lanes.length;
			lanes.push(seg.hi);
		} else {
			lanes[lane] = seg.hi;
		}
		laneOf.set(seg, lane);
	}
	if (lanes.length < 2) return;
	const step = Math.min(gap, ((pad - 4) * 2) / (lanes.length - 1));
	for (const seg of cluster) {
		const offset = (laneOf.get(seg)! - (lanes.length - 1) / 2) * step;
		const a = seg.route[seg.k];
		const b = seg.route[seg.k + 1];
		if (vertical) {
			a.x += offset;
			b.x += offset;
		} else {
			a.y += offset;
			b.y += offset;
		}
	}
}

/** Croisements : pour chaque lien, abscisses où un de ses segments horizontaux croise un vertical d'un autre lien. */
export function crossings(routes: Map<string, Point[]>, margin = 14): Map<string, Map<number, number[]>> {
	const verticals: { id: string; x: number; lo: number; hi: number }[] = [];
	for (const [id, r] of routes) {
		for (let k = 0; k < r.length - 1; k++) {
			if (Math.abs(r[k].x - r[k + 1].x) < 0.01) {
				verticals.push({ id, x: r[k].x, lo: Math.min(r[k].y, r[k + 1].y), hi: Math.max(r[k].y, r[k + 1].y) });
			}
		}
	}
	const out = new Map<string, Map<number, number[]>>();
	for (const [id, r] of routes) {
		const perSegment = new Map<number, number[]>();
		for (let k = 0; k < r.length - 1; k++) {
			const a = r[k];
			const b = r[k + 1];
			if (Math.abs(a.y - b.y) >= 0.01) continue;
			const lo = Math.min(a.x, b.x) + margin;
			const hi = Math.max(a.x, b.x) - margin;
			const xsHit = verticals
				.filter((v) => v.id !== id && v.x > lo && v.x < hi && a.y > v.lo + 2 && a.y < v.hi - 2)
				.map((v) => v.x);
			if (xsHit.length) perSegment.set(k, xsHit);
		}
		if (perSegment.size) out.set(id, perSegment);
	}
	return out;
}

const f = (n: number) => Math.round(n * 10) / 10;

/** Direction dominante d'un segment (un seul axe). */
function axis(a: Point, b: Point): Point {
	const dx = b.x - a.x;
	const dy = b.y - a.y;
	return Math.abs(dx) >= Math.abs(dy) ? { x: Math.sign(dx), y: 0 } : { x: 0, y: Math.sign(dy) };
}

/** Tracé SVG : virages arrondis et ponts aux croisements (`hops` : abscisses par segment). */
export function pathFor(points: Point[], hops?: Map<number, number[]>, radius = 10, hop = 5): string {
	if (points.length < 2) return '';
	let d = `M${f(points[0].x)},${f(points[0].y)}`;
	// Point d'arrivée de chaque segment, raccourci par l'arrondi du virage suivant.
	let cur = points[0];
	for (let k = 0; k < points.length - 1; k++) {
		const a = points[k];
		const b = points[k + 1];
		const c = points[k + 2];
		let end = b;
		let r = 0;
		const din = axis(a, b);
		const dout = c ? axis(b, c) : null;
		if (c && dout && (din.x !== dout.x || din.y !== dout.y)) {
			const lenAB = Math.abs(b.x - a.x) + Math.abs(b.y - a.y);
			const lenBC = Math.abs(c.x - b.x) + Math.abs(c.y - b.y);
			r = Math.min(radius, lenAB / 2, lenBC / 2);
			end = { x: b.x - din.x * r, y: b.y - din.y * r };
		}
		// Ponts sur les segments horizontaux.
		const xsHop = hops?.get(k);
		if (xsHop && Math.abs(a.y - b.y) < 0.01) {
			const dir = Math.sign(b.x - a.x) || 1;
			const sorted = [...xsHop].sort((p, q) => (p - q) * dir);
			for (const x of sorted) {
				if ((x - cur.x) * dir < hop + 1 || (end.x - x) * dir < hop + 1) continue;
				d += ` L${f(x - dir * hop)},${f(a.y)} A${hop},${hop} 0 0 ${dir > 0 ? 1 : 0} ${f(x + dir * hop)},${f(a.y)}`;
			}
		}
		d += ` L${f(end.x)},${f(end.y)}`;
		if (c && dout && r > 0) {
			const next = { x: b.x + dout.x * r, y: b.y + dout.y * r };
			d += ` Q${f(b.x)},${f(b.y)} ${f(next.x)},${f(next.y)}`;
			cur = next;
		} else {
			cur = end;
		}
	}
	return d;
}

/** Point où placer une étiquette : milieu du plus long segment horizontal. */
export function labelPoint(points: Point[]): Point {
	let best = { x: (points[0].x + points[points.length - 1].x) / 2, y: (points[0].y + points[points.length - 1].y) / 2 };
	let bestLen = -1;
	for (let k = 0; k < points.length - 1; k++) {
		const a = points[k];
		const b = points[k + 1];
		const len = Math.abs(b.x - a.x);
		if (Math.abs(a.y - b.y) < 0.01 && len > bestLen) {
			bestLen = len;
			best = { x: (a.x + b.x) / 2, y: a.y };
		}
	}
	return best;
}

/** Le segment [a, b] traverse-t-il l'intérieur du rectangle ? (tests) */
export function crossesRect(a: Point, b: Point, r: Rect): boolean {
	const x1 = Math.min(a.x, b.x);
	const x2 = Math.max(a.x, b.x);
	const y1 = Math.min(a.y, b.y);
	const y2 = Math.max(a.y, b.y);
	return x2 > r.x && x1 < r.x + r.w && y2 > r.y && y1 < r.y + r.h;
}
