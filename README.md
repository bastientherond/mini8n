<p align="center">
  <img src="docs/banniere.svg" alt="mini8n — automatisez vos tâches répétitives, sans écrire une ligne de code" width="100%">
</p>

<p align="center">
  <img src="https://img.shields.io/badge/sans_code-glisser_%26_relier-7c3aed?style=for-the-badge" alt="Sans code">
  <img src="https://img.shields.io/badge/vos_données-restent_chez_vous-22c55e?style=for-the-badge" alt="Vos données restent chez vous">
  <img src="https://img.shields.io/badge/100%25-open_source-0ea5e9?style=for-the-badge" alt="Open source">
  <img src="https://img.shields.io/badge/interface-en_français-f59e0b?style=for-the-badge" alt="En français">
</p>

---

## 💡 L'idée

Tout le monde a des tâches qui reviennent sans cesse : exporter un fichier chaque lundi,
recopier des réponses de formulaire dans un tableau, prévenir l'équipe quand une commande
arrive, envoyer le même rapport chaque matin…

**mini8n les fait à votre place.** Vous dessinez le chemin que suivent vos données, comme
un schéma sur un tableau blanc, et mini8n s'occupe de le parcourir — à l'heure prévue,
à chaque nouvel événement, ou quand vous cliquez sur un bouton.

> Inspiré de [n8n](https://n8n.io), en plus simple, plus léger, et pensé pour être
> installé chez vous.

## 🧩 Comment ça marche

<table>
<tr>
<td align="center" width="33%">

### 🟦 1. Un déclencheur

**Quand** faut-il agir ?

Tous les jours à 8 h, à chaque formulaire rempli, quand un site reçoit une nouvelle
donnée, ou simplement d'un clic.

</td>
<td align="center" width="33%">

### 🟪 2. Des étapes

**Que** faut-il faire ?

Récupérer, trier, filtrer, calculer, regrouper, croiser deux sources…
Chaque étape est un bloc à relier au suivant.

</td>
<td align="center" width="33%">

### 🟨 3. Un résultat

**Où** envoyer le résultat ?

Un e-mail, un message Slack ou Teams, un fichier Excel, une mise à jour dans
votre base de données.

</td>
</tr>
</table>

À chaque étape, vous **voyez vos données** : ce qui entre, ce qui sort, combien de lignes
passent. Pas de boîte noire, on comprend tout de suite ce qui se passe.

## ✨ Des exemples concrets

| | Situation | Ce que fait mini8n |
| :---: | --- | --- |
| 📬 | Chaque matin, le responsable veut les ventes de la veille | lit la base de données, regroupe par produit et **envoie un récapitulatif Excel par e-mail** à 8 h |
| 📝 | Un formulaire d'inscription à un événement | publie **une page de formulaire**, range chaque réponse et **prévient l'équipe sur Slack** |
| 🔔 | Surveiller les nouvelles annonces d'un site | interroge le site toutes les 15 minutes et **n'alerte que pour les nouveautés** |
| 🧹 | Un fichier client plein de doublons | lit le fichier, **supprime les doublons, corrige les formats** et produit un fichier propre |
| 🔗 | Deux outils qui ne se parlent pas | reçoit les données de l'un et **les transmet à l'autre** automatiquement |
| 🚨 | Un traitement automatique a planté | un workflow « en cas d'erreur » **vous prévient immédiatement** |

## 🌈 Pourquoi mini8n ?

- 🎨 **Visuel** — on construit en glissant des blocs sur une page, on range tout d'un clic,
  et les liens se colorent quand les données circulent.
- 🔒 **Vos données restent chez vous** — mini8n s'installe sur votre propre serveur.
  Rien ne part chez un prestataire, et les mots de passe enregistrés sont chiffrés.
- 👥 **Fait pour travailler à plusieurs** — des espaces de travail par équipe, des membres
  avec des rôles, et des liens à partager vers chaque workflow.
- 🧪 **On teste avant de lancer** — chaque workflow s'essaie dans l'éditeur avec des données
  d'exemple, puis on l'active et il tourne tout seul. L'historique garde la trace de chaque
  exécution.
- 🪶 **Léger et simple** — une interface en français, claire, sans menus à rallonge.
- 🆓 **Libre et gratuit** — uniquement des briques open source.

## 🔌 Ce que vous pouvez brancher

<table>
<tr>
<td>🗄️ <b>Bases de données</b><br>PostgreSQL, MySQL / MariaDB, Oracle</td>
<td>📁 <b>Fichiers</b><br>Excel, CSV, JSON, Parquet</td>
<td>✉️ <b>E-mail</b><br>un message par ligne ou un récapitulatif avec pièce jointe</td>
</tr>
<tr>
<td>💬 <b>Messageries</b><br>Slack, Microsoft Teams, Discord, Mattermost, Google Chat</td>
<td>🌐 <b>Sites et applications</b><br>appels à n'importe quel service web, webhooks</td>
<td>📋 <b>Formulaires</b><br>pages publiques à partager, réponses validées</td>
</tr>
</table>

Et pour les cas plus fins, des **petites formules** (comme dans un tableur) permettent
de calculer une colonne, mettre en forme une date ou composer un message personnalisé —
avec suggestions et aperçu du résultat pendant la saisie.

## 🤖 Déboguer avec Claude

Connectez **Claude** (Claude Code, Claude Desktop) ou une autre IA compatible MCP à votre
workspace : elle lit vos workflows, les exécute, regarde les données de chaque étape et
corrige ce qui ne va pas. Vous la voyez travailler **en direct** dans l'éditeur, comme un
collègue. Créez un jeton dans **Paramètres → Accès des IA**, puis :

```bash
claude mcp add --transport http mini8n http://localhost:8080/api/mcp --header "Authorization: Bearer m8n_…"
```

## 🚀 Envie d'essayer ?

L'installation, la configuration, la sécurité et le fonctionnement interne sont décrits
dans la **[documentation technique](docs/TECHNIQUE.md)**.

<p align="center">
  <sub>Fait avec 💜 — mini8n</sub>
</p>
