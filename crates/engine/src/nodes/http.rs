use arrow::array::RecordBatch;

use serde_json::{Map, Value as Json};

use super::Params;
use crate::data::json_to_batch;
use crate::error::{Result, fail};
use crate::expr::Scope;
use crate::host::{Host, HttpRequest};

pub const MAX_RESPONSE_BYTES: usize = 50 * 1024 * 1024;

/// Suit un chemin `a.b.0.c` dans un document JSON.
pub fn navigate(doc: serde_json::Value, path: &str) -> Result<serde_json::Value> {
    let mut cur = doc;
    for part in path.split('.').map(str::trim).filter(|s| !s.is_empty()) {
        cur = match cur {
            serde_json::Value::Object(mut o) => o.remove(part),
            serde_json::Value::Array(mut a) => part
                .parse::<usize>()
                .ok()
                .filter(|i| *i < a.len())
                .map(|i| a.swap_remove(i)),
            _ => None,
        }
        .ok_or_else(|| {
            crate::error::EngineError::Msg(format!(
                "Chemin introuvable dans la réponse : « {part} »"
            ))
        })?;
    }
    Ok(cur)
}

/// Valide les paramètres et construit la requête (commune au navigateur et au serveur).
pub fn build_request(p: &Params) -> Result<HttpRequest> {
    let url = p.str("url").trim().to_string();
    if url.is_empty() {
        return fail("URL requise");
    }
    // Uniquement des URL absolues http(s) : pas de data:, blob:, javascript:, file:, ni de chemin relatif.
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return fail("L'URL doit commencer par https:// (ou http://)");
    }
    let method = p.str_or("method", "GET").to_uppercase();
    if !matches!(method.as_str(), "GET" | "POST" | "PUT" | "PATCH" | "DELETE") {
        return fail(format!("Méthode non autorisée : {method}"));
    }
    let mut headers = Vec::new();
    let mut has_content_type = false;
    for line in p.str("headers").lines() {
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim();
            if k.is_empty() {
                continue;
            }
            if k.contains(|c: char| c.is_whitespace() || c.is_control()) || v.contains(['\r', '\n'])
            {
                return fail(format!("En-tête invalide : {k}"));
            }
            has_content_type |= k.eq_ignore_ascii_case("content-type");
            headers.push((k.to_string(), v.trim().to_string()));
        }
    }
    let body = p.str("body");
    let body = if !body.trim().is_empty() && method != "GET" {
        if !has_content_type && serde_json::from_str::<serde_json::Value>(&body).is_ok() {
            headers.push(("Content-Type".into(), "application/json".into()));
        }
        Some(body)
    } else {
        None
    };
    Ok(HttpRequest {
        method,
        url,
        headers,
        body,
    })
}

/// Convertit une réponse en table (JSON → lignes, sinon une ligne `status` / `body`).
pub fn response_to_batch(status: u16, text: String, items_path: &str) -> Result<RecordBatch> {
    if text.len() > MAX_RESPONSE_BYTES {
        return fail("Réponse trop volumineuse (50 Mo max.)");
    }
    if !(200..300).contains(&status) {
        let snippet: String = text.chars().take(200).collect();
        return fail(format!("HTTP {status} : {snippet}"));
    }
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(doc) => json_to_batch(&navigate(doc, items_path)?),
        Err(_) => json_to_batch(&serde_json::json!({ "status": status, "body": text })),
    }
}

/// Paramètres acceptant des expressions `{{ … }}`.
pub const TEMPLATED: [&str; 3] = ["url", "headers", "body"];

/// Remplace les expressions `{{ … }}` des paramètres concernés.
pub fn render_params(p: &Params, keys: &[&str], scope: &Scope) -> Result<Map<String, Json>> {
    let mut out = p.0.clone();
    for key in keys {
        let text = p.str(key);
        if crate::expr::has_expressions(&text) {
            let rendered = crate::expr::render_once(&text, scope)
                .map_err(|e| crate::error::EngineError::Msg(format!("{key} : {}", e.message)))?;
            out.insert((*key).to_string(), Json::String(rendered));
        }
    }
    Ok(out)
}

/// Exécute la requête décrite par les paramètres et renvoie la table obtenue.
pub async fn request(p: &Params<'_>, host: &dyn Host, scope: &Scope<'_>) -> Result<RecordBatch> {
    let params = render_params(p, &TEMPLATED, scope)?;
    let p = &Params(&params);
    let req = build_request(p)?;
    let (status, text) = send(req, host).await?;
    response_to_batch(status, text, &p.str("items_path"))
}

/// Nombre maximal de requêtes en mode « pour chaque ligne ».
pub const MAX_PER_ROW: usize = 500;

/// Nœud « Requête HTTP » : une seule requête (expressions évaluées sur la première ligne
/// de l'entrée éventuelle) ou une requête par ligne, réponses réunies dans une table.
pub async fn run(
    p: &Params<'_>,
    host: &dyn Host,
    ctx: &super::NodeCtx<'_>,
    input: Option<&RecordBatch>,
) -> Result<RecordBatch> {
    let scope = ctx.scope(input);
    let per_row = p.str("each") == "pour chaque ligne";
    let Some(input) = input.filter(|_| per_row) else {
        return request(p, host, &scope).await;
    };
    if input.num_rows() > MAX_PER_ROW {
        return fail(format!(
            "{MAX_PER_ROW} requêtes au maximum par exécution ({} lignes en entrée)",
            input.num_rows()
        ));
    }
    let mut out = Vec::with_capacity(input.num_rows());
    for r in 0..input.num_rows() {
        let row = input.slice(r, 1);
        let scope = ctx.scope(Some(&row));
        let batch = request(p, host, &scope)
            .await
            .map_err(|e| crate::error::EngineError::Msg(format!("Ligne {} : {e}", r + 1)))?;
        out.push(batch);
    }
    crate::data::append_batches(&out)
}

#[cfg(not(target_arch = "wasm32"))]
async fn send(req: HttpRequest, host: &dyn Host) -> Result<(u16, String)> {
    let res = host.http(req).await?;
    Ok((res.status, res.body))
}

#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    extern "C" {
        /// `fetch` global : fonctionne dans une page comme dans un Web Worker.
        #[wasm_bindgen(js_name = fetch)]
        pub fn global_fetch(req: &web_sys::Request) -> js_sys::Promise;
    }

    pub fn js_error(e: JsValue) -> crate::error::EngineError {
        let msg = e
            .dyn_ref::<js_sys::Error>()
            .map(|e| String::from(e.message()))
            .or_else(|| e.as_string())
            .unwrap_or_else(|| format!("{e:?}"));
        crate::error::EngineError::Msg(format!("Requête impossible : {msg} (réseau ou CORS ?)"))
    }
}

#[cfg(target_arch = "wasm32")]
async fn send(req: HttpRequest, _host: &dyn Host) -> Result<(u16, String)> {
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;
    use web::js_error;

    let init = web_sys::RequestInit::new();
    init.set_method(&req.method);
    // Jamais de cookies ni d'identifiants du navigateur vers des API tierces.
    init.set_credentials(web_sys::RequestCredentials::Omit);
    init.set_mode(web_sys::RequestMode::Cors);
    init.set_cache(web_sys::RequestCache::NoStore);
    init.set_referrer_policy(web_sys::ReferrerPolicy::NoReferrer);
    let headers = web_sys::Headers::new().map_err(js_error)?;
    for (k, v) in &req.headers {
        headers.set(k, v).map_err(js_error)?;
    }
    init.set_headers(&headers);
    if let Some(body) = &req.body {
        init.set_body(&JsValue::from_str(body));
    }

    let request = web_sys::Request::new_with_str_and_init(&req.url, &init).map_err(js_error)?;
    let resp: web_sys::Response = JsFuture::from(web::global_fetch(&request))
        .await
        .map_err(js_error)?
        .dyn_into()
        .map_err(js_error)?;
    let declared = resp
        .headers()
        .get("content-length")
        .ok()
        .flatten()
        .and_then(|v| v.parse::<usize>().ok());
    if declared.is_some_and(|n| n > MAX_RESPONSE_BYTES) {
        return fail("Réponse trop volumineuse (50 Mo max.)");
    }
    let text = JsFuture::from(resp.text().map_err(js_error)?)
        .await
        .map_err(js_error)?
        .as_string()
        .unwrap_or_default();
    Ok((resp.status(), text))
}
