#!/usr/bin/env sh
# Génère les secrets nécessaires à docker compose (une seule fois).
set -eu
cd "$(dirname "$0")/.."
umask 077
mkdir -p secrets
chmod 700 secrets
if [ ! -s secrets/db_password.txt ]; then
  # 48 caractères aléatoires (base64 sans caractères ambigus pour une URL).
  head -c 36 /dev/urandom | base64 | tr '+/' '-_' > secrets/db_password.txt
  echo "secrets/db_password.txt créé"
else
  echo "secrets/db_password.txt existe déjà (inchangé)"
fi
# Lisible par l'utilisateur des conteneurs ; le dossier parent reste privé (700).
chmod 644 secrets/db_password.txt
