import assert from 'node:assert/strict';
import { test } from 'node:test';
import { type GraphEdge, removeNodesKeepingLinks } from './graph.ts';

let seq = 0;
const make = (source: string, target: string, targetHandle: string): GraphEdge => ({ id: `n${++seq}`, source, target, targetHandle });
const e = (source: string, target: string, targetHandle = 'in-0'): GraphEdge => ({ id: `${source}-${target}-${targetHandle}`, source, target, targetHandle });
const pairs = (edges: GraphEdge[]) => edges.map((x) => `${x.source}>${x.target}:${x.targetHandle}`).sort();

test('supprimer le nœud du milieu relie ses voisins', () => {
	const out = removeNodesKeepingLinks([e('trigger', 'fichier'), e('fichier', 'mail')], ['fichier'], make);
	assert.deepEqual(pairs(out), ['trigger>mail:in-0']);
});

test('une chaîne de nœuds supprimés est enjambée', () => {
	const out = removeNodesKeepingLinks([e('a', 'b'), e('b', 'c'), e('c', 'd')], ['b', 'c'], make);
	assert.deepEqual(pairs(out), ['a>d:in-0']);
});

test("l'entrée visée est conservée et les branches sont toutes reliées", () => {
	const out = removeNodesKeepingLinks(
		[e('src', 'x'), e('x', 'm', 'in-1'), e('x', 'y'), e('autre', 'm')],
		['x'],
		make,
	);
	assert.deepEqual(pairs(out), ['autre>m:in-0', 'src>m:in-1', 'src>y:in-0']);
});

test('nœud en bout de chaîne ou sans entrée : simple suppression', () => {
	assert.deepEqual(pairs(removeNodesKeepingLinks([e('a', 'b')], ['b'], make)), []);
	assert.deepEqual(pairs(removeNodesKeepingLinks([e('a', 'b')], ['a'], make)), []);
});

test('fusion supprimée : seule son entrée principale est reportée, pas de doublon', () => {
	const out = removeNodesKeepingLinks(
		[e('a', 'm'), e('b', 'm', 'in-1'), e('m', 'z'), e('a', 'z')],
		['m'],
		make,
	);
	assert.deepEqual(pairs(out), ['a>z:in-0']);
});
