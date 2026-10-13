# mini8n — documentation technique

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
./scripts/init-secrets.sh          # génère les secrets (base, clé des identifiants, jeton connecteurs)
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
| `connectors` | `connectors/Dockerfile` | Node non-root : pilote Oracle « thin » (node-oracledb), joignable par l'API seulement |
| `proxy` | `caddy:2.11-alpine` | TLS automatique (profil `tls`) |
| `backup` | `postgres:18-alpine` | sauvegarde périodique de la base (`pg_dump`, volume `db-backups`) |

En production, avec un nom de domaine pointant vers le serveur :

```bash
# .env : SITE_ADDRESS=mini8n.exemple.com  et  PUBLIC_ORIGIN=https://mini8n.exemple.com
docker compose --profile tls up -d --build
```

## Sauvegardes et restauration

Le service `backup` sauvegarde la base toutes les `BACKUP_INTERVAL_HOURS` heures (24 par
défaut) dans le volume `db-backups` et garde les `BACKUP_KEEP` plus récentes (14). Chaque
sauvegarde est vérifiée (`pg_restore --list`) avant de remplacer les précédentes ; un échec
apparaît dans `docker compose logs backup`.

```bash
./scripts/backup.sh              # sauvegarde immédiate, copiée dans ./backups (à envoyer hors site)
./scripts/restore.sh backups/mini8n-20260101-030000.dump
```

- **Hors site** : copiez régulièrement `./backups` ailleurs (autre machine, stockage objet) —
  un volume Docker disparaît avec le serveur.
- **Clé des identifiants** : `secrets/credentials_key.txt` n'est volontairement **pas** dans
  les sauvegardes. Conservez-la à part (gestionnaire de mots de passe) : sans elle, les
  identifiants d'une base restaurée sont illisibles ; avec elle seule, une sauvegarde volée
  ne révèle pas les mots de passe des bases et du SMTP.
- **Restauration** : la sauvegarde est chargée dans une base neuve, puis échangée en une
  transaction avec la base actuelle, conservée sous `mini8n_avant_<date>` pour un retour
  arrière. Un fichier invalide ou incomplet ne modifie rien. L'API est arrêtée pendant
  l'échange ; ses migrations mettent le schéma à jour au redémarrage.
- **Tester** : restaurez une sauvegarde de temps en temps (par exemple sur une machine de
  test) — une sauvegarde jamais restaurée n'est qu'une hypothèse.

## Mises à jour et retour arrière

En production, on déploie **les images construites par la CI**, jamais une copie de travail
(modifications non enregistrées, dépendances locales…). Le workflow `Release` publie sur
GitHub Container Registry, après analyse des vulnérabilités (Trivy : aucune faille critique ou
élevée corrigeable) :

- tag `vX.Y.Z` → images `ghcr.io/<compte>/mini8n/{server,web,connectors}:X.Y.Z` ;
- branche `main` → tag `main` (préproduction).

```bash
# .env : MINI8N_REGISTRY=ghcr.io/<compte>/mini8n   MINI8N_VERSION=1.2.0
./scripts/backup.sh                       # toujours une sauvegarde avant une mise à jour
docker compose pull
docker compose up -d --no-build
```

**Retour arrière** : remettre la version précédente dans `MINI8N_VERSION`, puis `pull` et
`up -d --no-build`. Les migrations de schéma ne sont appliquées que vers l'avant : si la
nouvelle version a modifié le schéma, restaurer aussi la sauvegarde faite juste avant
(`./scripts/restore.sh`). Publier une version : `git tag v1.2.0 && git push origin v1.2.0`.

**Dépendances** : le workflow `Audit des dépendances` vérifie, à chaque modification et chaque
semaine, les vulnérabilités connues (RustSec via `cargo deny`, `npm audit`), les licences et
la provenance des crates (`deny.toml` : toute exception y est justifiée). Dependabot propose
chaque semaine les mises à jour (crates, npm, images Docker, actions GitHub).

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
| `TEST_MYSQL=hôte:port:base:user:mdp`, `TEST_ORACLE=…`, `TEST_CONNECTORS_URL`, `TEST_CONNECTORS_TOKEN` | + tests MySQL et Oracle |
| `npm test` (dans `connectors/`) | tests du connecteur Oracle |

## Structure

```
crates/engine/          moteur Rust → wasm (workflow, nœuds, expressions, Parquet)
crates/server/          API Rust (axum + sqlx)
  migrations/           schéma PostgreSQL
  src/auth.rs           inscription, connexion, sessions, mot de passe
  src/accounts.rs       mot de passe oublié, vérification d'adresse, invitations
  src/mailer.rs         e-mails de la plateforme (SMTP_HOST…)
  src/workspaces.rs     workspaces, membres, rôles
  src/workflows.rs      stockage des workflows (version optimiste)
  src/live.rs           collaboration en direct (WebSocket : présence, sauvegardes diffusées)
  src/mcp/              serveur MCP : IA connectées (outils, édition du graphe, présence)
  src/tokens.rs         jetons d'accès personnels des IA
  src/triggers.rs       activation, webhooks et formulaires publics, exécution à distance
  src/runner.rs         exécution côté serveur (pool de threads, sous-workflows, erreurs)
  src/scheduler.rs      planification et surveillance d'API
  src/schedule.rs       intervalle / quotidien / hebdomadaire / cron, fuseaux horaires
  src/net.rs            client HTTP sortant anti-SSRF, politique réseau (liste blanche)
  src/credentials.rs    identifiants chiffrés ; src/crypto.rs : AES-256-GCM
  src/files.rs          fichiers du workspace
  src/integrations/     PostgreSQL / MySQL (sqlx), Oracle (connecteur), fichiers, e-mail, messageries
  src/executions.rs     historique des exécutions
  src/security.rs       anti-CSRF, limitation de débit, IP cliente
  tests/api.rs          tests d'intégration (PostgreSQL) ; tests/live.rs : WebSocket ; tests/mcp.rs : IA
web/                    interface Svelte 5 + Vite + Tailwind 4 + shadcn-svelte
  nginx/                configuration nginx (en-têtes de sécurité, proxy)
  src/lib/api.ts        client API ; session.svelte.ts : compte et workspaces
  src/lib/live.svelte.ts  collaboration en direct (connexion, présence des autres membres)
  src/lib/router.svelte.ts  routes (#/w/<workspace>/workflows/<id>…)
  src/lib/engine/       worker WASM, client RPC
  src/lib/components/
    shell/              barre du haut, barre latérale, menu avatar, palette ⌘K, dialogues
    pages/              accueil, workflows, membres, paramètres, compte
    Editor.svelte       éditeur de workflow (canevas, palette de nœuds, vue détaillée)
    canvas/             canevas maison (nœuds, liens routés, mini-carte, commandes)
connectors/             service Node « connectors » (Oracle, node-oracledb thin)
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
- **Inscriptions fermées par défaut** (`ALLOW_REGISTRATION=false`) : le premier compte
  (administrateur) peut toujours être créé, les autres personnes rejoignent un workspace sur
  **invitation** (lien à usage unique, 7 jours, révocable). Seule une adresse **vérifiée** est
  ajoutée directement : sinon, quiconque s'inscrirait avec l'adresse d'un collègue recevrait
  l'accès qui lui est destiné.
- **E-mails de la plateforme** (`SMTP_HOST`, `SMTP_FROM`… facultatifs) : mot de passe oublié
  (lien de 30 min, à usage unique, déconnecte toutes les sessions et débloque le compte),
  vérification d'adresse, invitations. « Mot de passe oublié » répond de la même façon que
  l'adresse soit inscrite ou non et envoie en tâche de fond (pas de déduction des comptes) ;
  jetons de 256 bits stockés hachés. Sans SMTP, le lien d'invitation est affiché au
  propriétaire pour qu'il le transmette (l'adresse n'est alors pas considérée vérifiée).

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

## Intégrations

| Nœud | Rôle |
| --- | --- |
| PostgreSQL · lire / écrire | requête SQL → table ; insertion ou mise à jour (`ON CONFLICT`) de la table d'entrée |
| MySQL · lire / écrire | idem pour MySQL / MariaDB (`ON DUPLICATE KEY UPDATE`) |
| Oracle · lire / écrire | via le service `connectors` (node-oracledb en mode thin : open source, sans Instant Client) |
| Créer un fichier | table → fichier CSV, JSON, Parquet ou Excel dans l'espace **Fichiers** du workspace |
| Lire un fichier | CSV (séparateur détecté), JSON, Parquet, Excel / OpenDocument → table |
| Envoyer un e-mail | SMTP (STARTTLS / TLS) : un e-mail par ligne ou un récapitulatif avec pièce jointe CSV / Excel |
| Message (Slack, Teams…) | webhook entrant Slack, Discord, Microsoft Teams, Mattermost ou Google Chat |

Les textes acceptent des variables : `{{colonne}}` (valeur de la ligne), `{{nb_lignes}}` et
`{{tableau}}` (récapitulatif).

Les accès sont des **identifiants** du workspace (page « Identifiants ») : chiffrés en AES-256-GCM
(clé `CREDENTIALS_KEY`), jamais renvoyés au navigateur, référencés par les nœuds (un export de
workflow ne contient aucun secret). Ces nœuds s'exécutent **toujours sur le serveur**, y compris
lors d'un test dans l'éditeur. Les connexions suivent la même protection SSRF que les requêtes
HTTP : pour joindre une base sur un réseau privé, ajoutez-la à `NETWORK_ALLOWLIST`
(`db.interne,10.20.0.0/16`).

La page « Identifiants » affiche le **logo du service** de chaque identifiant et les **workflows qui
l'utilisent** (liens vers l'éditeur ; `GET …/credentials` renvoie `usage`, calculé à partir des
définitions des workflows). Les logos sont définis dans le code (`web/src/lib/brands.ts`,
`BrandLogo.svelte`) et ne se configurent pas. Pour un webhook de messagerie, le service (Slack,
Discord, Microsoft Teams, Mattermost, Google Chat) est reconnu d'après l'hôte de l'URL à
l'enregistrement : seul son nom est exposé dans le résumé, l'URL reste secrète. Les nœuds
d'intégration portent le même logo sur le canevas et dans la palette.

> Sauvegardez `secrets/credentials_key.txt` : sans elle, les identifiants enregistrés sont illisibles.

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
| Intégrations | PostgreSQL, MySQL, Oracle, fichiers, e-mail, messageries | voir « Intégrations » |

Dans l'éditeur, un **double-clic** sur un nœud (ou Entrée) ouvre sa vue détaillée plein écran :
entrée à gauche, paramètres au centre, sortie à droite.

Un nœud **désactivé** transmet son entrée telle quelle. « Exécuter jusqu'ici » n'exécute
que le nœud et ses ancêtres.

## Canevas

Le canevas est fait maison (`web/src/lib/canvas`, `web/src/lib/components/canvas`) :

- **Liens routés** : tracés à angles droits qui contournent les nœuds (plus court chemin
  sur une grille construite à partir des nœuds, virages pénalisés), liens parallèles
  écartés en couloirs, petits ponts aux croisements, virages arrondis. Après une
  exécution, les liens prennent la couleur des données et affichent le nombre de lignes.
- **Commandes** (barre en bas à gauche, `CanvasControls.svelte`) : zoom, tout afficher,
  Ranger, annuler / rétablir, mini-carte (masquable), aide « Gestes et raccourcis ».
- **Sélection** (`SelectionBar.svelte`) : une barre d'actions flotte au-dessus des nœuds
  sélectionnés — Paramétrer, Exécuter jusqu'ici, Dupliquer, Désactiver, Supprimer — et
  s'efface pendant un geste (déplacement, tracé d'un lien, rectangle de sélection).
- **Ranger** (`Maj + Alt + T`) : nœuds rangés en colonnes
  selon leur profondeur, ordre limitant les croisements (l'entrée A est alimentée par le
  nœud du haut), chaque nœud aligné sur ses voisins, positions sur la grille. Annulable.
- **Interactions** : glisser le fond (ou molette) pour se déplacer, `Ctrl/⌘ + molette` ou
  pincement pour zoomer, `Maj + glisser` pour sélectionner plusieurs nœuds, `Maj + clic`
  pour ajouter à la sélection, `Alt` pendant un déplacement pour ignorer la grille,
  tirer depuis un port (ou le lâcher sur un nœud) pour relier — les boucles sont refusées ;
  **lâcher le lien dans le vide** ouvre la liste des nœuds qui peuvent s'y brancher, avec en
  tête des suggestions selon l'étape voisine (`web/src/lib/canvas/suggest.ts`) : le nœud
  choisi est créé à cet endroit, déjà relié (une seule étape d'annulation),
  `Suppr` pour supprimer, `⌘/Ctrl + Z` / `Maj + ⌘/Ctrl + Z` pour annuler / rétablir,
  `⌘/Ctrl + A` pour tout sélectionner, `Maj + 1` pour tout afficher.
- Le routage est testé à part : `npm test` (aucune dépendance, `node --test`).

### Collaboration en direct

Plusieurs membres peuvent travailler sur le même workflow : chacun voit le **curseur** des
autres (avec leur nom et une couleur), les nœuds qu'ils **sélectionnent**, celui qu'ils
**paramètrent** (crayon et nom), et les nœuds qu'ils **déplacent**, en temps réel. Les pastilles en
haut à droite du canevas listent les présents ; un clic centre la vue sur l'un d'eux.

**Exécution partagée** : pendant une exécution, le nœud en cours a un contour dégradé animé,
les suivants sont « en attente », les liens où circulent les données s'animent et chaque
résultat (lignes, colonnes, erreur) s'affiche dès que le nœud se termine. Le moteur signale
le début et la fin de chaque nœud (`Host::node_started` / `node_finished`), et cet état est
diffusé : tous les membres voient la même exécution au même moment (« Alice exécute… »).
Les données elles-mêmes restent dans le navigateur de celui qui exécute.

- **Transport** : un WebSocket par workflow ouvert,
  `GET /api/workspaces/{ws}/workflows/{id}/live` — même origine, cookie de session, traverse
  nginx et Caddy sans configuration particulière. Le serveur relaie la présence (curseur,
  sélection, nœud ouvert, déplacement en cours) et la mémorise pour les arrivants ; il ne
  réémet jamais un message client tel quel (validé, puis renvoyé avec l'identité de l'émetteur).
- **Modifications** : elles passent toujours par la sauvegarde habituelle (`PUT`, version
  optimiste) ; chaque nouvelle version est diffusée aux autres clients, qui la **fusionnent**
  avec leurs modifications non enregistrées (fusion à trois voies par nœud, par lien et
  pour le nom ; si le même élément a été modifié des deux côtés, la modification locale
  l'emporte — `web/src/lib/canvas/merge.ts`). Une sauvegarde refusée (409) est fusionnée
  puis renvoyée automatiquement. L'historique d'annulation est rejoué sur la nouvelle
  version : annuler ne défait que ses propres modifications.
- **Sécurité** : `Origin` vérifié à l'ouverture (pas de détournement de WebSocket depuis
  un autre site), accès réservé aux membres, revérifié chaque minute (session révoquée,
  membre retiré) et coupé immédiatement au retrait d'un membre ou à la suppression du
  workflow ; messages de 32 Ko au plus, 40 par seconde, 25 participants par workflow.
- **Pourquoi pas QUIC / WebTransport** : un navigateur ne peut pas ouvrir de connexion QUIC
  directe, seulement WebTransport (HTTP/3), qu'aucun des proxys (nginx, Caddy) ne relaie et
  qui impose HTTPS — y compris en local. Pour quelques dizaines de petits messages par
  seconde, WebSocket offre la même réactivité. Avec le profil `tls`, la page elle-même est
  déjà servie en HTTP/3 (QUIC) par Caddy.

### IA connectées (MCP)

Une IA compatible [MCP](https://modelcontextprotocol.io) — Claude Code, Claude Desktop… — se
connecte à un workspace pour **déboguer en direct** : elle apparaît dans l'éditeur comme un
participant (pastille 🤖, nœud examiné, exécution animée) et ses modifications sont
enregistrées puis diffusées comme celles d'un membre (fusion avec les modifications locales).

- **Connexion** : Paramètres → Accès des IA → créer un jeton (`m8n_…`, affiché une fois), puis
  `claude mcp add --transport http mini8n https://<site>/api/mcp --header "Authorization: Bearer m8n_…"`.
  En développement : `http://localhost:5173/api/mcp` (proxy Vite).
- **Transport** : « Streamable HTTP » sans session, `POST /api/mcp` (un message JSON-RPC par
  requête, réponse JSON ; pas de flux SSE). Protocoles 2025-06-18, 2025-03-26, 2024-11-05.
- **Outils** : `list_workflows`, `get_workflow`, `create_workflow`, `list_node_types`,
  `list_credentials` (nom et type des identifiants, jamais les secrets) ; `run_workflow` (sur le
  serveur, tout ou jusqu'à un nœud, données de déclencheur au choix ; statut, colonnes, erreur
  et échantillon de chaque nœud), `get_run_result`, `get_node_output` (pagination, 30 min) ;
  `update_node`, `add_node`, `delete_node`, `connect_nodes`, `disconnect_nodes` (mêmes règles
  que l'éditeur : paramètres connus, pas de boucle) ; `list_executions`, `get_execution`.
- **Sécurité** : jeton personnel limité à un workspace, stocké haché (SHA-256), révocable,
  expiration au choix ; appartenance au workspace revérifiée à chaque appel ; jamais de
  cookie (pas de CSRF), `Origin` refusé s'il n'est pas celui du site (DNS rebinding) ;
  120 appels par minute et par jeton. L'IA agit au nom du membre : les nœuds d'envoi
  (e-mails, messages, écritures en base) agissent réellement quand elle exécute. Les
  identifiants restent chiffrés et ne sont jamais exposés.

## Expressions

Les expressions sont un sous-ensemble de JavaScript, comme dans n8n, évalué par le
moteur Rust (aucun moteur JavaScript n'est embarqué : pas de boucle infinie ni d'accès
au système possible).

```js
$json.prix * $json.quantite > 100 && $json.categorie === "livres"
$json.nom.trim().toUpperCase() + " (" + ($json.age ?? "?") + ")"
$json.stock > 0 ? "dispo" : "rupture"
$json["code postal"].startsWith("75")
`Commande ${$json.id} du ${$now.format("dd/MM/yyyy")}`
$input.all().map(i => i.json.montant).sum()
$('Clients').first().json.email
$json.date.toDate().plus(30, "days").toISODate()
```

- **Données** : `$json` (ligne en cours ; colonnes aplaties `adresse.ville` accessibles en
  `$json.adresse.ville`), `$input.first()` / `.last()` / `.all()` / `.item`,
  `$('Nom du nœud')` (sortie d'un autre nœud exécuté), `$index`, `$now`, `$today`.
- **Syntaxe** : opérateurs `+ - * / % **`, `=== !== == != < <= > >=`, `&& || ! ??`,
  ternaire `a ? b : c`, gabarits `` `…${x}…` ``, tableaux et objets littéraux,
  fonctions fléchées en argument (`liste.filter(x => x > 1)`), `?.`, commentaires.
- **Méthodes** : texte (`toUpperCase`, `includes`, `replaceAll`, `split`, `padStart`,
  `toNumber`, `toDate`, `isEmail`…), nombres (`toFixed`, `round(2)`, `toLocaleString`…),
  listes (`map`, `filter`, `find`, `reduce`, `join`, `sum`, `average`, `unique`, `pluck`…),
  dates (`format('EEEE d MMMM')`, `plus`, `minus`, `startOf`, `diff`, `year`, `weekday`…),
  `Math.*`, `JSON.stringify/parse`, `Object.keys`. La liste complète, documentée et testée,
  est dans [`crates/engine/src/expr/docs.rs`](../crates/engine/src/expr/docs.rs).
- **Dans les champs texte** (URL, en-têtes et corps HTTP, e-mails, messages, nom de fichier),
  les expressions s'écrivent entre `{{ … }}` : `Bonjour {{ $json.prenom }} !`.
- L'ancienne syntaxe reste acceptée : colonnes nues (`prix > 15`) et fonctions
  (`upper(nom)`, `round(x, 2)`, `if(c, a, b)`…).

Dans l'éditeur (CodeMirror), chaque champ propose l'autocomplétion contextuelle
(colonnes avec leur type et une valeur d'exemple, méthodes selon le type de la valeur,
autres nœuds), souligne les erreurs et affiche le résultat en direct, ligne par ligne.
Les colonnes du panneau d'entrée peuvent être glissées dans un champ, et le bouton
d'agrandissement ouvre un éditeur plein écran avec l'explorateur des données disponibles.
Garde-fous : longueur, profondeur, nombre d'opérations et taille des résultats bornés.

## Bibliothèques (toutes open source)

| Bibliothèque | Licence | Usage |
| --- | --- | --- |
| [arrow-rs / parquet](https://github.com/apache/arrow-rs) | Apache-2.0 | tables en mémoire, Parquet, CSV, JSON |
| [wasm-bindgen](https://github.com/wasm-bindgen/wasm-bindgen), js-sys, web-sys | MIT / Apache-2.0 | pont Rust ↔ JS |
| serde, serde_json, thiserror, bytes | MIT / Apache-2.0 | sérialisation, erreurs |
| [Svelte 5](https://svelte.dev) + Vite | MIT | interface |
| [shadcn-svelte](https://shadcn-svelte.com) + bits-ui | MIT | composants UI |
| Tailwind CSS, clsx, tailwind-merge, Lucide | MIT / ISC | styles, icônes |
| [CodeMirror 6](https://codemirror.net) | MIT | éditeur de code (expressions, SQL, JSON) |
| [axum](https://github.com/tokio-rs/axum), tokio, tower-http | MIT | API HTTP |
| [sqlx](https://github.com/launchbadge/sqlx) | MIT / Apache-2.0 | PostgreSQL, migrations |
| argon2, sha2, getrandom (RustCrypto) | MIT / Apache-2.0 | hachage, jetons |
| [reqwest](https://github.com/seanmonstar/reqwest) (rustls) | MIT / Apache-2.0 | requêtes HTTP des workflows côté serveur |
| chrono, chrono-tz | MIT / Apache-2.0 | planification, fuseaux horaires, dates des expressions |
| aes-gcm, base64 (RustCrypto) | MIT / Apache-2.0 | chiffrement des identifiants |
| sqlx (PostgreSQL, MySQL) | MIT / Apache-2.0 | intégrations bases de données |
| [node-oracledb](https://github.com/oracle/node-oracledb) (mode thin) | Apache-2.0 / UPL-1.0 | intégration Oracle, sans client propriétaire |
| [lettre](https://github.com/lettre/lettre) | MIT | e-mail SMTP |
| [calamine](https://github.com/tafia/calamine), [rust_xlsxwriter](https://github.com/jmcnamara/rust_xlsxwriter) | MIT / Apache-2.0 | lecture et écriture Excel |
| PostgreSQL, nginx, Caddy | PostgreSQL / BSD-2 / Apache-2.0 | base, serveur web, TLS |

## Limites actuelles / pistes

- Les limites de débit de l'API et les salles de collaboration sont en mémoire (une seule
  instance de `server` ; plusieurs instances demanderaient un relais commun, ex. PostgreSQL
  `LISTEN/NOTIFY`).
- Collaboration : les modifications des autres apparaissent à chaque sauvegarde (moins
  d'une seconde après), pas frappe par frappe ; le texte d'un même paramètre modifié en
  même temps n'est pas fusionné caractère par caractère.
- Le nœud HTTP exécuté dans l'éditeur est soumis au CORS du navigateur (pas côté serveur).
- Pas encore de déclencheurs e-mail (IMAP) ni flux RSS/XML, ni de connecteurs SQL Server, MongoDB, S3.
- Le `.wasm` fait ~8 Mo (~1,8 Mo gzip), surtout à cause des noyaux Arrow.
