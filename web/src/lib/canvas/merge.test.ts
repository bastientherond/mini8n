import assert from 'node:assert/strict';
import { test } from 'node:test';
import { type MergeEdge, canonical, mergeGraph } from './merge.ts';

type N = { id: string; position: { x: number; y: number }; data: { name: string } };

const n = (id: string, x = 0, name = id): N => ({ id, position: { x, y: 0 }, data: { name } });
const e = (source: string, target: string): MergeEdge => ({ id: `${source}-${target}`, source, target, targetHandle: 'in-0' });
const g = (nodes: N[], edges: MergeEdge[] = [], name = 'wf') => ({ name, nodes, edges });
const ids = (xs: { id: string }[]) => xs.map((x) => x.id);

test('les clés sont triées et les valeurs absentes ignorées', () => {
	assert.equal(canonical({ b: 1, a: { d: [1, { f: 2, e: 3 }], c: undefined } }), '{"a":{"d":[1,{"e":3,"f":2}]},"b":1}');
});

test('modifications de nœuds différents : les deux sont conservées', () => {
	const base = g([n('a'), n('b')]);
	const local = g([n('a', 100), n('b')]);
	const remote = g([n('a'), n('b', 0, 'renommé')]);
	const out = mergeGraph(base, local, remote);
	assert.equal(out.nodes[0].position.x, 100);
	assert.equal(out.nodes[1].data.name, 'renommé');
});

test('même nœud modifié des deux côtés : la version locale l’emporte', () => {
	const out = mergeGraph(g([n('a')]), g([n('a', 10)]), g([n('a', 20)]));
	assert.equal(out.nodes[0].position.x, 10);
});

test('ajouts des deux côtés, suppression distante', () => {
	const base = g([n('a'), n('b')]);
	const local = g([n('a'), n('b'), n('local')]);
	const remote = g([n('a'), n('distant')]);
	assert.deepEqual(ids(mergeGraph(base, local, remote).nodes), ['a', 'distant', 'local']);
});

test('l’ordre des clés venant du serveur ne compte pas comme une modification', () => {
	const base = g([n('a', 5)]);
	const local = g([n('a', 50)]);
	const remote = g([{ data: { name: 'a' }, position: { y: 0, x: 5 }, id: 'a' }]);
	assert.equal(mergeGraph(base, local, remote).nodes[0].position.x, 50);
});

test('liens vers un nœud supprimé ailleurs, et boucles créées des deux côtés : retirés', () => {
	const base = g([n('a'), n('b'), n('c')]);
	const local = g([n('a'), n('b'), n('c')], [e('a', 'c'), e('b', 'a')]);
	const remote = g([n('a'), n('b')], [e('a', 'b')]);
	assert.deepEqual(ids(mergeGraph(base, local, remote).edges), ['a-b']);
});

test('nom : renommage local prioritaire, sinon le nom distant', () => {
	assert.equal(mergeGraph(g([], [], 'x'), g([], [], 'x'), g([], [], 'y')).name, 'y');
	assert.equal(mergeGraph(g([], [], 'x'), g([], [], 'z'), g([], [], 'y')).name, 'z');
});
