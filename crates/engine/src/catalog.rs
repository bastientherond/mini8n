//! Description des types de nœuds, exposée à l'interface pour générer
//! la palette et les formulaires de paramètres.

use serde::Serialize;
use serde_json::{Value as Json, json};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Trigger,
    Source,
    Transform,
    Combine,
    Flow,
    Integration,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamType {
    /// Texte sur une ligne.
    String,
    /// Texte multi-ligne.
    Text,
    /// Expression JavaScript (éditeur de code avec autocomplétion).
    Expression,
    /// Requête SQL (éditeur de code).
    Sql,
    /// Document JSON.
    Json,
    Number,
    Boolean,
    Select,
    /// Jeton secret d'URL publique (webhook, formulaire), généré par l'interface.
    Token,
    /// Identifiant d'un workflow du workspace.
    Workflow,
    /// Identifiant chiffré du workspace (type donné par `credential`).
    Credential,
    /// Fichier du workspace.
    File,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParamSpec {
    pub name: &'static str,
    pub label: &'static str,
    #[serde(rename = "type")]
    pub ty: ParamType,
    pub default: Json,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<&'static str>,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub placeholder: &'static str,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub help: &'static str,
    /// Type d'identifiant attendu (paramètres `credential`).
    #[serde(skip_serializing_if = "str::is_empty")]
    pub credential: &'static str,
    /// Le texte accepte des expressions `{{ … }}`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub template: bool,
    /// Variables propres à ce paramètre (proposées par l'autocomplétion).
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    pub vars: &'static [&'static str],
    /// Affiché seulement si le paramètre nommé vaut l'une de ces valeurs.
    #[serde(rename = "showIf", skip_serializing_if = "Option::is_none")]
    pub show_if: Option<(&'static str, &'static [&'static str])>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeSpec {
    pub kind: &'static str,
    pub label: &'static str,
    pub category: Category,
    pub description: &'static str,
    /// Nom d'icône Lucide.
    pub icon: &'static str,
    /// Libellés des entrées (vide pour un déclencheur).
    pub inputs: Vec<&'static str>,
    /// L'entrée peut rester non reliée (lecture de données) : reliée, elle fixe l'ordre
    /// d'exécution et fournit `$json` aux expressions des paramètres.
    #[serde(rename = "optionalInput", skip_serializing_if = "std::ops::Not::not")]
    pub optional_input: bool,
    pub params: Vec<ParamSpec>,
}

fn param(name: &'static str, label: &'static str, ty: ParamType, default: Json) -> ParamSpec {
    ParamSpec {
        name,
        label,
        ty,
        default,
        options: vec![],
        placeholder: "",
        help: "",
        credential: "",
        template: false,
        vars: &[],
        show_if: None,
    }
}

fn credential(kind: &'static str) -> ParamSpec {
    let mut p = param(
        "credential",
        "Identifiant",
        ParamType::Credential,
        json!(""),
    );
    p.credential = kind;
    p
}

impl ParamSpec {
    fn options(mut self, o: &[&'static str]) -> Self {
        self.options = o.to_vec();
        self
    }
    fn placeholder(mut self, p: &'static str) -> Self {
        self.placeholder = p;
        self
    }
    fn help(mut self, h: &'static str) -> Self {
        self.help = h;
        self
    }
    /// Accepte des expressions `{{ … }}`.
    fn template(mut self) -> Self {
        self.template = true;
        self
    }
    fn vars(mut self, v: &'static [&'static str]) -> Self {
        self.vars = v;
        self
    }
    /// N'afficher que si `param` vaut l'une des `values`.
    fn show_if(mut self, param: &'static str, values: &'static [&'static str]) -> Self {
        self.show_if = Some((param, values));
        self
    }
}

const EXPR_HELP: &str = "Syntaxe JavaScript : $json.colonne, opérateurs (+ - * / % === !== && || ?? … ? … : …), \
méthodes (.toUpperCase(), .round(2), .includes()…), $now, $('Nœud').first().json.colonne. \
Ctrl+Espace affiche les suggestions.";

const SQL_HELP: &str = "{{ $json.colonne }} insère une valeur de l'entrée en paramètre lié (sans risque d'injection SQL, \
guillemets inutiles) : WHERE id = {{ $json.id }}. En mode « une fois », la première ligne est utilisée.";

/// Une exécution, ou une par ligne de l'entrée (lectures de données).
fn each_param() -> ParamSpec {
    param("each", "Exécuter", ParamType::Select, json!("une fois"))
        .options(&["une fois", "pour chaque ligne"])
        .help("« Pour chaque ligne » : une exécution par ligne de l'entrée, {{ $json.colonne }} prenant la valeur de la ligne ; les résultats sont réunis dans une seule table.")
}

/// Variables des messages récapitulatifs (e-mail, messagerie).
const SUMMARY_VARS: &[&str] = &["nb_lignes", "tableau", "lignes"];

const MESSAGE_HELP: &str = "{{ $json.colonne }} : valeur de la ligne (en récapitulatif : de la première ligne). \
Récapitulatif : {{ lignes }} insère la « Ligne répétée » de chaque ligne, {{ nb_lignes }} le nombre de lignes, \
{{ tableau }} un tableau des 20 premières lignes.";

/// Gabarit répété pour chaque ligne en mode récapitulatif (variable `lignes`).
fn row_param() -> ParamSpec {
    param("row", "Ligne répétée", ParamType::Text, json!(""))
        .placeholder("- {{ $json.produit }} : {{ $json.prix }} €")
        .template()
        .show_if("mode", &["récapitulatif"])
        .help("Écrite une fois pour chaque ligne de l'entrée, puis insérée dans le message à la place de {{ lignes }} (à la fin du message s'il ne contient pas {{ lignes }}).")
}

pub fn all() -> Vec<NodeSpec> {
    use Category::*;
    use ParamType as P;
    vec![
        // ------------------------------------------------------------------ déclencheurs
        NodeSpec {
            kind: "trigger_manual",
            label: "Déclenchement manuel",
            category: Trigger,
            description: "Démarre le workflow avec le bouton Exécuter (éditeur ou serveur).",
            icon: "mouse-pointer-click",
            inputs: vec![],
            optional_input: false,
            params: vec![],
        },
        NodeSpec {
            kind: "trigger_schedule",
            label: "Planification",
            category: Trigger,
            description: "Démarre le workflow à intervalle régulier ou à heure fixe (workflow actif).",
            icon: "clock",
            inputs: vec![],
            optional_input: false,
            params: vec![
                param("mode", "Fréquence", P::Select, json!("intervalle"))
                    .options(&["intervalle", "quotidien", "hebdomadaire", "cron"]),
                param("every", "Toutes les … minutes", P::Number, json!(60))
                    .help("1 minute minimum.")
                    .show_if("mode", &["intervalle"]),
                param("time", "Heure", P::String, json!("08:00"))
                    .placeholder("HH:MM")
                    .show_if("mode", &["quotidien", "hebdomadaire"]),
                param("days", "Jours", P::String, json!("lun, mar, mer, jeu, ven"))
                    .placeholder("lun, mer, ven")
                    .help("lun, mar, mer, jeu, ven, sam, dim.")
                    .show_if("mode", &["hebdomadaire"]),
                param("cron", "Expression cron", P::String, json!("*/15 * * * *"))
                    .placeholder("minute heure jour mois jour-semaine")
                    .show_if("mode", &["cron"])
                    .help("Mode cron (5 champs) : *, listes 1,2, plages 1-5, pas */15. Jour de semaine : 0 = dimanche."),
                param("timezone", "Fuseau horaire", P::String, json!("Europe/Paris"))
                    .placeholder("Europe/Paris"),
            ],
        },
        NodeSpec {
            kind: "trigger_webhook",
            label: "Webhook",
            category: Trigger,
            description: "Démarre le workflow à la réception d'une requête HTTP sur une URL secrète.",
            icon: "webhook",
            inputs: vec![],
            optional_input: false,
            params: vec![
                param("token", "URL du webhook", P::Token, json!(""))
                    .help("L'URL contient un jeton secret : ne la partagez qu'avec l'émetteur. Le workflow doit être actif."),
                param("method", "Méthode acceptée", P::Select, json!("POST"))
                    .options(&["POST", "GET", "PUT", "toutes"]),
                param("secret", "Secret (en-tête X-Mini8n-Secret)", P::String, json!(""))
                    .placeholder("facultatif")
                    .help("Si renseigné, les requêtes sans l'en-tête X-Mini8n-Secret correspondant sont refusées."),
                param("respond", "Réponse", P::Select, json!("immédiate"))
                    .options(&["immédiate", "résultat du workflow"])
                    .help("« Résultat du workflow » attend la fin de l'exécution (30 s max.) et renvoie la sortie en JSON."),
                param("test_data", "Données de test", P::Json, json!("{\n  \"exemple\": \"valeur\",\n  \"nombre\": 42\n}"))
                    .help("Utilisées quand vous exécutez le workflow depuis l'éditeur."),
            ],
        },
        NodeSpec {
            kind: "trigger_form",
            label: "Formulaire",
            category: Trigger,
            description: "Publie un formulaire ; chaque envoi démarre le workflow.",
            icon: "clipboard-list",
            inputs: vec![],
            optional_input: false,
            params: vec![
                param("token", "Lien du formulaire", P::Token, json!(""))
                    .help("Page publique (sans compte). Le workflow doit être actif."),
                param("title", "Titre", P::String, json!("Nous contacter")),
                param("description", "Description", P::Text, json!("")),
                param("fields", "Champs", P::Text, json!("Nom | texte | requis\nE-mail | email | requis\nMessage | texte long"))
                    .help("Un champ par ligne : « Libellé | type | requis ». Types : texte, texte long, email, nombre, date, case."),
                param("confirmation", "Message de confirmation", P::String, json!("Merci, votre réponse a bien été envoyée.")),
                param("test_data", "Données de test", P::Json, json!(""))
                    .help("Vide : une réponse vide est générée à partir des champs."),
            ],
        },
        NodeSpec {
            kind: "trigger_poll",
            label: "Surveillance d'API",
            category: Trigger,
            description: "Interroge une API JSON régulièrement et démarre le workflow avec les nouveaux éléments.",
            icon: "radar",
            inputs: vec![],
            optional_input: false,
            params: vec![
                param("url", "URL", P::String, json!("https://jsonplaceholder.typicode.com/posts"))
                    .template()
                    .placeholder("https://api.exemple.com/items"),
                param("headers", "En-têtes", P::Text, json!(""))
                    .placeholder("Authorization: Bearer …")
                    .template(),
                param("items_path", "Chemin des éléments", P::String, json!(""))
                    .placeholder("data.items"),
                param("key", "Colonne identifiante", P::String, json!("id"))
                    .help("Sert à reconnaître les éléments déjà vus. Vide : l'élément entier est comparé."),
                param("every", "Toutes les … minutes", P::Number, json!(15))
                    .help("1 minute minimum. Au premier passage, les éléments existants sont mémorisés sans déclencher."),
            ],
        },
        NodeSpec {
            kind: "trigger_workflow",
            label: "Appelé par un workflow",
            category: Trigger,
            description: "Démarre quand un autre workflow l'exécute (nœud « Exécuter un workflow »).",
            icon: "corner-down-right",
            inputs: vec![],
            optional_input: false,
            params: vec![param("test_data", "Données de test", P::Json, json!("[\n  {\"id\": 1, \"valeur\": \"a\"}\n]"))
                .help("Utilisées quand vous exécutez ce workflow depuis l'éditeur.")],
        },
        NodeSpec {
            kind: "trigger_error",
            label: "Sur erreur",
            category: Trigger,
            description: "Démarre quand une exécution automatique d'un autre workflow du workspace échoue.",
            icon: "triangle-alert",
            inputs: vec![],
            optional_input: false,
            params: vec![],
        },
        // ------------------------------------------------------------------ sources
        NodeSpec {
            kind: "manual",
            label: "Données JSON",
            category: Source,
            description: "Saisie manuelle d'un tableau d'objets JSON.",
            icon: "braces",
            inputs: vec!["Entrée"],
            optional_input: true,
            params: vec![param(
                "data",
                "Données",
                P::Json,
                json!(
                    "[\n  {\"produit\": \"Clavier\", \"categorie\": \"info\", \"prix\": 49.9, \"quantite\": 3},\n  {\"produit\": \"Souris\", \"categorie\": \"info\", \"prix\": 19.5, \"quantite\": 10},\n  {\"produit\": \"Roman\", \"categorie\": \"livres\", \"prix\": 12, \"quantite\": 7},\n  {\"produit\": \"BD\", \"categorie\": \"livres\", \"prix\": 15, \"quantite\": 2}\n]"
                ),
            )
            .help("Tableau d'objets ; les objets imbriqués sont aplatis (a.b).")],
        },
        NodeSpec {
            kind: "csv",
            label: "CSV",
            category: Source,
            description: "Lit un contenu CSV collé.",
            icon: "sheet",
            inputs: vec!["Entrée"],
            optional_input: true,
            params: vec![
                param("csv", "Contenu CSV", P::Text, json!("ville,pays,habitants\nParis,FR,2100000\nLyon,FR,520000\nBerlin,DE,3700000"))
                    .placeholder("col1,col2\n1,2"),
                param("delimiter", "Séparateur", P::String, json!(",")),
                param("header", "Première ligne = en-têtes", P::Boolean, json!(true)),
            ],
        },
        NodeSpec {
            kind: "http",
            label: "Requête HTTP",
            category: Source,
            description: "Appelle une API et convertit la réponse JSON en table.",
            icon: "globe",
            inputs: vec!["Entrée"],
            optional_input: true,
            params: vec![
                each_param(),
                param("method", "Méthode", P::Select, json!("GET"))
                    .options(&["GET", "POST", "PUT", "PATCH", "DELETE"]),
                param("url", "URL", P::String, json!("https://jsonplaceholder.typicode.com/users"))
                    .placeholder("https://api.exemple.com/items")
                    .template(),
                param("headers", "En-têtes", P::Text, json!(""))
                    .placeholder("Authorization: Bearer …\nAccept: application/json")
                    .template(),
                param("body", "Corps", P::Text, json!(""))
                    .placeholder("{\"date\": \"{{ $now.toISODate() }}\"}")
                    .template()
                    .show_if("method", &["POST", "PUT", "PATCH", "DELETE"]),
                param("items_path", "Chemin des éléments", P::String, json!(""))
                    .placeholder("data.items")
                    .help("Chemin (séparé par des points) vers le tableau dans la réponse. Vide = racine."),
            ],
        },
        NodeSpec {
            kind: "filter",
            label: "Filtrer",
            category: Transform,
            description: "Garde les lignes qui respectent une condition.",
            icon: "funnel",
            inputs: vec!["Entrée"],
            optional_input: false,
            params: vec![param("condition", "Condition", P::Expression, json!("$json.prix > 15"))
                .placeholder("$json.prix > 10 && $json.categorie === \"livres\"")
                .help(EXPR_HELP)],
        },
        NodeSpec {
            kind: "formula",
            label: "Formule",
            category: Transform,
            description: "Ajoute ou remplace une colonne calculée.",
            icon: "sigma",
            inputs: vec!["Entrée"],
            optional_input: false,
            params: vec![
                param("column", "Colonne", P::String, json!("total")),
                param("expression", "Expression", P::Expression, json!("($json.prix * $json.quantite).round(2)"))
                    .help(EXPR_HELP),
            ],
        },
        NodeSpec {
            kind: "select",
            label: "Colonnes",
            category: Transform,
            description: "Garde ou retire des colonnes.",
            icon: "columns-3",
            inputs: vec!["Entrée"],
            optional_input: false,
            params: vec![
                param("mode", "Mode", P::Select, json!("garder")).options(&["garder", "retirer"]),
                param("columns", "Colonnes", P::String, json!(""))
                    .placeholder("col1, col2")
                    .help("Liste séparée par des virgules. En mode « garder », l'ordre est respecté."),
            ],
        },
        NodeSpec {
            kind: "rename",
            label: "Renommer",
            category: Transform,
            description: "Renomme des colonnes.",
            icon: "pencil",
            inputs: vec!["Entrée"],
            optional_input: false,
            params: vec![param("mapping", "Renommages", P::Text, json!(""))
                .placeholder("ancien -> nouveau\nprix -> price")
                .help("Une règle « ancien -> nouveau » par ligne.")],
        },
        NodeSpec {
            kind: "sort",
            label: "Trier",
            category: Transform,
            description: "Trie les lignes sur une ou plusieurs colonnes.",
            icon: "arrow-down-wide-narrow",
            inputs: vec!["Entrée"],
            optional_input: false,
            params: vec![param("by", "Trier par", P::String, json!(""))
                .placeholder("prix desc, produit")
                .help("Colonnes séparées par des virgules, suivies de « asc » ou « desc ».")],
        },
        NodeSpec {
            kind: "limit",
            label: "Limiter",
            category: Transform,
            description: "Garde une tranche de lignes.",
            icon: "list-end",
            inputs: vec!["Entrée"],
            optional_input: false,
            params: vec![
                param("count", "Nombre de lignes", P::Number, json!(10)),
                param("offset", "Décalage", P::Number, json!(0)),
            ],
        },
        NodeSpec {
            kind: "distinct",
            label: "Dédoublonner",
            category: Transform,
            description: "Supprime les lignes en double.",
            icon: "copy-minus",
            inputs: vec!["Entrée"],
            optional_input: false,
            params: vec![param("columns", "Colonnes comparées", P::String, json!(""))
                .placeholder("toutes")
                .help("Vide = toutes les colonnes. La première occurrence est conservée.")],
        },
        NodeSpec {
            kind: "aggregate",
            label: "Agréger",
            category: Transform,
            description: "Regroupe les lignes et calcule des agrégats.",
            icon: "group",
            inputs: vec!["Entrée"],
            optional_input: false,
            params: vec![
                param("group_by", "Regrouper par", P::String, json!("categorie"))
                    .placeholder("col1, col2"),
                param("aggregations", "Agrégats", P::String, json!("count(*) as nb, sum(prix) as total_prix, avg(prix) as prix_moyen"))
                    .help("Fonctions : count, count_distinct, sum, avg, min, max, first, last. Ex. « sum(prix) as total ».")
                    .placeholder("count(*) as n, sum(prix) as total"),
            ],
        },
        NodeSpec {
            kind: "merge",
            label: "Fusionner",
            category: Combine,
            description: "Combine deux tables : empilement ou jointure.",
            icon: "merge",
            inputs: vec!["A", "B"],
            optional_input: false,
            params: vec![
                param("mode", "Mode", P::Select, json!("empiler"))
                    .options(&["empiler", "jointure interne", "jointure gauche"]),
                param("left_key", "Clé de A", P::String, json!(""))
                    .placeholder("id")
                    .help("Plusieurs clés : séparées par des virgules.")
                    .show_if("mode", &["jointure interne", "jointure gauche"]),
                param("right_key", "Clé de B", P::String, json!(""))
                    .placeholder("identique à A si vide")
                    .show_if("mode", &["jointure interne", "jointure gauche"]),
            ],
        },
        // ------------------------------------------------------------------ intégrations
        NodeSpec {
            kind: "postgres_query",
            label: "PostgreSQL · lire",
            category: Integration,
            description: "Exécute une requête SQL sur PostgreSQL et renvoie les lignes.",
            icon: "database",
            inputs: vec!["Entrée"],
            optional_input: true,
            params: vec![
                credential("postgres"),
                each_param(),
                param("query", "Requête SQL", P::Sql, json!("SELECT * FROM commandes ORDER BY id DESC LIMIT 100"))
                    .help(SQL_HELP),
                param("max_rows", "Lignes maximum", P::Number, json!(10000)),
            ],
        },
        NodeSpec {
            kind: "postgres_write",
            label: "PostgreSQL · écrire",
            category: Integration,
            description: "Insère (ou met à jour) les lignes de la table d'entrée dans une table PostgreSQL.",
            icon: "database",
            inputs: vec!["Entrée"],
            optional_input: false,
            params: vec![
                credential("postgres"),
                param("table", "Table", P::String, json!("")).placeholder("schema.table"),
                param("mode", "Mode", P::Select, json!("insertion")).options(&["insertion", "insertion ou mise à jour"]),
                param("key", "Clé (mise à jour)", P::String, json!("id"))
                    .help("Colonnes de la contrainte unique utilisée pour la mise à jour.")
                    .show_if("mode", &["insertion ou mise à jour"]),
            ],
        },
        NodeSpec {
            kind: "mysql_query",
            label: "MySQL · lire",
            category: Integration,
            description: "Exécute une requête SQL sur MySQL / MariaDB et renvoie les lignes.",
            icon: "database",
            inputs: vec!["Entrée"],
            optional_input: true,
            params: vec![
                credential("mysql"),
                each_param(),
                param("query", "Requête SQL", P::Sql, json!("SELECT * FROM commandes LIMIT 100"))
                    .help(SQL_HELP),
                param("max_rows", "Lignes maximum", P::Number, json!(10000)),
            ],
        },
        NodeSpec {
            kind: "mysql_write",
            label: "MySQL · écrire",
            category: Integration,
            description: "Insère (ou met à jour) les lignes de la table d'entrée dans une table MySQL / MariaDB.",
            icon: "database",
            inputs: vec!["Entrée"],
            optional_input: false,
            params: vec![
                credential("mysql"),
                param("table", "Table", P::String, json!("")),
                param("mode", "Mode", P::Select, json!("insertion")).options(&["insertion", "insertion ou mise à jour"])
                    .help("La mise à jour s'appuie sur la clé primaire ou un index unique de la table."),
            ],
        },
        NodeSpec {
            kind: "oracle_query",
            label: "Oracle · lire",
            category: Integration,
            description: "Exécute une requête SQL sur Oracle Database et renvoie les lignes.",
            icon: "database",
            inputs: vec!["Entrée"],
            optional_input: true,
            params: vec![
                credential("oracle"),
                each_param(),
                param("query", "Requête SQL", P::Sql, json!("SELECT * FROM commandes FETCH FIRST 100 ROWS ONLY"))
                    .help(SQL_HELP),
                param("max_rows", "Lignes maximum", P::Number, json!(10000)),
            ],
        },
        NodeSpec {
            kind: "oracle_write",
            label: "Oracle · écrire",
            category: Integration,
            description: "Insère les lignes de la table d'entrée dans une table Oracle.",
            icon: "database",
            inputs: vec!["Entrée"],
            optional_input: false,
            params: vec![credential("oracle"), param("table", "Table", P::String, json!("")).placeholder("SCHEMA.TABLE")],
        },
        NodeSpec {
            kind: "file_write",
            label: "Créer un fichier",
            category: Integration,
            description: "Enregistre la table d'entrée dans un fichier du workspace (CSV, JSON, Parquet, Excel).",
            icon: "file-plus",
            inputs: vec!["Entrée"],
            optional_input: false,
            params: vec![
                param("name", "Nom du fichier", P::String, json!("export-{{ $now.format('yyyy-MM-dd') }}"))
                    .template()
                    .help("L'extension est ajoutée selon le format. Expressions {{ … }} : première ligne de l'entrée, $now…"),
                param("format", "Format", P::Select, json!("csv")).options(&["csv", "json", "parquet", "excel"]),
            ],
        },
        NodeSpec {
            kind: "file_read",
            label: "Lire un fichier",
            category: Integration,
            description: "Lit un fichier du workspace (CSV, JSON, Parquet, Excel, OpenDocument).",
            icon: "file-input",
            inputs: vec!["Entrée"],
            optional_input: true,
            params: vec![
                param("file", "Fichier", P::File, json!("")),
                param("sheet", "Feuille (Excel)", P::String, json!("")).placeholder("première feuille"),
            ],
        },
        NodeSpec {
            kind: "email_send",
            label: "Envoyer un e-mail",
            category: Integration,
            description: "Envoie un e-mail par ligne, ou un récapitulatif avec la table en pièce jointe.",
            icon: "mail",
            inputs: vec!["Entrée"],
            optional_input: false,
            params: vec![
                credential("smtp"),
                param("mode", "Mode", P::Select, json!("un par ligne"))
                    .options(&["un par ligne", "récapitulatif"])
                    .help("Un par ligne : un e-mail pour chaque ligne de l'entrée. Récapitulatif : un seul e-mail pour toutes les lignes."),
                param("to", "Destinataire(s)", P::String, json!(""))
                    .placeholder("{{ $json.email }} ou equipe@exemple.com")
                    .template()
                    .vars(SUMMARY_VARS)
                    .help("Plusieurs adresses séparées par des virgules."),
                param("subject", "Objet", P::String, json!("Nouvelle commande {{ $json.id }}"))
                    .template()
                    .vars(SUMMARY_VARS),
                param("body", "Message", P::Text, json!("Bonjour,\n\nVoici les informations : {{ $json.produit }} × {{ $json.quantite }}.\n"))
                    .template()
                    .vars(SUMMARY_VARS)
                    .help(MESSAGE_HELP),
                row_param(),
                param("attachment", "Pièce jointe", P::Select, json!("aucune"))
                    .options(&["aucune", "csv", "excel"])
                    .show_if("mode", &["récapitulatif"]),
            ],
        },
        NodeSpec {
            kind: "chat_message",
            label: "Message (Slack, Teams…)",
            category: Integration,
            description: "Publie un message via un webhook Slack, Discord, Microsoft Teams, Mattermost ou Google Chat.",
            icon: "message-square",
            inputs: vec!["Entrée"],
            optional_input: false,
            params: vec![
                credential("chat_webhook"),
                param("service", "Service", P::Select, json!("Slack")).options(&["Slack", "Discord", "Microsoft Teams", "Mattermost", "Google Chat"]),
                param("mode", "Mode", P::Select, json!("récapitulatif"))
                    .options(&["récapitulatif", "un par ligne"])
                    .help("Récapitulatif : un seul message pour toutes les lignes. Un par ligne : un message pour chaque ligne de l'entrée."),
                param("message", "Message", P::Text, json!("{{ nb_lignes }} nouvelle(s) ligne(s) :\n{{ tableau }}"))
                    .template()
                    .vars(SUMMARY_VARS)
                    .help(MESSAGE_HELP),
                row_param(),
            ],
        },
        // ------------------------------------------------------------------ flux
        NodeSpec {
            kind: "subworkflow",
            label: "Exécuter un workflow",
            category: Flow,
            description: "Envoie la table à un autre workflow (déclencheur « Appelé par un workflow ») et récupère sa sortie.",
            icon: "workflow",
            inputs: vec!["Entrée"],
            optional_input: false,
            params: vec![param("workflow", "Workflow", P::Workflow, json!(""))
                .help("Workflow du même workspace. Sa sortie est son dernier nœud terminal exécuté avec succès.")],
        },
    ]
}

pub fn spec(kind: &str) -> Option<NodeSpec> {
    all().into_iter().find(|s| s.kind == kind)
}

/// Nœuds exécutés par l'hôte serveur (bases de données, fichiers, e-mail, messagerie).
pub fn is_integration(kind: &str) -> bool {
    matches!(
        kind,
        "postgres_query"
            | "postgres_write"
            | "mysql_query"
            | "mysql_write"
            | "oracle_query"
            | "oracle_write"
            | "file_write"
            | "file_read"
            | "email_send"
            | "chat_message"
    )
}
