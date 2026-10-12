//! Description des types de nœuds, exposée à l'interface pour générer
//! la palette et les formulaires de paramètres.

use serde::Serialize;
use serde_json::{Value as Json, json};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Source,
    Transform,
    Combine,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamType {
    /// Texte sur une ligne.
    String,
    /// Texte multi-ligne.
    Text,
    /// Expression (éditeur monospace).
    Expression,
    /// Document JSON.
    Json,
    Number,
    Boolean,
    Select,
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
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeSpec {
    pub kind: &'static str,
    pub label: &'static str,
    pub category: Category,
    pub description: &'static str,
    /// Nom d'icône Lucide.
    pub icon: &'static str,
    /// Libellés des entrées (vide pour une source).
    pub inputs: Vec<&'static str>,
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
    }
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
}

const EXPR_HELP: &str = "Colonnes par leur nom (ou `nom avec espaces`). Opérateurs : + - * / % == != < <= > >= && || !. \
Fonctions : upper, lower, trim, len, concat, contains, starts_with, ends_with, replace, substr, \
round, floor, ceil, abs, sqrt, pow, min, max, coalesce, if, is_null, number, string.";

pub fn all() -> Vec<NodeSpec> {
    use Category::*;
    use ParamType as P;
    vec![
        NodeSpec {
            kind: "manual",
            label: "Données JSON",
            category: Source,
            description: "Saisie manuelle d'un tableau d'objets JSON.",
            icon: "braces",
            inputs: vec![],
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
            inputs: vec![],
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
            inputs: vec![],
            params: vec![
                param("method", "Méthode", P::Select, json!("GET"))
                    .options(&["GET", "POST", "PUT", "PATCH", "DELETE"]),
                param("url", "URL", P::String, json!("https://jsonplaceholder.typicode.com/users"))
                    .placeholder("https://api.exemple.com/items"),
                param("headers", "En-têtes", P::Text, json!(""))
                    .placeholder("Authorization: Bearer …\nAccept: application/json"),
                param("body", "Corps", P::Text, json!("")),
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
            params: vec![param("condition", "Condition", P::Expression, json!("prix > 15"))
                .placeholder("prix > 10 && categorie == \"livres\"")
                .help(EXPR_HELP)],
        },
        NodeSpec {
            kind: "formula",
            label: "Formule",
            category: Transform,
            description: "Ajoute ou remplace une colonne calculée.",
            icon: "sigma",
            inputs: vec!["Entrée"],
            params: vec![
                param("column", "Colonne", P::String, json!("total")),
                param("expression", "Expression", P::Expression, json!("round(prix * quantite, 2)"))
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
            params: vec![
                param("mode", "Mode", P::Select, json!("empiler"))
                    .options(&["empiler", "jointure interne", "jointure gauche"]),
                param("left_key", "Clé de A", P::String, json!(""))
                    .placeholder("id")
                    .help("Pour les jointures. Plusieurs clés : séparées par des virgules."),
                param("right_key", "Clé de B", P::String, json!(""))
                    .placeholder("identique à A si vide"),
            ],
        },
    ]
}

pub fn spec(kind: &str) -> Option<NodeSpec> {
    all().into_iter().find(|s| s.kind == kind)
}
