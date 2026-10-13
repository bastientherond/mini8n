import assert from 'node:assert/strict';
import { test } from 'node:test';
import { layered } from './layout.ts';

const n = (id: string, y = 0) => ({ id, w: 240, h: 94, y });

test('une chaîne est alignée sur une seule ligne', () => {
	const pos = layered([n('a', 300), n('b', 0), n('c', 120)], [
		{ source: 'a', target: 'b' },
		{ source: 'b', target: 'c' },
	]);
	const ys = ['a', 'b', 'c'].map((id) => pos.get(id)!.y);
	assert.equal(new Set(ys).size, 1);
	const xs = ['a', 'b', 'c'].map((id) => pos.get(id)!.x);
	assert.ok(xs[0] < xs[1] && xs[1] < xs[2]);
});

test('branches symétriques, fusion alignée au milieu, positions sur la grille', () => {
	const pos = layered([n('src'), n('x'), n('y'), n('m')], [
		{ source: 'src', target: 'x' },
		{ source: 'src', target: 'y' },
		{ source: 'x', target: 'm' },
		{ source: 'y', target: 'm' },
	]);
	const c = (id: string) => pos.get(id)!.y + 47;
	assert.ok(Math.abs(c('src') - c('m')) <= 10);
	assert.ok(Math.abs((c('x') + c('y')) / 2 - c('src')) <= 10);
	assert.ok(c('y') - c('x') >= 94 + 60);
	for (const p of pos.values()) assert.ok(p.x % 10 === 0 && p.y % 10 === 0);
});

test('nœuds isolés et cycles ne bloquent pas', () => {
	const pos = layered([n('a'), n('b'), n('seul')], [
		{ source: 'a', target: 'b' },
		{ source: 'b', target: 'a' },
	]);
	assert.equal(pos.size, 3);
});

test("l'entrée A est alimentée par le nœud du haut", () => {
	const pos = layered([n('src'), n('x'), n('y'), n('m')], [
		{ source: 'src', target: 'x' },
		{ source: 'src', target: 'y' },
		{ source: 'x', target: 'm', input: 1 },
		{ source: 'y', target: 'm', input: 0 },
	]);
	assert.ok(pos.get('y')!.y < pos.get('x')!.y);
});
