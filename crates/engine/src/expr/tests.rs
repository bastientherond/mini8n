use arrow::array::RecordBatch;
use serde_json::json;

use super::value::strict_eq;
use super::*;
use crate::data::json_to_batch;

fn batch() -> RecordBatch {
    json_to_batch(&json!([
        {"nom": " alice ", "age": 30, "prix": 12.5, "ville": {"cp": "75001"}, "code postal": "75001"},
        {"nom": "Bob", "age": null, "prix": 20, "ville": {"cp": "69002"}, "code postal": "69002"}
    ]))
    .unwrap()
}

fn scope(b: &RecordBatch) -> Scope<'_> {
    let mut s = Scope::new(Some(b));
    s.offset_min = 120;
    s.now_ms = value::parse_date("2026-10-12T15:04:05+02:00", 0).unwrap();
    s
}

fn eval_row(src: &str, r: usize) -> Result<serde_json::Value, ExprError> {
    let b = batch();
    let s = scope(&b);
    let e = Expr::compile(src, &s)?;
    let rows = RowAccessor::new(&b).unwrap();
    Ok(e.eval(Some(&rows), r)?.to_json(120))
}

fn ok(src: &str) -> serde_json::Value {
    eval_row(src, 0).unwrap_or_else(|e| panic!("{src} : {e}"))
}

fn error(src: &str) -> String {
    match eval_row(src, 0) {
        Ok(v) => panic!("{src} aurait dû échouer, a donné {v}"),
        Err(e) => e.message,
    }
}

#[test]
fn javascript_syntax() {
    assert_eq!(ok("$json.nom.trim().toUpperCase()"), json!("ALICE"));
    assert_eq!(ok("$json['code postal']"), json!("75001"));
    assert_eq!(ok("$json.ville.cp"), json!("75001"));
    assert_eq!(ok("$json.age >= 18 ? 'majeur' : 'mineur'"), json!("majeur"));
    assert_eq!(ok("`${$json.age} ans`"), json!("30 ans"));
    assert_eq!(ok("$json.prix * 2 === 25"), json!(true));
    assert_eq!(ok("2 ** 10"), json!(1024));
    assert_eq!(ok("$json.nom || 'inconnu'"), json!(" alice "));
    assert_eq!(
        eval_row("$json.age ?? 'inconnu'", 1).unwrap(),
        json!("inconnu")
    );
    assert_eq!(
        ok("[1, 2, 3].map(x => x * 2).filter(x => x > 2)"),
        json!([4, 6])
    );
    assert_eq!(ok("$input.all().map(i => i.json.prix).sum()"), json!(32.5));
    assert_eq!(
        ok("$input.first().json.age + $input.last().json.prix"),
        json!(50.0)
    );
    assert_eq!(ok("$input.item.json.nom"), json!(" alice "));
    assert_eq!(ok("$index"), json!(0));
    assert_eq!(ok("({ a: 1, b: [1, 2] }).b.length"), json!(2));
    assert_eq!(
        ok("JSON.stringify($json.ville)"),
        json!("{\"cp\":\"75001\"}")
    );
    assert_eq!(
        ok("$now.format('dd/MM/yyyy HH:mm')"),
        json!("12/10/2026 15:04")
    );
    assert_eq!(ok("$today.toISO()"), json!("2026-10-12T00:00:00.000+02:00"));
    assert_eq!(ok("$now.plus(1, 'jour').weekdayLong"), json!("mardi"));
    assert_eq!(ok("1 /* commentaire */ + 1 // fin"), json!(2));
    assert_eq!(ok("{{ $json.age + 1 }}"), json!(31));
}

#[test]
fn legacy_syntax_still_works() {
    assert_eq!(
        ok("upper(trim(nom)) + \" (\" + coalesce(string(age), \"?\") + \")\""),
        json!("ALICE (30)")
    );
    assert_eq!(ok("prix > 10 and age = 30"), json!(true));
    assert_eq!(ok("not age > 40"), json!(true));
    assert_eq!(ok("if(age >= 18, 'oui', 'non')"), json!("oui"));
    assert_eq!(ok("ville.cp"), json!("75001"));
}

#[test]
fn helpful_errors() {
    assert!(
        error("$json.nmo").contains("vouliez-vous dire $json.nom"),
        "{}",
        error("$json.nmo")
    );
    assert!(error("$json.nom.toUppercase()").contains(".toUpperCase()"));
    assert!(error("$json.age.toUpperCase()").contains("s'applique à un texte"));
    assert!(error("$json.nom.length()").contains("propriété"));
    assert!(error("Math.rond(1)").contains("Math.round"));
    assert!(error("nom +").contains("incomplète"));
    assert!(error("foo").contains("n'est pas défini"));
    assert!(error("$('Inconnu').first()").contains("pas disponible"));
    let e = Expr::compile("1 + $json.x", &Scope::new(None)).unwrap_err();
    assert!(e.message.contains("Pas de données"));
    assert_eq!(e.span, Some(Span::new(4, 9)));
}

#[test]
fn other_nodes() {
    let b = batch();
    let clients = json_to_batch(&json!([{"email": "a@b.fr"}, {"email": "c@d.fr"}])).unwrap();
    let lookup = |name: &str| (name == "Clients").then(|| clients.clone());
    let mut s = scope(&b);
    s.nodes = Some(&lookup);
    s.node_names = vec!["Clients".into()];
    let rows = RowAccessor::new(&b).unwrap();
    let eval = |src: &str, r: usize| {
        Expr::compile(src, &s)
            .and_then(|e| e.eval(Some(&rows), r))
            .map(|v| v.to_json(0))
    };
    assert_eq!(
        eval("$('Clients').first().json.email", 1).unwrap(),
        json!("a@b.fr")
    );
    assert_eq!(
        eval("$('Clients').item.json.email", 1).unwrap(),
        json!("c@d.fr")
    );
    assert_eq!(
        eval("$node['Clients'].json.email", 0).unwrap(),
        json!("a@b.fr")
    );
    assert_eq!(eval("$('Clients').all().length", 0).unwrap(), json!(2));
    assert!(
        eval("$('Client').first()", 0)
            .unwrap_err()
            .message
            .contains("$('Clients')")
    );
}

#[test]
fn templates() {
    let b = batch();
    let s = scope(&b);
    let rows = RowAccessor::new(&b).unwrap();
    let t = Template::compile(
        "Bonjour {{ $json.nom.trim() }}, {{ $json.age }} ans {{ '}}' }}",
        &s,
    )
    .unwrap();
    assert_eq!(
        t.render(Some(&rows), 0).unwrap(),
        "Bonjour alice, 30 ans }}"
    );
    assert_eq!(t.render(Some(&rows), 1).unwrap(), "Bonjour Bob,  ans }}");
    let e = Template::compile("a {{ $json.nom ", &s).unwrap_err();
    assert!(e.message.contains("}}"));
    let e = Template::compile("x {{ $json.nmo }}", &s).unwrap_err();
    assert_eq!(e.span, Some(Span::new(11, 14)));
    // Variables supplémentaires (messages récapitulatifs).
    let s = Scope::new(Some(&b)).with_var("nb_lignes", Value::Int(2));
    assert_eq!(
        render_once("{{nb_lignes}} ligne(s)", &s).unwrap(),
        "2 ligne(s)"
    );
    assert_eq!(
        render_once("sans expression", &s).unwrap(),
        "sans expression"
    );
}

#[test]
fn trial_reports_value_type_and_errors() {
    let b = batch();
    let s = scope(&b);
    let t = trial("$json.prix", false, &s, 1);
    assert!(t.ok);
    assert_eq!(t.value, Some(json!(20.0)));
    assert_eq!(t.kind, Some("number"));
    assert_eq!(t.rows, 2);
    let t = trial("Total : {{ $json.prix }} €", true, &s, 0);
    assert_eq!(t.value, Some(json!("Total : 12.5 €")));
    let t = trial("$json.prix +", false, &s, 0);
    assert!(!t.ok);
    assert!(t.span.is_some());
}

#[test]
fn limits() {
    assert!(error("'a'.repeat(100000000)").contains("trop long"));
    assert!(error("[1,2,3,4,5,6,7,8,9,10].map(a => [1,2,3,4,5,6,7,8,9,10].map(b => [1,2,3,4,5,6,7,8,9,10].map(c => [1,2,3,4,5,6,7,8,9,10].map(d => [1,2,3,4,5,6,7,8,9,10].map(e => [1,2,3,4,5,6,7,8,9,10].map(f => [1,2,3,4,5,6,7,8,9,10].map(g => g)))))))").contains("coûteuse"));
    let deep = format!("{}1{}", "(".repeat(200), ")".repeat(200));
    assert!(error(&deep).contains("imbriquée"));
    assert!(error(&"1 + ".repeat(2000)).contains("trop longue"));
    // Comparateur incohérent : pas de panique.
    assert!(eval_row("[3, 1, 2, 5, 4].sort((a, b) => 1)", 0).is_ok());
}

/// Les exemples de la documentation (autocomplétion) donnent le résultat annoncé.
#[test]
fn documented_examples() {
    let s = Scope::new(None);
    for d in docs::DOCS {
        if d.result.is_empty() {
            continue;
        }
        let v = Expr::compile(d.example, &s)
            .and_then(|e| e.eval(None, 0))
            .unwrap_or_else(|e| panic!("{} ({}) : {}", d.example, d.name, e));
        let expected = V::from_json(&serde_json::from_str(d.result).unwrap());
        assert!(
            strict_eq(&v, &expected),
            "{} : attendu {}, obtenu {}",
            d.example,
            d.result,
            v.to_json(0)
        );
    }
}
