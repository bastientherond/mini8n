#!/usr/bin/env bash
# Test de charge de l'API (ApacheBench « ab » et curl requis).
#
#   EMAIL=… PASSWORD=… ./scripts/load-test.sh http://localhost:3000
#
# Crée un workspace temporaire « Test de charge » avec WORKFLOWS workflows actifs
# (webhook → formule → filtre → agrégat), puis mesure :
#   1. GET /api/health            (API + aller-retour PostgreSQL)
#   2. GET /api/workspaces        (requête authentifiée : session + base)
#   3. une rafale de webhooks     (HOOKS par workflow, en parallèle) : temps pour que toutes
#      les exécutions se terminent, refus éventuels (file pleine : 429), aucune perte.
# Le workspace est supprimé à la fin. À lancer contre un serveur de test, jamais en production
# (le cookie de session du compte de test apparaît dans la ligne de commande de ab).
set -euo pipefail

BASE="${1:?Usage : EMAIL=… PASSWORD=… $0 URL_API (ex. http://localhost:3000)}"
EMAIL="${EMAIL:?EMAIL requis (compte existant)}"
PASSWORD="${PASSWORD:?PASSWORD requis}"
WORKFLOWS="${WORKFLOWS:-20}"
HOOKS="${HOOKS:-12}"
REQUESTS="${REQUESTS:-2000}"
CONCURRENCY="${CONCURRENCY:-50}"
ROWS="${ROWS:-500}"

ORIGIN="$BASE"
JAR="$(mktemp)"
api() { # méthode chemin [corps JSON]
  curl -sf -X "$1" "$BASE/api$2" -b "$JAR" -c "$JAR" -H "Origin: $ORIGIN" -H 'Content-Type: application/json' ${3:+-d "$3"}
}
json() { python3 -c "import json,sys;print(json.load(sys.stdin)$1)"; }

WS=""
cleanup() { [ -z "$WS" ] || api DELETE "/workspaces/$WS" >/dev/null || true; rm -f "$JAR"; }
trap cleanup EXIT
# Identifiants transmis par l'entrée standard (jamais en argument, visible avec ps).
python3 -c 'import json,os;print(json.dumps({"email": os.environ["EMAIL"], "password": os.environ["PASSWORD"]}))' \
  | curl -sf -X POST "$BASE/api/auth/login" -b "$JAR" -c "$JAR" -H "Origin: $ORIGIN" \
      -H 'Content-Type: application/json' --data-binary @- >/dev/null \
  || { echo "Connexion impossible avec $EMAIL" >&2; exit 1; }
WS="$(api POST /workspaces '{"name":"Test de charge"}' | json "['id']")"

echo "== Préparation : $WORKFLOWS workflows actifs (webhook → formule → filtre → agrégat, $ROWS lignes)"
TOKENS=()
for i in $(seq 1 "$WORKFLOWS"); do
  token="charge$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n')"
  def=$(python3 - "$token" "$ROWS" <<'PY'
import json, sys
token, rows = sys.argv[1], int(sys.argv[2])
node = lambda i, kind, params: {"id": i, "position": {"x": 0, "y": 0}, "data": {"kind": kind, "name": i, "params": params}}
data = json.dumps([{"client": f"c{n % 50}", "montant": n % 997} for n in range(rows)])
print(json.dumps({"nodes": [
    node("hook", "trigger_webhook", {"token": token, "method": "POST", "respond": "immédiate", "test_data": "{}"}),
    node("data", "manual", {"data": data}),
    node("calc", "formula", {"column": "ttc", "expression": "($json.montant * 1.2).round(2)"}),
    node("gros", "filter", {"condition": "$json.ttc > 100"}),
    node("agg", "aggregate", {"group_by": "client", "aggregations": "count(*) as n, sum(ttc) as total"}),
], "edges": [
    {"id": "e1", "source": "hook", "target": "data", "targetHandle": "in-0"},
    {"id": "e2", "source": "data", "target": "calc", "targetHandle": "in-0"},
    {"id": "e3", "source": "calc", "target": "gros", "targetHandle": "in-0"},
    {"id": "e4", "source": "gros", "target": "agg", "targetHandle": "in-0"},
]}))
PY
)
  wf="$(api POST "/workspaces/$WS/workflows" "{\"name\":\"Charge $i\",\"definition\":$def}" | json "['id']")"
  api PUT "/workspaces/$WS/workflows/$wf/active" '{"active":true}' >/dev/null
  TOKENS+=("$token")
done

summary() { grep -E "Requests per second|Time per request.*\(mean\)|Failed requests|Non-2xx|50%|95%|99%" | sed 's/^/   /' || true; }

echo "== 1. GET /api/health ($REQUESTS requêtes, $CONCURRENCY en parallèle)"
ab -q -n "$REQUESTS" -c "$CONCURRENCY" "$BASE/api/health" | summary

echo "== 2. GET /api/workspaces authentifié"
COOKIE="$(awk '$6 ~ /session/ {print $6"="$7}' "$JAR" | tail -n 1)"
ab -q -n "$REQUESTS" -c "$CONCURRENCY" -C "$COOKIE" "$BASE/api/workspaces" | summary

echo "== 3. Rafale de webhooks : $WORKFLOWS × $HOOKS déclenchements en parallèle"
BODY="$(mktemp)"; echo '{"source":"test de charge"}' > "$BODY"
start=$(python3 -c 'import time;print(time.time())')
for t in "${TOKENS[@]}"; do
  ab -q -n "$HOOKS" -c 4 -p "$BODY" -T application/json "$BASE/api/hooks/$t" > "$BODY.$t" &
done
wait
accepted=0; refused=0
for t in "${TOKENS[@]}"; do
  total=$(awk '/Complete requests/ {print $3}' "$BODY.$t")
  bad=$(awk '/Non-2xx responses/ {print $3}' "$BODY.$t"); bad=${bad:-0}
  accepted=$((accepted + total - bad)); refused=$((refused + bad))
  rm -f "$BODY.$t"
done
rm -f "$BODY"
echo "   acceptés : $accepted, refusés : $refused (limites de débit ou file pleine)"
# Bilan de toutes les exécutions du workspace (pages de 200).
executions() {
  local before="" all="[]" page
  while :; do
    page="$(api GET "/workspaces/$WS/executions?limit=200${before:+&before=$before}")"
    all="$(python3 -c 'import json,sys;print(json.dumps(json.loads(sys.argv[1])+json.loads(sys.argv[2])))' "$all" "$page")"
    [ "$(echo "$page" | json '.__len__()')" -lt 200 ] && break
    before="$(echo "$page" | json '[-1]["startedAt"]' | python3 -c 'import sys,urllib.parse;print(urllib.parse.quote(sys.stdin.read().strip()))')"
  done
  echo "$all"
}
# Attente de la fin de toutes les exécutions acceptées.
while :; do
  read -r finished running < <(executions | python3 -c "
import json,sys
e=json.load(sys.stdin)
print(sum(x['status']!='running' for x in e), sum(x['status']=='running' for x in e))")
  [ "$running" = 0 ] && [ "$finished" -ge "$accepted" ] && break
  sleep 0.2
done
end=$(python3 -c 'import time;print(time.time())')
python3 -c "print(f'   toutes terminées en {$end - $start:.1f} s, soit {$accepted / ($end - $start):.0f} exécutions/s')"
executions | python3 -c "
import json,sys
e=json.load(sys.stdin)
d=sorted(x['durationMs'] or 0 for x in e)
ok=sum(x['status']=='success' for x in e)
print(f'   {len(e)} exécutions enregistrées pour {$accepted} acceptées, {ok} réussies ; durée médiane {d[len(d)//2]:.0f} ms, p95 {d[int(len(d)*0.95)]:.0f} ms, max {d[-1]:.0f} ms')"
