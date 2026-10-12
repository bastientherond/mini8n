//! Environnement d'exécution fourni par l'hôte (navigateur ou serveur).
//!
//! Le moteur ne fait ni réseau ni accès aux autres workflows lui-même : il délègue
//! à l'hôte, qui applique ses propres règles (protection SSRF côté serveur,
//! contrôle d'accès au workspace, profondeur maximale des sous-workflows…).

use std::future::Future;
use std::pin::Pin;

use arrow::array::RecordBatch;

use crate::error::{Result, fail};

/// Future non `Send` (compatible WebAssembly).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

pub trait Host {
    /// Requête HTTP sortante (côté serveur ; le navigateur utilise `fetch` directement).
    fn http<'a>(&'a self, _req: HttpRequest) -> BoxFuture<'a, Result<HttpResponse>> {
        Box::pin(async {
            fail("Les requêtes HTTP ne sont pas disponibles dans cet environnement")
        })
    }

    /// Exécute un autre workflow du même workspace avec `input` et renvoie sa sortie.
    fn call_workflow<'a>(
        &'a self,
        _workflow_id: &'a str,
        _input: RecordBatch,
    ) -> BoxFuture<'a, Result<RecordBatch>> {
        Box::pin(async {
            fail("L'appel d'un autre workflow n'est pas disponible dans cet environnement")
        })
    }
}

/// Hôte minimal (tests, exécution hors ligne).
pub struct NoHost;

impl Host for NoHost {}

/// Horodatage courant en millisecondes depuis l'époque Unix.
#[cfg(target_arch = "wasm32")]
pub fn now_ms() -> f64 {
    js_sys::Date::now()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn now_ms() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

/// Date ISO 8601 (UTC) à partir d'un horodatage Unix en millisecondes.
pub fn iso_from_ms(ms: f64) -> String {
    let total_secs = (ms / 1000.0).floor() as i64;
    let days = total_secs.div_euclid(86_400);
    let secs = total_secs.rem_euclid(86_400);
    // Conversion jours → date civile (algorithme de Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

pub fn now_iso() -> String {
    iso_from_ms(now_ms())
}

#[cfg(test)]
mod tests {
    use super::iso_from_ms;

    #[test]
    fn iso_dates() {
        assert_eq!(iso_from_ms(0.0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_from_ms(1_791_812_591_676.0), "2026-10-12T13:43:11Z");
        assert_eq!(iso_from_ms(951_782_400_000.0), "2000-02-29T00:00:00Z");
    }
}
