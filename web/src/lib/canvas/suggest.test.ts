import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { NodeSpec } from '../engine/types';
import { propose } from './suggest.ts';

const spec = (kind: string, category: string, inputs: string[] = ['Entrée']): NodeSpec =>
	({ kind, label: kind, category, description: '', icon: '', inputs, params: [] }) as unknown as NodeSpec;

const catalog = [
	spec('trigger_webhook', 'trigger', []),
	spec('trigger_schedule', 'trigger', []),
	spec('csv', 'source', []),
	spec('postgres_query', 'integration', []),
	spec('filter', 'transform'),
	spec('formula', 'transform'),
	spec('sort', 'transform'),
	spec('aggregate', 'transform'),
	spec('merge', 'combine', ['A', 'B']),
	spec('email_send', 'integration'),
	spec('chat_message', 'integration'),
	spec('postgres_write', 'integration'),
];
const kinds = (xs: NodeSpec[]) => xs.map((s) => s.kind);
const find = (k: string) => catalog.find((s) => s.kind === k);

test('après un nœud : seulement ce qui a une entrée, jamais un déclencheur', () => {
	const { candidates } = propose(catalog, find('csv'), 'after');
	assert.ok(candidates.every((s) => s.inputs.length > 0 && s.category !== 'trigger'));
	assert.ok(!kinds(candidates).includes('postgres_query'));
});

test('les suggestions dépendent de l’étape précédente', () => {
	assert.deepEqual(kinds(propose(catalog, find('trigger_webhook'), 'after').suggested).slice(0, 3), ['filter', 'formula', 'chat_message']);
	assert.deepEqual(kinds(propose(catalog, find('postgres_write'), 'after').suggested), ['chat_message', 'email_send']);
	// Type inconnu de la table : la catégorie décide.
	assert.equal(propose(catalog, find('csv'), 'after').suggested[0].kind, 'filter');
});

test('avant un nœud : sources et déclencheurs proposés, tout est branchable', () => {
	const { suggested, candidates } = propose(catalog, find('filter'), 'before');
	assert.equal(candidates.length, catalog.length);
	assert.deepEqual(kinds(suggested), ['csv', 'postgres_query', 'trigger_webhook', 'trigger_schedule']);
	assert.equal(propose(catalog, find('email_send'), 'before').suggested[0].kind, 'aggregate');
});

test('au plus cinq suggestions, sans doublon', () => {
	const { suggested } = propose(catalog, find('filter'), 'after');
	assert.ok(suggested.length <= 5);
	assert.equal(new Set(kinds(suggested)).size, suggested.length);
});
