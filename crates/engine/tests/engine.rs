use futures::executor::block_on;
use mini8n_engine::data::{batch_to_json, from_parquet, to_parquet};
use mini8n_engine::{RunReport, Status, Workflow, run};
use serde_json::{Value, json};

fn workflow(v: Value) -> Workflow {
    serde_json::from_value(v).expect("workflow valide")
}

fn rows(report: &RunReport, id: &str) -> Value {
    let r = &report.results[id];
    assert_eq!(r.status, Status::Success, "{id} : {:?}", r.error);
    let batch = from_parquet(r.parquet.clone().unwrap()).unwrap();
    batch_to_json(&batch).unwrap()
}

const SALES: &str = r#"[
  {"produit": "Clavier", "categorie": "info", "prix": 49.9, "quantite": 3},
  {"produit": "Souris", "categorie": "info", "prix": 19.5, "quantite": 10},
  {"produit": "Roman", "categorie": "livres", "prix": 12, "quantite": 7},
  {"produit": "BD", "categorie": "livres", "prix": 15, "quantite": 2}
]"#;

#[test]
fn pipeline_filter_formula_sort_limit() {
    let wf = workflow(json!({
        "nodes": [
            {"id": "src", "kind": "manual", "params": {"data": SALES}},
            {"id": "f", "kind": "filter", "params": {"condition": "prix >= 15 && categorie != \"x\""}},
            {"id": "calc", "kind": "formula", "params": {"column": "total", "expression": "round(prix * quantite, 1)"}},
            {"id": "s", "kind": "sort", "params": {"by": "total desc"}},
            {"id": "l", "kind": "limit", "params": {"count": 2}},
            {"id": "c", "kind": "select", "params": {"columns": "produit, total"}}
        ],
        "edges": [
            {"source": "src", "target": "f"},
            {"source": "f", "target": "calc"},
            {"source": "calc", "target": "s"},
            {"source": "s", "target": "l"},
            {"source": "l", "target": "c"}
        ]
    }));
    let report = block_on(run(&wf, None));
    assert!(report.ok, "{report:?}");
    assert_eq!(
        rows(&report, "c"),
        json!([{"produit": "Souris", "total": 195.0}, {"produit": "Clavier", "total": 149.7}])
    );
}

#[test]
fn aggregate_by_category() {
    let wf = workflow(json!({
        "nodes": [
            {"id": "src", "kind": "manual", "params": {"data": SALES}},
            {"id": "agg", "kind": "aggregate", "params": {
                "group_by": "categorie",
                "aggregations": "count(*) as n, sum(quantite) as qte, avg(prix) as moy, max(produit)"
            }}
        ],
        "edges": [{"source": "src", "target": "agg"}]
    }));
    let report = block_on(run(&wf, None));
    assert_eq!(
        rows(&report, "agg"),
        json!([
            {"categorie": "info", "n": 2, "qte": 13, "moy": 34.7, "max_produit": "Souris"},
            {"categorie": "livres", "n": 2, "qte": 9, "moy": 13.5, "max_produit": "Roman"}
        ])
    );
}

#[test]
fn csv_join_and_append() {
    let wf = workflow(json!({
        "nodes": [
            {"id": "villes", "kind": "csv", "params": {"csv": "ville,pays\nParis,FR\nBerlin,DE\nRome,IT"}},
            {"id": "pays", "kind": "manual", "params": {"data": "[{\"code\":\"FR\",\"nom\":\"France\"},{\"code\":\"DE\",\"nom\":\"Allemagne\"}]"}},
            {"id": "inner", "kind": "merge", "params": {"mode": "jointure interne", "left_key": "pays", "right_key": "code"}},
            {"id": "left", "kind": "merge", "params": {"mode": "jointure gauche", "left_key": "pays", "right_key": "code"}},
            {"id": "stack", "kind": "merge", "params": {"mode": "empiler"}}
        ],
        "edges": [
            {"source": "villes", "target": "inner", "input": 0},
            {"source": "pays", "target": "inner", "input": 1},
            {"source": "villes", "target": "left", "input": 0},
            {"source": "pays", "target": "left", "input": 1},
            {"source": "villes", "target": "stack", "input": 0},
            {"source": "pays", "target": "stack", "input": 1}
        ]
    }));
    let report = block_on(run(&wf, None));
    assert_eq!(
        rows(&report, "inner"),
        json!([
            {"ville": "Paris", "pays": "FR", "code": "FR", "nom": "France"},
            {"ville": "Berlin", "pays": "DE", "code": "DE", "nom": "Allemagne"}
        ])
    );
    assert_eq!(rows(&report, "left").as_array().unwrap().len(), 3);
    assert_eq!(rows(&report, "left")[2]["nom"], Value::Null);
    let stacked = rows(&report, "stack");
    assert_eq!(stacked.as_array().unwrap().len(), 5);
    assert_eq!(
        stacked[3],
        json!({"ville": null, "pays": null, "code": "FR", "nom": "France"})
    );
}

#[test]
fn errors_propagate_as_skipped() {
    let wf = workflow(json!({
        "nodes": [
            {"id": "src", "kind": "manual", "params": {"data": SALES}},
            {"id": "bad", "kind": "filter", "params": {"condition": "inconnue > 1"}},
            {"id": "after", "kind": "limit", "params": {}}
        ],
        "edges": [{"source": "src", "target": "bad"}, {"source": "bad", "target": "after"}]
    }));
    let report = block_on(run(&wf, None));
    assert!(!report.ok);
    assert_eq!(report.results["src"].status, Status::Success);
    assert_eq!(report.results["bad"].status, Status::Error);
    assert!(
        report.results["bad"]
            .error
            .as_ref()
            .unwrap()
            .contains("inconnue")
    );
    assert_eq!(report.results["after"].status, Status::Skipped);
}

#[test]
fn cycle_is_rejected_and_target_limits_execution() {
    let cyclic = workflow(json!({
        "nodes": [{"id": "a", "kind": "limit"}, {"id": "b", "kind": "limit"}],
        "edges": [{"source": "a", "target": "b"}, {"source": "b", "target": "a"}]
    }));
    let report = block_on(run(&cyclic, None));
    assert!(report.error.unwrap().contains("cycle"));

    let wf = workflow(json!({
        "nodes": [
            {"id": "src", "kind": "manual", "params": {"data": SALES}},
            {"id": "x", "kind": "limit", "params": {"count": 1}},
            {"id": "y", "kind": "limit", "params": {"count": 2}}
        ],
        "edges": [{"source": "src", "target": "x"}, {"source": "src", "target": "y"}]
    }));
    let report = block_on(run(&wf, Some("x")));
    assert_eq!(report.order, vec!["src", "x"]);
}

#[test]
fn expressions() {
    let wf = workflow(json!({
        "nodes": [
            {"id": "src", "kind": "manual", "params": {"data": "[{\"nom\": \" alice \", \"age\": 30, \"ville\": {\"cp\": \"75001\"}}, {\"nom\": \"Bob\", \"age\": null}]"}},
            {"id": "e1", "kind": "formula", "params": {"column": "label", "expression": "upper(trim(nom)) + \" (\" + coalesce(string(age), \"?\") + \")\""}},
            {"id": "e2", "kind": "formula", "params": {"column": "majeur", "expression": "if(age >= 18, true, false)"}},
            {"id": "e3", "kind": "formula", "params": {"column": "cp", "expression": "`ville.cp`"}}
        ],
        "edges": [{"source": "src", "target": "e1"}, {"source": "e1", "target": "e2"}, {"source": "e2", "target": "e3"}]
    }));
    let report = block_on(run(&wf, None));
    let out = rows(&report, "e3");
    assert_eq!(out[0]["label"], "ALICE (30)");
    assert_eq!(out[1]["label"], "BOB (?)");
    assert_eq!(out[0]["majeur"], true);
    assert_eq!(out[1]["majeur"], false);
    assert_eq!(out[0]["cp"], "75001");
}

#[test]
fn distinct_rename_and_empty_tables() {
    let wf = workflow(json!({
        "nodes": [
            {"id": "src", "kind": "csv", "params": {"csv": "a,b\n1,x\n1,x\n2,y\n1,z"}},
            {"id": "d", "kind": "distinct", "params": {"columns": "a"}},
            {"id": "r", "kind": "rename", "params": {"mapping": "a -> id\nb => code"}},
            {"id": "empty", "kind": "manual", "params": {"data": "[]"}},
            {"id": "after_empty", "kind": "limit", "params": {"count": 5}}
        ],
        "edges": [
            {"source": "src", "target": "d"},
            {"source": "d", "target": "r"},
            {"source": "empty", "target": "after_empty"}
        ]
    }));
    let report = block_on(run(&wf, None));
    assert_eq!(
        rows(&report, "r"),
        json!([{"id": 1, "code": "x"}, {"id": 2, "code": "y"}])
    );
    assert_eq!(
        report.results["after_empty"].status,
        Status::Success,
        "{:?}",
        report.results["after_empty"].error
    );
    assert_eq!(report.results["after_empty"].rows, 0);
}

#[test]
fn parquet_roundtrip() {
    let batch =
        mini8n_engine::data::json_to_batch(&json!([{"a": 1, "b": "x", "c": [1, 2]}])).unwrap();
    let bytes = to_parquet(&batch).unwrap();
    assert_eq!(&bytes[..4], b"PAR1");
    let back = from_parquet(bytes).unwrap();
    assert_eq!(back, batch);
}

#[test]
fn hardening_limits() {
    let deep = format!("{}1{}", "(".repeat(200), ")".repeat(200));
    let wf = workflow(json!({
        "nodes": [
            {"id": "src", "kind": "manual", "params": {"data": SALES}},
            {"id": "deep", "kind": "formula", "params": {"column": "x", "expression": deep}},
            {"id": "long", "kind": "filter", "params": {"condition": "1 + ".repeat(2000) + "1"}}
        ],
        "edges": [{"source": "src", "target": "deep"}, {"source": "src", "target": "long"}]
    }));
    let report = block_on(run(&wf, None));
    assert!(
        report.results["deep"]
            .error
            .as_ref()
            .unwrap()
            .contains("imbriquée")
    );
    assert!(
        report.results["long"]
            .error
            .as_ref()
            .unwrap()
            .contains("trop longue")
    );
}

#[test]
fn csv_export_neutralizes_formulas() {
    let batch = mini8n_engine::data::json_to_batch(&json!([
        {"a": "=HYPERLINK(\"http://evil\")", "n": -3},
        {"a": "normal", "n": 4}
    ]))
    .unwrap();
    let csv = mini8n_engine::data::batch_to_csv(&batch).unwrap();
    assert!(csv.contains("'=HYPERLINK"), "{csv}");
    assert!(
        csv.contains("-3"),
        "les nombres négatifs restent intacts : {csv}"
    );
}

mod triggers {
    use super::*;
    use mini8n_engine::data::json_to_batch;
    use mini8n_engine::{BoxFuture, Host, RunOptions, TriggerEvent, run_with};

    fn two_triggers() -> Workflow {
        workflow(json!({
            "nodes": [
                {"id": "hook", "kind": "trigger_webhook", "params": {"test_data": "[{\"montant\": 10}, {\"montant\": 30}]"}},
                {"id": "cron", "kind": "trigger_schedule", "params": {}},
                {"id": "gros", "kind": "filter", "params": {"condition": "montant > 20"}},
                {"id": "trace", "kind": "limit", "params": {"count": 1}}
            ],
            "edges": [{"source": "hook", "target": "gros"}, {"source": "cron", "target": "trace"}]
        }))
    }

    #[test]
    fn manual_run_uses_test_data_for_every_trigger() {
        let report = block_on(run(&two_triggers(), None));
        assert!(report.ok, "{report:?}");
        assert_eq!(rows(&report, "gros"), json!([{"montant": 30}]));
        assert_eq!(report.results["trace"].rows, 1);
    }

    #[test]
    fn fired_trigger_only_runs_its_branch() {
        let event = TriggerEvent {
            node_id: "hook".into(),
            data: json_to_batch(&json!([{"montant": 99}, {"montant": 1}])).unwrap(),
        };
        let report = block_on(run_with(
            &two_triggers(),
            RunOptions {
                target: None,
                trigger: Some(event),
            },
            &mini8n_engine::NoHost,
        ));
        assert!(report.ok, "un déclencheur ignoré n'est pas une erreur");
        assert_eq!(rows(&report, "gros"), json!([{"montant": 99}]));
        assert_eq!(report.results["cron"].status, Status::Skipped);
        assert_eq!(report.results["trace"].status, Status::Skipped);
        assert_eq!(report.output.as_deref(), Some("gros"));
        assert_eq!(report.output_batch().unwrap().unwrap().num_rows(), 1);
    }

    #[test]
    fn unknown_trigger_is_rejected() {
        let event = TriggerEvent {
            node_id: "gros".into(),
            data: json_to_batch(&json!([])).unwrap(),
        };
        let report = block_on(run_with(
            &two_triggers(),
            RunOptions {
                target: None,
                trigger: Some(event),
            },
            &mini8n_engine::NoHost,
        ));
        assert!(!report.ok);
        assert!(report.error.unwrap().contains("Déclencheur introuvable"));
    }

    /// Hôte de test : le « sous-workflow » ajoute une colonne.
    struct Doubler;
    impl Host for Doubler {
        fn call_workflow<'a>(
            &'a self,
            id: &'a str,
            input: arrow::array::RecordBatch,
        ) -> BoxFuture<'a, mini8n_engine::Result<arrow::array::RecordBatch>> {
            Box::pin(async move {
                assert_eq!(id, "wf-enfant");
                let sub = workflow(json!({
                    "nodes": [
                        {"id": "in", "kind": "trigger_workflow"},
                        {"id": "x2", "kind": "formula", "params": {"column": "double", "expression": "montant * 2"}}
                    ],
                    "edges": [{"source": "in", "target": "x2"}]
                }));
                let report = run_with(
                    &sub,
                    RunOptions {
                        target: None,
                        trigger: Some(TriggerEvent {
                            node_id: "in".into(),
                            data: input,
                        }),
                    },
                    &mini8n_engine::NoHost,
                )
                .await;
                Ok(report.output_batch()?.expect("sortie"))
            })
        }
    }

    #[test]
    fn subworkflow_returns_child_output() {
        let wf = workflow(json!({
            "nodes": [
                {"id": "src", "kind": "manual", "params": {"data": "[{\"montant\": 21}]"}},
                {"id": "call", "kind": "subworkflow", "params": {"workflow": "wf-enfant"}}
            ],
            "edges": [{"source": "src", "target": "call"}]
        }));
        let report = block_on(run_with(&wf, RunOptions::default(), &Doubler));
        assert_eq!(
            rows(&report, "call"),
            json!([{"montant": 21, "double": 42}])
        );
        // Sans hôte, l'appel échoue proprement.
        let report = block_on(run(&wf, None));
        assert_eq!(report.results["call"].status, Status::Error);
    }

    #[test]
    fn form_trigger_test_output_follows_fields() {
        let wf = workflow(json!({
            "nodes": [{"id": "f", "kind": "trigger_form", "params": {"fields": "Nom | texte | requis\nÂge | nombre"}}],
            "edges": []
        }));
        let report = block_on(run(&wf, None));
        let cols: Vec<_> = report.results["f"]
            .columns
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(cols, vec!["nom", "age", "soumis_le"]);
    }
}
