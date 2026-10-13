#!/usr/bin/env sh
# Sauvegarde immédiate de la base, copiée sur la machine hôte (pour une copie hors site).
#
#   ./scripts/backup.sh [dossier]     (défaut : ./backups)
#
# La clé de chiffrement des identifiants (secrets/credentials_key.txt) n'est PAS incluse :
# conservez-la ailleurs (gestionnaire de mots de passe…). Sans elle, les identifiants
# d'une sauvegarde restaurée sont illisibles.
set -eu
cd "$(dirname "$0")/.."
DEST="${1:-backups}"
umask 077
mkdir -p "$DEST"

docker compose run --rm backup once
latest="$(docker compose run --rm --no-deps --entrypoint sh backup -c 'ls -1 /var/lib/postgresql/mini8n-*.dump | sort | tail -n 1')"
latest="$(printf '%s' "$latest" | tr -d '\r')"
[ -n "$latest" ] || { echo "Aucune sauvegarde trouvée" >&2; exit 1; }
# Copie depuis le volume, via un conteneur éphémère (le service « backup » peut être arrêté).
docker compose run --rm --no-deps --entrypoint cat backup "$latest" > "$DEST/$(basename "$latest")"
echo "Sauvegarde copiée : $DEST/$(basename "$latest")"
