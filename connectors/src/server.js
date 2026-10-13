// Service interne « connectors » de mini8n : accès Oracle Database via node-oracledb
// en mode thin (pur JavaScript, open source, sans Oracle Instant Client).
//
// Sécurité :
// - joignable uniquement par l'API mini8n (réseau Docker interne), jeton partagé obligatoire ;
// - l'API résout et valide l'hôte (anti-SSRF) : ce service n'accepte qu'une adresse IP ;
// - identifiants SQL validés, valeurs toujours liées (jamais concaténées) ;
// - corps limités, délais de connexion et d'appel bornés.

import { createServer } from 'node:http';
import { readFileSync } from 'node:fs';
import { isIP } from 'node:net';
import { timingSafeEqual, createHash } from 'node:crypto';
import oracledb from 'oracledb';

const MAX_BODY = 25 * 1024 * 1024;
const MAX_ROWS = 100_000;
const CALL_TIMEOUT_MS = 120_000;

oracledb.fetchAsString = [oracledb.CLOB, oracledb.NCLOB];
oracledb.fetchAsBuffer = [oracledb.BLOB];

export function loadToken(env = process.env) {
  const token = env.CONNECTORS_TOKEN_FILE ? readFileSync(env.CONNECTORS_TOKEN_FILE, 'utf8').trim() : (env.CONNECTORS_TOKEN ?? '').trim();
  if (token.length < 32) throw new Error('CONNECTORS_TOKEN(_FILE) manquant ou trop court (32 caractères min.)');
  return token;
}

function sameSecret(a, b) {
  const ha = createHash('sha256').update(a).digest();
  const hb = createHash('sha256').update(b).digest();
  return timingSafeEqual(ha, hb);
}

class HttpError extends Error {
  constructor(status, message) {
    super(message);
    this.status = status;
  }
}

const IDENT = /^[A-Za-z][A-Za-z0-9_$#]{0,127}$/;

export function validTable(name) {
  const parts = String(name ?? '').split('.');
  return parts.length >= 1 && parts.length <= 2 && parts.every((p) => IDENT.test(p));
}

export function validColumn(name) {
  return IDENT.test(String(name ?? ''));
}

/** Paramètres de connexion : l'hôte doit être une adresse IP (validée en amont par l'API). */
export function connectionConfig(c) {
  if (!c || typeof c !== 'object') throw new HttpError(400, 'connexion manquante');
  if (!isIP(String(c.host))) throw new HttpError(400, 'hôte invalide : adresse IP attendue');
  const port = Number(c.port ?? 1521);
  if (!Number.isInteger(port) || port < 1 || port > 65535) throw new HttpError(400, 'port invalide');
  const service = String(c.service ?? '');
  if (!/^[A-Za-z0-9_.$#-]{1,128}$/.test(service)) throw new HttpError(400, 'nom de service invalide');
  const host = isIP(String(c.host)) === 6 ? `[${c.host}]` : c.host;
  return {
    user: String(c.user ?? ''),
    password: String(c.password ?? ''),
    connectString: `${host}:${port}/${service}`,
    connectTimeout: 15,
  };
}

/** Valeur Oracle → JSON. */
export function toJson(v) {
  if (v === null || v === undefined) return null;
  if (v instanceof Date) return Number.isNaN(v.getTime()) ? null : v.toISOString();
  if (Buffer.isBuffer(v)) return v.toString('base64');
  if (typeof v === 'bigint') return Number(v);
  return v;
}

async function withConnection(c, fn) {
  let conn;
  try {
    conn = await oracledb.getConnection(connectionConfig(c));
    conn.callTimeout = CALL_TIMEOUT_MS;
    return await fn(conn);
  } catch (e) {
    if (e instanceof HttpError) throw e;
    // ORA-xxxxx / NJS-xxx : message utile, sans pile.
    throw new HttpError(400, String(e?.message ?? e).split('\n')[0]);
  } finally {
    if (conn) await conn.close().catch(() => {});
  }
}

/**
 * Jeux de valeurs liées : un tableau par exécution (positionnels :1, :2…).
 * Uniquement des scalaires ; les booléens deviennent 1 / 0 (Oracle < 23ai).
 */
export function bindSets(binds) {
  if (binds === undefined || binds === null) return [[]];
  if (!Array.isArray(binds) || binds.length > 500 || !binds.every(Array.isArray)) {
    throw new HttpError(400, 'paramètres liés invalides');
  }
  return binds.map((set) =>
    set.map((v) => {
      if (v === null || typeof v === 'string' || typeof v === 'number') return v;
      if (typeof v === 'boolean') return v ? 1 : 0;
      throw new HttpError(400, 'paramètre lié invalide : valeur scalaire attendue');
    }),
  );
}

async function query(body) {
  const sql = String(body.sql ?? '').trim();
  if (!sql) throw new HttpError(400, 'requête vide');
  const maxRows = Math.min(Math.max(Number(body.maxRows) || 10_000, 1), MAX_ROWS);
  const sets = bindSets(body.binds);
  return withConnection(body.connection, async (conn) => {
    // Simulation : transaction en lecture seule, annulée à la fin (le serveur n'envoie
    // alors qu'une seule lecture SELECT, vérifiée avant l'appel).
    const readOnly = body.readOnly === true;
    if (readOnly) await conn.execute('SET TRANSACTION READ ONLY');
    let columns = [];
    const rows = [];
    // Une exécution par jeu de valeurs (mode « pour chaque ligne »), lignes réunies.
    for (const binds of sets) {
      if (rows.length >= maxRows) break;
      const res = await conn.execute(sql.replace(/;\s*$/, ''), binds, {
        outFormat: oracledb.OUT_FORMAT_ARRAY,
        maxRows: maxRows - rows.length,
      });
      if (columns.length === 0) columns = (res.metaData ?? []).map((m) => m.name);
      for (const r of res.rows ?? []) rows.push(r.map(toJson));
    }
    if (readOnly) await conn.rollback();
    return { columns, rows };
  });
}

/** Types de liaison déduits des valeurs (gère les colonnes commençant par null). */
export function bindDefs(columns, rows) {
  return columns.map((_, i) => {
    let type = null;
    let maxSize = 1;
    for (const row of rows) {
      const v = row[i];
      if (v === null || v === undefined) continue;
      if (typeof v === 'number' || typeof v === 'boolean') type ??= oracledb.NUMBER;
      else {
        type = oracledb.STRING;
        maxSize = Math.max(maxSize, Buffer.byteLength(String(v)));
      }
    }
    return type === oracledb.NUMBER ? { type } : { type: oracledb.STRING, maxSize: Math.min(Math.max(maxSize, 1), 32767) };
  });
}

/** Valeurs prêtes à lier : booléens → 1/0, objets → JSON, chaînes si la colonne est textuelle. */
export function bindRows(columns, rows, defs) {
  return rows.map((row) =>
    columns.map((_, i) => {
      const v = row[i];
      if (v === null || v === undefined) return null;
      if (typeof v === 'boolean') return v ? 1 : 0;
      if (typeof v === 'object') return JSON.stringify(v);
      return defs[i].type === oracledb.STRING ? String(v) : v;
    }),
  );
}

async function write(body) {
  const { table, columns, rows } = body;
  if (!validTable(table)) throw new HttpError(400, 'nom de table invalide');
  if (!Array.isArray(columns) || columns.length === 0 || !columns.every(validColumn)) {
    throw new HttpError(400, 'noms de colonnes invalides (lettres, chiffres, _)');
  }
  if (!Array.isArray(rows)) throw new HttpError(400, 'lignes manquantes');
  if (rows.length === 0) return { rows: 0 };
  const defs = bindDefs(columns, rows);
  const values = bindRows(columns, rows, defs);
  const sql = `INSERT INTO ${table} (${columns.join(', ')}) VALUES (${columns.map((_, i) => `:${i + 1}`).join(', ')})`;
  return withConnection(body.connection, async (conn) => {
    const res = await conn.executeMany(sql, values, { autoCommit: true, bindDefs: defs });
    return { rows: res.rowsAffected ?? values.length };
  });
}

async function test(body) {
  return withConnection(body.connection, async (conn) => {
    const res = await conn.execute(`SELECT banner FROM v$version WHERE ROWNUM = 1`).catch(() => null);
    return { version: res?.rows?.[0]?.[0] ?? `Oracle ${conn.oracleServerVersionString}` };
  });
}

const routes = { '/oracle/query': query, '/oracle/write': write, '/oracle/test': test };

function readBody(req) {
  return new Promise((resolve, reject) => {
    let size = 0;
    const chunks = [];
    req.on('data', (c) => {
      size += c.length;
      if (size > MAX_BODY) {
        reject(new HttpError(413, 'corps trop volumineux'));
        req.destroy();
      } else chunks.push(c);
    });
    req.on('end', () => {
      try {
        resolve(JSON.parse(Buffer.concat(chunks).toString('utf8') || '{}'));
      } catch {
        reject(new HttpError(400, 'JSON invalide'));
      }
    });
    req.on('error', reject);
  });
}

export function createApp(token) {
  return createServer(async (req, res) => {
    const send = (status, payload) => {
      res.writeHead(status, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' });
      res.end(JSON.stringify(payload));
    };
    try {
      if (req.method === 'GET' && req.url === '/healthz') return send(200, { status: 'ok' });
      const handler = routes[req.url];
      if (req.method !== 'POST' || !handler) throw new HttpError(404, 'introuvable');
      const auth = req.headers.authorization ?? '';
      if (!auth.startsWith('Bearer ') || !sameSecret(auth.slice(7), token)) throw new HttpError(401, 'jeton invalide');
      if (!String(req.headers['content-type'] ?? '').startsWith('application/json')) throw new HttpError(415, 'JSON attendu');
      send(200, await handler(await readBody(req)));
    } catch (e) {
      const status = e instanceof HttpError ? e.status : 500;
      if (status === 500) console.error(e);
      send(status, { error: status === 500 ? 'erreur interne' : e.message });
    }
  });
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const port = Number(process.env.PORT ?? 8090);
  const app = createApp(loadToken());
  app.headersTimeout = 30_000;
  app.requestTimeout = 180_000;
  app.listen(port, '0.0.0.0', () => console.log(`connecteurs mini8n à l'écoute sur ${port} (oracledb ${oracledb.versionString}, mode thin)`));
  for (const sig of ['SIGTERM', 'SIGINT']) process.on(sig, () => app.close(() => process.exit(0)));
}
