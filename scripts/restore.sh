#!/usr/bin/env sh
# Restaure une sauvegarde (fichier .dump produit par scripts/backup.sh ou le service backup).
#
#   ./scripts/restore.sh backups/mini8n-20260101-030000.dump
#
# La sauvegarde est restaurée dans une base neuve, puis échangée avec la base actuelle,
# conservée sous le nom mini8n_avant_<date> (retour arrière possible, à supprimer ensuite).
# Une restauration en échec ne modifie rien. L'API est arrêtée pendant l'échange, puis
# redémarrée (ses migrations mettent le schéma à jour si la sauvegarde est plus ancienne).
# La clé secrets/credentials_key.txt doit être celle en usage lors de la sauvegarde.
set -eu
cd "$(dirname "$0")/.."
FILE="${1:?Usage : $0 <fichier .dump>}"
[ -f "$FILE" ] || { echo "Fichier introuvable : $FILE" >&2; exit 1; }

printf 'Toutes les données actuelles seront remplacées par %s. Tapez « restaurer » : ' "$FILE"
read -r answer
[ "$answer" = "restaurer" ] || { echo "Abandon."; exit 1; }

sql() { docker compose exec -T db psql --username=mini8n --dbname=postgres -v ON_ERROR_STOP=1 -qAt -c "$1"; }
old="mini8n_avant_$(date -u +%Y%m%d_%H%M%S)_$$"

sql "DROP DATABASE IF EXISTS mini8n_restauration"
sql "CREATE DATABASE mini8n_restauration"
# Fichier transmis sur l'entrée standard : pas de montage, la base reste sur le réseau interne.
if ! docker compose exec -T db pg_restore --username=mini8n --dbname=mini8n_restauration \
  --no-owner --exit-on-error --single-transaction < "$FILE"; then
  sql "DROP DATABASE mini8n_restauration"
  echo "Échec de la restauration : rien n'a été modifié." >&2
  exit 1
fi

running="$(docker compose ps --status running --quiet server)"
[ -z "$running" ] || docker compose stop server
# Échange en une transaction : soit les deux renommages, soit aucun.
if ! docker compose exec -T db psql --username=mini8n --dbname=postgres -v ON_ERROR_STOP=1 -qAt <<SQL
SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = 'mini8n' AND pid <> pg_backend_pid();
BEGIN;
ALTER DATABASE mini8n RENAME TO $old;
ALTER DATABASE mini8n_restauration RENAME TO mini8n;
COMMIT;
SQL
then
  sql "DROP DATABASE IF EXISTS mini8n_restauration"
  [ -z "$running" ] || docker compose start server
  echo "Échec de l'échange des bases : rien n'a été modifié." >&2
  exit 1
fi
[ -z "$running" ] || docker compose start server
echo "Restauration terminée. Ancienne base conservée : $old"
echo "Après vérification : docker compose exec db dropdb -U mini8n $old"
