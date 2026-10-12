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
# Clé de chiffrement des identifiants (32 octets, base64) : à sauvegarder, sa perte rend
# les identifiants enregistrés illisibles.
if [ ! -s secrets/credentials_key.txt ]; then
  head -c 32 /dev/urandom | base64 > secrets/credentials_key.txt
  echo "secrets/credentials_key.txt créé (à sauvegarder)"
fi
# Jeton partagé entre l'API et le service connectors.
if [ ! -s secrets/connectors_token.txt ]; then
  head -c 36 /dev/urandom | base64 | tr '+/' '-_' > secrets/connectors_token.txt
  echo "secrets/connectors_token.txt créé"
fi
# Lisibles par l'utilisateur des conteneurs ; le dossier parent reste privé (700).
chmod 644 secrets/*.txt
