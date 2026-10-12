import { test } from 'node:test';
import assert from 'node:assert/strict';
import oracledb from 'oracledb';
import { bindDefs, bindRows, connectionConfig, createApp, toJson, validColumn, validTable } from '../src/server.js';

const TOKEN = 'x'.repeat(40);

async function withServer(fn) {
  const app = createApp(TOKEN);
  await new Promise((r) => app.listen(0, '127.0.0.1', r));
  const base = `http://127.0.0.1:${app.address().port}`;
  try {
    await fn(base);
  } finally {
    await new Promise((r) => app.close(r));
  }
}

test('identifiants SQL', () => {
  assert.ok(validTable('COMMANDES'));
  assert.ok(validTable('VENTES.COMMANDES_2026'));
  for (const bad of ['', 'a.b.c', '1T', 'T; DROP TABLE X', 'T--', '"T"']) assert.ok(!validTable(bad), bad);
  assert.ok(validColumn('PRIX_HT'));
  assert.ok(!validColumn('prix ht'));
});

test("l'hôte doit être une adresse IP", () => {
  assert.equal(connectionConfig({ host: '10.0.0.5', port: 1521, service: 'FREEPDB1', user: 'u', password: 'p' }).connectString, '10.0.0.5:1521/FREEPDB1');
  assert.equal(connectionConfig({ host: '::1', port: 1521, service: 'X' }).connectString, '[::1]:1521/X');
  assert.throws(() => connectionConfig({ host: 'db.interne', port: 1521, service: 'X' }), /adresse IP/);
  assert.throws(() => connectionConfig({ host: '10.0.0.5', port: 99999, service: 'X' }), /port/);
  assert.throws(() => connectionConfig({ host: '10.0.0.5', port: 1521, service: 'X)(HOST=evil' }), /service/);
});

test('liaisons typées', () => {
  const cols = ['A', 'B', 'C'];
  const rows = [[null, 1, true], ['texte long', null, false]];
  const defs = bindDefs(cols, rows);
  assert.equal(defs[0].type, oracledb.STRING);
  assert.equal(defs[0].maxSize, 10);
  assert.equal(defs[1].type, oracledb.NUMBER);
  assert.deepEqual(bindRows(cols, rows, defs), [[null, 1, 1], ['texte long', null, 0]]);
  assert.equal(toJson(new Date('2026-10-12T08:00:00Z')), '2026-10-12T08:00:00.000Z');
});

test('authentification et routes', async () => {
  await withServer(async (base) => {
    assert.equal((await fetch(`${base}/healthz`)).status, 200);
    const post = (path, headers = {}) =>
      fetch(`${base}${path}`, { method: 'POST', headers: { 'Content-Type': 'application/json', ...headers }, body: '{}' });
    assert.equal((await post('/oracle/query')).status, 401);
    assert.equal((await post('/oracle/query', { Authorization: 'Bearer faux' })).status, 401);
    assert.equal((await post('/inconnu', { Authorization: `Bearer ${TOKEN}` })).status, 404);
    const r = await post('/oracle/query', { Authorization: `Bearer ${TOKEN}` });
    assert.equal(r.status, 400);
    assert.match((await r.json()).error, /requête vide/);
  });
});
