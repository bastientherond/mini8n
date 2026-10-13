#!/bin/sh
# Sauvegarde périodique de PostgreSQL (service « backup » de compose.yaml).
#
#   backup.sh          boucle : une sauvegarde toutes les BACKUP_INTERVAL_HOURS heures
#   backup.sh once     une sauvegarde immédiate
#
# Format personnalisé de pg_dump (compressé, restaurable table par table), vérifié par
# pg_restore --list ; les BACKUP_KEEP plus récentes sont conservées.
set -eu

DIR="${BACKUP_DIR:-/var/lib/postgresql}"
KEEP="${BACKUP_KEEP:-14}"
INTERVAL="${BACKUP_INTERVAL_HOURS:-24}"
PGPASSWORD="$(cat /run/secrets/db_password)"
export PGPASSWORD

log() { echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) backup: $*"; }

backup() {
  name="mini8n-$(date -u +%Y%m%d-%H%M%S).dump"
  tmp="$DIR/.$name.partial"
  if ! pg_dump -h "$PGHOST" -U "$PGUSER" -d "$PGDATABASE" --format=custom --compress=6 --file="$tmp"; then
    rm -f "$tmp"
    log "ÉCHEC de pg_dump"
    return 1
  fi
  # Une sauvegarde illisible ne remplace jamais les précédentes.
  if ! pg_restore --list "$tmp" >/dev/null; then
    rm -f "$tmp"
    log "ÉCHEC : sauvegarde illisible"
    return 1
  fi
  mv "$tmp" "$DIR/$name"
  log "$name ($(du -h "$DIR/$name" | cut -f1))"
  # Rotation : les KEEP plus récentes (noms horodatés, tri lexical = chronologique).
  ls -1 "$DIR"/mini8n-*.dump 2>/dev/null | sort -r | tail -n +"$((KEEP + 1))" | while read -r old; do
    rm -f "$old"
    log "supprimée : $(basename "$old")"
  done
}

case "$INTERVAL" in
  ''|*[!0-9]*) log "BACKUP_INTERVAL_HOURS invalide : $INTERVAL"; exit 1 ;;
esac
case "$KEEP" in
  ''|*[!0-9]*|0) log "BACKUP_KEEP invalide : $KEEP"; exit 1 ;;
esac

if [ "${1:-}" = "once" ]; then
  backup
  exit
fi

log "toutes les $INTERVAL h, $KEEP conservées"
while true; do
  backup || true
  sleep "$((INTERVAL * 3600))"
done
