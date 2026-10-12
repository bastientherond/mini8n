use arrow::array::RecordBatch;

use super::Params;
use crate::error::Result;

/// Suit un chemin `a.b.0.c` dans un document JSON.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn navigate(doc: serde_json::Value, path: &str) -> Result<serde_json::Value> {
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

#[cfg(not(target_arch = "wasm32"))]
pub async fn request(_p: &Params<'_>) -> Result<RecordBatch> {
    crate::error::fail("Le nœud HTTP n'est disponible que dans le navigateur (WebAssembly)")
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
const MAX_RESPONSE_BYTES: usize = 50 * 1024 * 1024;

#[cfg(target_arch = "wasm32")]
pub async fn request(p: &Params<'_>) -> Result<RecordBatch> {
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;

    use crate::data::json_to_batch;
    use crate::error::fail;
    use web::js_error;

    let url = p.str("url").trim().to_string();
    if url.is_empty() {
        return fail("URL requise");
    }
    // Uniquement des URL absolues http(s) : pas de data:, blob:, javascript:, ni de chemin relatif.
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return fail("L'URL doit commencer par https:// (ou http://)");
    }
    let method = p.str_or("method", "GET").to_uppercase();
    if !matches!(method.as_str(), "GET" | "POST" | "PUT" | "PATCH" | "DELETE") {
        return fail(format!("Méthode non autorisée : {method}"));
    }
    let init = web_sys::RequestInit::new();
    init.set_method(&method);
    // Jamais de cookies ni d'identifiants du navigateur vers des API tierces.
    init.set_credentials(web_sys::RequestCredentials::Omit);
    init.set_mode(web_sys::RequestMode::Cors);
    init.set_cache(web_sys::RequestCache::NoStore);
    init.set_referrer_policy(web_sys::ReferrerPolicy::NoReferrer);

    let headers = web_sys::Headers::new().map_err(js_error)?;
    let mut has_content_type = false;
    for line in p.str("headers").lines() {
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim();
            if k.is_empty() {
                continue;
            }
            has_content_type |= k.eq_ignore_ascii_case("content-type");
            headers.set(k, v.trim()).map_err(js_error)?;
        }
    }
    let body = p.str("body");
    if !body.trim().is_empty() && method != "GET" {
        if !has_content_type && serde_json::from_str::<serde_json::Value>(&body).is_ok() {
            headers
                .set("Content-Type", "application/json")
                .map_err(js_error)?;
        }
        init.set_body(&JsValue::from_str(&body));
    }
    init.set_headers(&headers);

    let req = web_sys::Request::new_with_str_and_init(&url, &init).map_err(js_error)?;
    let resp: web_sys::Response = JsFuture::from(web::global_fetch(&req))
        .await
        .map_err(js_error)?
        .dyn_into()
        .map_err(js_error)?;
    let status = resp.status();
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
    if text.len() > MAX_RESPONSE_BYTES {
        return fail("Réponse trop volumineuse (50 Mo max.)");
    }
    if !resp.ok() {
        let snippet: String = text.chars().take(200).collect();
        return fail(format!("HTTP {status} : {snippet}"));
    }
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(doc) => json_to_batch(&navigate(doc, &p.str("items_path"))?),
        Err(_) => json_to_batch(&serde_json::json!({ "status": status, "body": text })),
    }
}
