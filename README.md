# mini8n

Un clone minimaliste de [n8n](https://n8n.io) : éditeur de workflows visuel dans le navigateur,
moteur d'exécution en **Rust compilé en WebAssembly**, données échangées entre nœuds au format **Parquet**.

```
                 ┌──────────── navigateur ─────────────────────────────┐
                 │ Svelte 5 + shadcn-svelte + Svelte Flow  Web Worker  │
                 │ (comptes, workspaces, éditeur)  ⇄  moteur wasm      │
                 └───────────────┬─────────────────────────────────────┘
                                 │ HTTPS (profil tls : Caddy)
┌─ docker compose ───────────────▼─────────────────────────────────────────┐
│  proxy (Caddy, optionnel) ─▶ web (nginx non-root : SPA + /api, CSP…)     │
│                                  │ réseau « backend » (interne)          │
│                                  ▼                                       │
│                     server (API Rust axum, distroless)                   │
│                                  │ réseau « data » (interne)             │
│                                  ▼                                       │
│                     db (PostgreSQL 18)                                   │
│  engine : image de build (Rust → wasm) copiée dans web                   │
└──────────────────────────────────────────────────────────────────────────┘
```

## Démarrer avec Docker (recommandé)

```bash
./scripts/init-secrets.sh          # génère secrets/db_password.txt
cp .env.example .env               # optionnel : adapter la configuration
docker compose up -d --build
```

L'interface est servie sur http://localhost:8080 (uniquement en local par défaut).
Au premier accès, l'application demande de **créer un compte**, puis de **créer un workspace**.

Interface (inspirée de Supabase) :
- **barre du haut** : sélecteur de workspace (avec votre rôle) / sélecteur de workflow, recherche `⌘K`,
  aide, avatar ;
- **barre latérale** (se déploie au survol, épinglable) : Accueil, Éditeur, Workflows, Membres,
  Paramètres, Mon compte ;
- **menu de l'avatar** : compte et mot de passe, actions du workspace, changement de workspace,
  thème, palette de commandes, raccourcis, déconnexion (ou de tous les appareils) ;
- chaque page a sa propre URL (`#/w/<workspace>/workflows/<id>`), partageable entre membres.

| Conteneur | Image | Rôle |
| --- | --- | --- |
| `engine` | `crates/engine/Dockerfile` | build + tests du moteur, produit le `.wasm` (jamais exécuté) |
| `db` | `postgres:18-alpine` | données (comptes, sessions, workspaces, workflows) |
| `server` | `crates/server/Dockerfile` | API Rust, image distroless non-root |
| `web` | `web/Dockerfile` | nginx non-root : interface + reverse proxy `/api` |
| `proxy` | `caddy:2.11-alpine` | TLS automatique (profil `tls`) |

En production, avec un nom de domaine pointant vers le serveur :

```bash
# .env : SITE_ADDRESS=mini8n.exemple.com  et  PUBLIC_ORIGIN=https://mini8n.exemple.com
docker compose --profile tls up -d --build
```

## Développement local

Prérequis : Rust (rustup) avec la cible `wasm32-unknown-unknown`, le CLI `wasm-bindgen`
**de la même version** que la dépendance (0.2.129), Node.js et un PostgreSQL.

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129
# API (PostgreSQL local) — l'origine Vite doit être autorisée :
DATABASE_URL=postgres://… PUBLIC_ORIGIN=http://localhost:5173 BIND_ADDR=127.0.0.1:3000 cargo run -p mini8n-server
# Interface (proxy /api → :3000)
cd web && npm install && npm run dev
```

| Commande | Effet |
| --- | --- |
| `npm run build` (dans `web/`) | wasm release + build de production dans `web/dist` |
| `npm run wasm` / `npm run wasm:dev` | recompile seulement le moteur (release / debug) |
| `npm run check` | vérification TypeScript / Svelte |
| `cargo test` | tests du moteur et du serveur |
| `TEST_DATABASE_URL=postgres://… cargo test -p mini8n-server` | + tests d'intégration de l'API |

## Structure

```
crates/engine/          moteur Rust → wasm (workflow, nœuds, expressions, Parquet)
crates/server/          API Rust (axum + sqlx)
  migrations/           schéma PostgreSQL
  src/auth.rs           inscription, connexion, sessions, mot de passe
  src/workspaces.rs     workspaces, membres, rôles
  src/workflows.rs      stockage des workflows (version optimiste)
  src/triggers.rs       activation, webhooks et formulaires publics, exécution à distance
  src/runner.rs         exécution côté serveur (pool de threads, sous-workflows, erreurs)
  src/scheduler.rs      planification et surveillance d'API
  src/schedule.rs       intervalle / quotidien / hebdomadaire / cron, fuseaux horaires
  src/net.rs            client HTTP sortant anti-SSRF
  src/executions.rs     historique des exécutions
  src/security.rs       anti-CSRF, limitation de débit, IP cliente
  tests/api.rs          tests d'intégration (PostgreSQL)
web/                    interface Svelte 5 + Vite + Tailwind 4 + shadcn-svelte
  nginx/                configuration nginx (en-têtes de sécurité, proxy)
  src/lib/api.ts        client API ; session.svelte.ts : compte et workspaces
  src/lib/router.svelte.ts  routes (#/w/<workspace>/workflows/<id>…)
  src/lib/engine/       worker WASM, client RPC
  src/lib/components/
    shell/              barre du haut, barre latérale, menu avatar, palette ⌘K, dialogues
    pages/              accueil, workflows, membres, paramètres, compte
    Editor.svelte       éditeur de workflow (canevas, palette de nœuds, inspecteur, données)
deploy/Caddyfile        reverse proxy TLS
compose.yaml            orchestration
```

## Sécurité

**Comptes et sessions**
- Mots de passe hachés en **Argon2id** (19 Mio, t=2, p=1, recommandations OWASP), 12 caractères minimum,
  calcul hors de la boucle async et borné en concurrence.
- Session : jeton aléatoire de 256 bits dans un cookie `HttpOnly; SameSite=Strict`, en plus
  `Secure` avec le préfixe `__Host-` dès que le site est servi en HTTPS (`COOKIE_SECURE=auto`,
  car Safari refuse les cookies `Secure` sur `http://localhost`) ;
  seul son hash SHA-256 est en base. Nouveau jeton à chaque connexion, expiration, 20 sessions max.
  par compte, révocation de toutes les autres sessions au changement de mot de passe.
- Connexion : même message et même temps de réponse pour un compte inexistant ou un mauvais
  mot de passe ; limitation par IP et par compte ; blocage progressif après 5 échecs.
- Inscriptions désactivables (`ALLOW_REGISTRATION=false`).

**API**
- Anti-CSRF : cookie `SameSite=Strict`, en-tête `Origin` obligatoire et égal à `PUBLIC_ORIGIN`
  pour toute requête modifiant des données, corps JSON obligatoire.
- Contrôle d'accès sur chaque route : un non-membre reçoit 404 (pas de fuite d'existence),
  les actions d'administration sont réservées aux propriétaires, un workspace garde toujours un
  propriétaire.
- Entrées validées (champs inconnus refusés, tailles bornées), corps limités à 2 Mo,
  délai max. de 30 s par requête, aucune erreur interne exposée, requêtes SQL paramétrées.

**Déclencheurs**
- Webhooks et formulaires identifiés par un jeton aléatoire de 192 bits (régénérable), actifs
  seulement si le workflow l'est ; secret de webhook comparé en temps constant ; limitation
  de débit par jeton et par IP ; corps limités à 1 Mo ; formulaires soumis à la vérification
  d'origine et validés champ par champ.
- Exécutions bornées (file, délai, profondeur de sous-workflows), paniques du moteur confinées,
  pas de boucle entre workflows « Sur erreur ».

**Navigateur**
- CSP stricte : aucun script inline, `wasm-unsafe-eval` seul, pas d'iframe (`frame-ancestors 'none'`),
  `connect-src` configurable (`CSP_CONNECT_SRC`) pour le nœud HTTP.
- COOP/COEP/CORP, `nosniff`, `Referrer-Policy: no-referrer`, `Permissions-Policy` restrictive, HSTS (profil tls).
- Le nœud HTTP n'envoie jamais de cookies (`credentials: omit`), n'accepte que des URL http(s)
  et limite la taille des réponses.
- Import de workflow validé et reconstruit ; export CSV protégé contre l'injection de formules ;
  en-têtes sensibles masqués à l'export JSON ; expressions bornées (longueur, profondeur) ;
  le moteur tourne dans un Worker redémarré en cas de blocage.

**Conteneurs**
- Images minimales (distroless, alpine), utilisateurs non-root, système de fichiers en lecture seule,
  `cap_drop: ALL`, `no-new-privileges`, limites mémoire / processus.
- Base de données sur un réseau Docker **interne** ; l'API n'a qu'un accès **sortant** (requêtes
  HTTP des workflows, filtrées contre le SSRF) ; seul nginx (ou Caddy) est exposé, sur
  `127.0.0.1` par défaut.
- Mot de passe PostgreSQL en secret Docker (fichier hors dépôt), authentification SCRAM-SHA-256.
- `npm ci --ignore-scripts`, `Cargo.lock` respecté (`--locked`), tests exécutés pendant le build.

## Données entre nœuds : Parquet

Chaque nœud produit une table Apache Arrow, **sérialisée en Parquet** (compression Snappy).
Les nœuds suivants relisent ce Parquet ; l'interface reçoit aussi ces octets pour
l'aperçu paginé et l'export (`.parquet`, `.csv`, `.json`). Plusieurs liens vers une même
entrée sont empilés (union des colonnes).

## Déclencheurs

Un workflow démarre par un ou plusieurs déclencheurs. Dans l'éditeur, « Exécuter » lance le
workflow dans le navigateur avec les **données de test** de chaque déclencheur. Pour les
déclencheurs automatiques, il faut **activer** le workflow (interrupteur « Actif ») : il est alors
exécuté **côté serveur**, et chaque exécution apparaît dans **Exécutions**.

| Déclencheur | Démarre le workflow… | Données produites |
| --- | --- | --- |
| Déclenchement manuel | bouton Exécuter (éditeur) ou « Exécuter sur le serveur » | `declenche_le` |
| Planification | toutes les N minutes, chaque jour / certains jours à HH:MM, ou expression cron (5 champs), dans le fuseau choisi | `declenche_le`, `planifie_pour` |
| Webhook | à chaque requête sur `/api/hooks/<jeton>` (méthode, secret `X-Mini8n-Secret` facultatif) ; réponse immédiate (202) ou résultat du workflow en JSON | corps JSON (objet ou tableau), formulaire encodé ou texte, `_methode`, `_recu_le` |
| Formulaire | à chaque envoi d'une page publique `/#/f/<jeton>` (champs texte, texte long, e-mail, nombre, date, case ; validés côté serveur) | une ligne par réponse + `soumis_le` |
| Surveillance d'API | quand une API JSON interrogée toutes les N minutes renvoie de **nouveaux** éléments (repérés par une colonne identifiante) | les nouveaux éléments seulement |
| Appelé par un workflow | quand un autre workflow l'exécute avec le nœud « Exécuter un workflow » | la table envoyée par l'appelant |
| Sur erreur | quand une exécution automatique d'un autre workflow du workspace échoue | `workflow`, `noeud`, `erreur`, `execution_id`, `date` |

Le nœud **Exécuter un workflow** envoie sa table d'entrée à un autre workflow et récupère sa
sortie (son dernier nœud terminal exécuté avec succès). Il fonctionne dans l'éditeur comme sur le
serveur, jusqu'à 5 niveaux d'imbrication.

Côté serveur : pool de threads dédié (`RUNNER_THREADS`), file bornée, délai maximal par exécution
(`EXECUTION_TIMEOUT_SECS`), historique conservé `EXECUTIONS_RETENTION_DAYS` jours. Les requêtes
HTTP des workflows (nœud HTTP, surveillance) ne peuvent joindre que des **adresses publiques**
(protection SSRF : boucle locale, réseaux privés et Docker, métadonnées cloud, redirections
vers ces adresses… sont refusés).

## Nœuds disponibles

| Catégorie | Nœud | Rôle |
| --- | --- | --- |
| Source | Données JSON | tableau d'objets saisi à la main (objets imbriqués aplatis en `a.b`) |
| Source | CSV | contenu CSV collé, types inférés |
| Source | Requête HTTP | `fetch` d'une API JSON (chemin vers le tableau configurable) |
| Transformation | Filtrer | garde les lignes vérifiant une expression |
| Transformation | Formule | ajoute / remplace une colonne calculée |
| Transformation | Colonnes | garder / retirer des colonnes |
| Transformation | Renommer | `ancien -> nouveau` |
| Transformation | Trier | `prix desc, nom` |
| Transformation | Limiter | nombre + décalage |
| Transformation | Dédoublonner | sur toutes les colonnes ou certaines |
| Transformation | Agréger | `group by` + `count, count_distinct, sum, avg, min, max, first, last` |
| Combinaison | Fusionner | empiler, jointure interne, jointure gauche |
| Flux | Exécuter un workflow | appelle un autre workflow du workspace et récupère sa sortie |

Un nœud **désactivé** transmet son entrée telle quelle. « Exécuter jusqu'ici » n'exécute
que le nœud et ses ancêtres.

## Expressions

```
prix * quantite > 100 && categorie == "livres"
upper(trim(nom)) + " (" + coalesce(string(age), "?") + ")"
if(stock > 0, "dispo", "rupture")
`code postal` == "75001"
```

Opérateurs `+ - * / %`, `== != < <= > >=`, `&& || !` (ou `and or not`).
Fonctions : `upper lower trim len concat contains starts_with ends_with replace substr
round floor ceil abs sqrt pow min max coalesce if is_null number string`.

## Bibliothèques (toutes open source)

| Bibliothèque | Licence | Usage |
| --- | --- | --- |
| [arrow-rs / parquet](https://github.com/apache/arrow-rs) | Apache-2.0 | tables en mémoire, Parquet, CSV, JSON |
| [wasm-bindgen](https://github.com/wasm-bindgen/wasm-bindgen), js-sys, web-sys | MIT / Apache-2.0 | pont Rust ↔ JS |
| serde, serde_json, thiserror, bytes | MIT / Apache-2.0 | sérialisation, erreurs |
| [Svelte 5](https://svelte.dev) + Vite | MIT | interface |
| [Svelte Flow](https://svelteflow.dev) (`@xyflow/svelte`) | MIT | canevas de nœuds |
| [shadcn-svelte](https://shadcn-svelte.com) + bits-ui | MIT | composants UI |
| Tailwind CSS, clsx, tailwind-merge, Lucide | MIT / ISC | styles, icônes |
| [axum](https://github.com/tokio-rs/axum), tokio, tower-http | MIT | API HTTP |
| [sqlx](https://github.com/launchbadge/sqlx) | MIT / Apache-2.0 | PostgreSQL, migrations |
| argon2, sha2, getrandom (RustCrypto) | MIT / Apache-2.0 | hachage, jetons |
| [reqwest](https://github.com/seanmonstar/reqwest) (rustls) | MIT / Apache-2.0 | requêtes HTTP des workflows côté serveur |
| chrono, chrono-tz | MIT / Apache-2.0 | planification, fuseaux horaires |
| PostgreSQL, nginx, Caddy | PostgreSQL / BSD-2 / Apache-2.0 | base, serveur web, TLS |

## Limites actuelles / pistes

- Pas encore de vérification d'e-mail ni de réinitialisation de mot de passe par e-mail.
- Les limites de débit de l'API sont en mémoire (une seule instance de `server`).
- Le nœud HTTP exécuté dans l'éditeur est soumis au CORS du navigateur (pas côté serveur).
- Pas encore de déclencheurs e-mail (IMAP) ni flux RSS/XML.
- Le `.wasm` fait ~8 Mo (~1,8 Mo gzip), surtout à cause des noyaux Arrow.
