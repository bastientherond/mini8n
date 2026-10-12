//! Requêtes HTTP sortantes des workflows exécutés côté serveur.
//!
//! Protection SSRF : seules les adresses publiques sont joignables (pas de boucle locale,
//! de réseaux privés — dont les réseaux Docker internes —, de lien local / métadonnées cloud…).
//! Le contrôle porte sur l'adresse résolue (résolveur DNS filtrant), sur les URL contenant
//! directement une IP, et sur chaque redirection.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use mini8n_engine::{HttpRequest, HttpResponse};
use reqwest::Url;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::redirect::Policy;

pub const MAX_RESPONSE_BYTES: usize = 50 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;

/// Adresse joignable depuis le serveur (hors réseaux privés, réservés, multicast…).
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public_v4(v4);
            }
            let s = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // unique local fc00::/7
                || (s[0] & 0xffc0) == 0xfe80 // lien local fe80::/10
                || (s[0] == 0x64 && s[1] == 0xff9b) // NAT64 64:ff9b::/96
                || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
                || s[0] == 0x0100) // discard 100::/64
        }
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local() // 169.254/16 (métadonnées cloud)
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || o[0] == 0
        || (o[0] == 100 && (o[1] & 0xc0) == 64) // CGNAT 100.64/10
        || (o[0] == 192 && o[1] == 0 && o[2] == 0) // 192.0.0/24
        || (o[0] == 198 && (o[1] & 0xfe) == 18) // benchmark 198.18/15
        || o[0] >= 240) // réservé
}

/// Règle de liste blanche : nom d'hôte exact ou réseau CIDR.
#[derive(Debug, Clone, PartialEq)]
enum AllowRule {
    Host(String),
    Net(IpAddr, u8),
}

/// Politique d'accès réseau des workflows : adresses publiques, plus les hôtes ou réseaux
/// privés explicitement autorisés (`NETWORK_ALLOWLIST`), ou tout si `HTTP_ALLOW_PRIVATE`.
#[derive(Debug, Clone, Default)]
pub struct NetPolicy {
    allow_private: bool,
    allowlist: Vec<AllowRule>,
}

fn in_cidr(ip: IpAddr, net: IpAddr, prefix: u8) -> bool {
    match (ip, net) {
        (IpAddr::V4(a), IpAddr::V4(b)) if prefix <= 32 => {
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            u32::from(a) & mask == u32::from(b) & mask
        }
        (IpAddr::V6(a), IpAddr::V6(b)) if prefix <= 128 => {
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix)
            };
            u128::from(a) & mask == u128::from(b) & mask
        }
        _ => false,
    }
}

impl NetPolicy {
    pub fn new(allow_private: bool, allowlist: &[String]) -> Self {
        let allowlist = allowlist
            .iter()
            .filter_map(|entry| {
                let e = entry.trim().to_ascii_lowercase();
                if let Some((net, prefix)) = e.split_once('/') {
                    Some(AllowRule::Net(net.parse().ok()?, prefix.parse().ok()?))
                } else if let Ok(ip) = e.parse::<IpAddr>() {
                    Some(AllowRule::Net(ip, if ip.is_ipv4() { 32 } else { 128 }))
                } else if !e.is_empty() {
                    Some(AllowRule::Host(e))
                } else {
                    None
                }
            })
            .collect();
        Self {
            allow_private,
            allowlist,
        }
    }

    /// Adresse `ip` (résolue depuis `host`) joignable ?
    pub fn allows(&self, host: &str, ip: IpAddr) -> bool {
        self.allow_private
            || is_public_ip(ip)
            || self.allowlist.iter().any(|r| match r {
                AllowRule::Host(h) => h.eq_ignore_ascii_case(host.trim_end_matches('.')),
                AllowRule::Net(net, prefix) => in_cidr(ip, *net, *prefix),
            })
    }

    /// Résout `host` et renvoie une adresse autorisée (bases de données, SMTP…).
    pub async fn resolve(&self, host: &str, port: u16) -> Result<SocketAddr, String> {
        let host = host.trim();
        if host.is_empty() {
            return Err("hôte manquant".into());
        }
        let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
            .await
            .map_err(|e| format!("résolution de « {host} » impossible : {e}"))?
            .collect();
        addrs
            .into_iter()
            .find(|a| self.allows(host, a.ip()))
            .ok_or_else(|| {
                format!("adresse non autorisée pour « {host} » (réseau privé : ajoutez-le à NETWORK_ALLOWLIST)")
            })
    }
}

/// Résolveur DNS qui écarte les adresses non autorisées.
struct GuardedResolver {
    policy: NetPolicy,
}

impl Resolve for GuardedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let policy = self.policy.clone();
        Box::pin(async move {
            let host = name.as_str().to_string();
            let addrs: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            let allowed: Vec<SocketAddr> = addrs
                .into_iter()
                .filter(|a| policy.allows(&host, a.ip()))
                .collect();
            if allowed.is_empty() {
                return Err(format!(
                    "adresse non autorisée pour « {host} » (réseau privé ou réservé)"
                )
                .into());
            }
            Ok(Box::new(allowed.into_iter()) as Addrs)
        })
    }
}

/// Contrôle d'une URL (schéma, IP littérale).
fn check_url(url: &reqwest::Url, policy: &NetPolicy) -> Result<(), String> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err("seules les URL http(s) sont autorisées".into());
    }
    let ip = match url.host() {
        Some(url::Host::Ipv4(ip)) => Some(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => Some(IpAddr::V6(ip)),
        Some(url::Host::Domain(_)) => None,
        None => return Err("URL sans hôte".into()),
    };
    if let Some(ip) = ip
        && !policy.allows(&ip.to_string(), ip)
    {
        return Err(format!(
            "adresse non autorisée : {ip} (réseau privé ou réservé)"
        ));
    }
    Ok(())
}

/// Exécute une requête HTTP pour un workflow (délai de 30 s, réponse limitée à 50 Mo).
pub async fn fetch(req: HttpRequest, policy: &NetPolicy) -> Result<HttpResponse, String> {
    let url = Url::parse(&req.url).map_err(|e| format!("URL invalide : {e}"))?;
    check_url(&url, policy)?;
    let redirect_policy = policy.clone();
    let method = reqwest::Method::from_bytes(req.method.as_bytes())
        .map_err(|_| "méthode invalide".to_string())?;

    let client = reqwest::Client::builder()
        .dns_resolver(Arc::new(GuardedResolver {
            policy: policy.clone(),
        }))
        .redirect(Policy::custom(move |attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                attempt.error("trop de redirections")
            } else if let Err(e) = check_url(attempt.url(), &redirect_policy) {
                attempt.error(e)
            } else {
                attempt.follow()
            }
        }))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("mini8n/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("client HTTP : {e}"))?;

    let mut builder = client.request(method, url);
    for (k, v) in &req.headers {
        if k.eq_ignore_ascii_case("host") {
            continue;
        }
        builder = builder.header(k.as_str(), v.as_str());
    }
    if let Some(body) = req.body {
        builder = builder.body(body);
    }
    let mut resp = builder.send().await.map_err(describe)?;
    let status = resp.status().as_u16();
    if resp
        .content_length()
        .is_some_and(|n| n as usize > MAX_RESPONSE_BYTES)
    {
        return Err("réponse trop volumineuse (50 Mo max.)".into());
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(describe)? {
        if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err("réponse trop volumineuse (50 Mo max.)".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(HttpResponse {
        status,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

/// Message d'erreur lisible (cause racine incluse : adresse refusée, délai…).
fn describe(e: reqwest::Error) -> String {
    let mut msg = if e.is_timeout() {
        "délai dépassé".to_string()
    } else if e.is_connect() {
        "connexion impossible".to_string()
    } else if e.is_redirect() {
        "redirection refusée".to_string()
    } else {
        "requête impossible".to_string()
    };
    let mut source = std::error::Error::source(&e);
    while let Some(s) = source {
        let text = s.to_string();
        if text.contains("non autorisée") || text.contains("redirection") {
            msg = format!("{msg} : {text}");
            break;
        }
        source = s.source();
    }
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_ips() {
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(is_public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.18.0.2",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
        ] {
            assert!(!is_public_ip(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn urls() {
        let strict = NetPolicy::new(false, &[]);
        let ok = |u: &str| check_url(&Url::parse(u).unwrap(), &strict).is_ok();
        assert!(ok("https://api.example.com/x"));
        assert!(!ok("http://127.0.0.1:3000/api"));
        assert!(!ok("http://[::1]/"));
        assert!(!ok("http://169.254.169.254/latest/meta-data"));
        assert!(!ok("file:///etc/passwd"));
        assert!(
            check_url(
                &Url::parse("http://127.0.0.1/").unwrap(),
                &NetPolicy::new(true, &[])
            )
            .is_ok()
        );
    }

    #[test]
    fn allowlist() {
        let p = NetPolicy::new(
            false,
            &[
                "db.interne".into(),
                "10.1.0.0/16".into(),
                "192.168.1.7".into(),
            ],
        );
        assert!(p.allows("db.interne", "10.9.9.9".parse().unwrap()));
        assert!(p.allows("x", "10.1.200.3".parse().unwrap()));
        assert!(!p.allows("x", "10.2.0.1".parse().unwrap()));
        assert!(p.allows("x", "192.168.1.7".parse().unwrap()));
        assert!(!p.allows("x", "192.168.1.8".parse().unwrap()));
        assert!(p.allows("x", "8.8.8.8".parse().unwrap()));
    }

    #[tokio::test]
    async fn blocks_private_hosts_end_to_end() {
        let req = |url: &str| HttpRequest {
            method: "GET".into(),
            url: url.into(),
            headers: vec![],
            body: None,
        };
        let strict = NetPolicy::new(false, &[]);
        let err = fetch(req("http://localhost:9/"), &strict)
            .await
            .unwrap_err();
        assert!(err.contains("non autorisée"), "{err}");
        let err = fetch(req("http://10.0.0.1/"), &strict).await.unwrap_err();
        assert!(err.contains("non autorisée"), "{err}");
    }
}
