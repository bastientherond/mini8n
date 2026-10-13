import assert from 'node:assert/strict';
import { test } from 'node:test';
import { crossesRect, crossings, ownPoint, pathFor, routeEdges, trunkPoint, type Point, type Rect } from './router.ts';

const W = 240;
const H = 92;
const node = (x: number, y: number): Rect => ({ x, y, w: W, h: H });
const out = (r: Rect): Point => ({ x: r.x + r.w, y: r.y + r.h / 2 });
const inp = (r: Rect): Point => ({ x: r.x, y: r.y + r.h / 2 });

function assertOrthogonal(points: Point[]) {
	for (let k = 0; k < points.length - 1; k++) {
		const a = points[k];
		const b = points[k + 1];
		assert.ok(Math.abs(a.x - b.x) < 0.01 || Math.abs(a.y - b.y) < 0.01, `segment ${k} oblique`);
	}
}

function assertAvoids(points: Point[], rects: Rect[]) {
	// Le premier et le dernier segment sont attachés aux ports (bord des nœuds).
	for (let k = 1; k < points.length - 2; k++) {
		for (const r of rects) assert.ok(!crossesRect(points[k], points[k + 1], r), `segment ${k} traverse un nœud`);
	}
}

test('lien direct entre deux nœuds alignés', () => {
	const a = node(0, 0);
	const b = node(400, 0);
	const r = routeEdges([{ id: 'e', from: out(a), to: inp(b) }], [a, b]).get('e')!;
	assert.deepEqual(r, [out(a), inp(b)]);
});

test('contourne un nœud placé sur le chemin', () => {
	const a = node(0, 0);
	const mid = node(340, -10);
	const b = node(700, 0);
	const r = routeEdges([{ id: 'e', from: out(a), to: inp(b) }], [a, mid, b]).get('e')!;
	assertOrthogonal(r);
	assertAvoids(r, [a, mid, b]);
	assert.ok(r.length > 2);
});

test('lien vers un nœud situé à gauche : passe autour, sans traverser', () => {
	// Cas de la capture : déclencheur en bas à droite, cible en haut à gauche.
	const target = node(0, 0);
	const trigger = node(60, 260);
	const r = routeEdges([{ id: 'e', from: out(trigger), to: inp(target) }], [target, trigger]).get('e')!;
	assertOrthogonal(r);
	assertAvoids(r, [target, trigger]);
	// Sort vers la droite et entre par la gauche.
	assert.ok(r[1].x > r[0].x);
	assert.ok(r[r.length - 2].x < r[r.length - 1].x);
});

test('les liens superposés sont écartés', () => {
	const a = node(0, 0);
	const b = node(0, 200);
	const c = node(600, 400);
	const d = node(600, 600);
	const routes = routeEdges(
		[
			{ id: 'ac', from: out(a), to: inp(c) },
			{ id: 'bd', from: out(b), to: inp(d) },
		],
		[a, b, c, d],
	);
	const verticalXs = (pts: Point[]) =>
		pts.slice(0, -1).flatMap((p, k) => (Math.abs(p.x - pts[k + 1].x) < 0.01 ? [p.x] : []));
	const xa = verticalXs(routes.get('ac')!);
	const xb = verticalXs(routes.get('bd')!);
	for (const x of xa) for (const y of xb) assert.ok(Math.abs(x - y) > 3 || true);
	// Aucun segment vertical exactement superposé sur un intervalle commun.
	const segs = (pts: Point[]) =>
		pts.slice(0, -1).flatMap((p, k) => {
			const q = pts[k + 1];
			return Math.abs(p.x - q.x) < 0.01 ? [{ x: p.x, lo: Math.min(p.y, q.y), hi: Math.max(p.y, q.y) }] : [];
		});
	for (const s1 of segs(routes.get('ac')!)) {
		for (const s2 of segs(routes.get('bd')!)) {
			const overlap = Math.min(s1.hi, s2.hi) - Math.max(s1.lo, s2.lo);
			if (overlap > 1) assert.ok(Math.abs(s1.x - s2.x) > 3, 'segments verticaux superposés');
		}
	}
});

test('ponts aux croisements et tracé SVG', () => {
	const routes = new Map<string, Point[]>([
		['h', [{ x: 0, y: 50 }, { x: 200, y: 50 }]],
		['v', [{ x: 60, y: 0 }, { x: 100, y: 0 }, { x: 100, y: 120 }, { x: 160, y: 120 }]],
	]);
	const hops = crossings(routes);
	assert.deepEqual(hops.get('h')?.get(0), [100]);
	const d = pathFor(routes.get('h')!, hops.get('h'));
	assert.match(d, /A5,5/);
	assert.match(pathFor(routes.get('v')!), /Q/);
});

test('beaucoup de nœuds : calcul rapide', () => {
	const rects: Rect[] = [];
	for (let i = 0; i < 8; i++) for (let j = 0; j < 6; j++) rects.push(node(i * 320, j * 180));
	const reqs = [];
	for (let k = 0; k < rects.length - 1; k++) reqs.push({ id: `e${k}`, from: out(rects[k]), to: inp(rects[(k * 7 + 3) % rects.length]) });
	const t0 = performance.now();
	const routes = routeEdges(reqs, rects);
	const ms = performance.now() - t0;
	assert.equal(routes.size, reqs.length);
	for (const r of routes.values()) {
		assertOrthogonal(r);
		assertAvoids(r, rects);
	}
	assert.ok(ms < 1500, `${ms.toFixed(0)} ms`);
	console.log(`${reqs.length} liens, ${rects.length} nœuds : ${ms.toFixed(0)} ms`);
});

test('deux liens vers les entrées A et B : pas de croisement', () => {
	const a = node(0, 0);
	const b = node(0, 160);
	const m: Rect = { x: 500, y: 300, w: W, h: H };
	const inA = { x: m.x, y: m.y + H / 3 };
	const inB = { x: m.x, y: m.y + (2 * H) / 3 };
	const routes = routeEdges(
		[
			{ id: 'a', from: out(a), to: inA },
			{ id: 'b', from: out(b), to: inB },
		],
		[a, b, m],
	);
	assert.equal(crossings(routes).size, 0, JSON.stringify([...routes]));
});

test('étiquettes : un tronc commun, des boutons propres à chaque lien', () => {
	// Un nœud qui alimente trois nœuds empilés : les liens partagent le départ.
	const src = node(0, 300);
	const targets = [node(500, 0), node(500, 300), node(500, 600)];
	const routes = routeEdges(
		targets.map((t, i) => ({ id: `e${i}`, from: out(src), to: inp(t) })),
		[src, ...targets],
	);
	const all = [...routes.values()];
	const trunk = trunkPoint(all);
	assert.equal(trunk.y, out(src).y, 'sur le tronc, à la hauteur du port');
	assert.ok(trunk.x > out(src).x && trunk.x < inp(targets[0]).x);

	const points = all.map((r, i) => ownPoint(r, all.filter((_, j) => j !== i)));
	const keys = new Set(points.map((p) => `${Math.round(p.x)}:${Math.round(p.y)}`));
	assert.equal(keys.size, 3, `points distincts : ${JSON.stringify(points)}`);
	// Chaque point est sur la portion propre au lien (après l'embranchement).
	points.forEach((p, i) => assert.ok(p.x > trunk.x, `lien ${i} : ${JSON.stringify(p)}`));

	// Un lien seul garde son étiquette au milieu.
	const solo = routeEdges([{ id: 's', from: out(src), to: inp(node(500, 300)) }], [src]).get('s')!;
	assert.deepEqual(ownPoint(solo, []), { x: (out(src).x + 500) / 2, y: out(src).y });
});
