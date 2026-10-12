//! Validation des entrées utilisateur.

use crate::error::{AppError, AppResult};

pub const PASSWORD_MIN: usize = 12;
pub const PASSWORD_MAX: usize = 128;

/// Retourne (e-mail tel que saisi, e-mail normalisé).
pub fn email(raw: &str) -> AppResult<(String, String)> {
    let email = raw.trim();
    let invalid = || AppError::bad("Adresse e-mail invalide");
    if email.len() > 254 || email.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(invalid());
    }
    let (local, domain) = email.split_once('@').ok_or_else(invalid)?;
    if local.is_empty()
        || local.len() > 64
        || domain.contains('@')
        || !domain.contains('.')
        || domain.starts_with('.')
        || domain.ends_with('.')
        || domain.contains("..")
    {
        return Err(invalid());
    }
    Ok((email.to_string(), email.to_lowercase()))
}

/// Libellé court (nom d'utilisateur, de workspace, de workflow).
pub fn label(raw: &str, what: &str, max: usize) -> AppResult<String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(AppError::bad(format!("{what} requis")));
    }
    if s.chars().count() > max {
        return Err(AppError::bad(format!(
            "{what} trop long ({max} caractères max.)"
        )));
    }
    if s.chars().any(char::is_control) {
        return Err(AppError::bad(format!(
            "{what} contient des caractères interdits"
        )));
    }
    Ok(s.to_string())
}

pub fn password(pw: &str, email_normalized: &str) -> AppResult<()> {
    let n = pw.chars().count();
    if n < PASSWORD_MIN {
        return Err(AppError::bad(format!(
            "Le mot de passe doit contenir au moins {PASSWORD_MIN} caractères"
        )));
    }
    if n > PASSWORD_MAX {
        return Err(AppError::bad(format!(
            "Le mot de passe doit contenir au plus {PASSWORD_MAX} caractères"
        )));
    }
    let lower = pw.to_lowercase();
    let local = email_normalized.split('@').next().unwrap_or_default();
    if lower == email_normalized || (local.len() >= 4 && lower.contains(local)) {
        return Err(AppError::bad(
            "Le mot de passe ne doit pas contenir votre adresse e-mail",
        ));
    }
    if pw.chars().collect::<std::collections::HashSet<_>>().len() < 5 {
        return Err(AppError::bad("Le mot de passe est trop répétitif"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails() {
        assert_eq!(email(" Alice@Example.COM ").unwrap().1, "alice@example.com");
        for bad in [
            "", "a", "a@b", "a@@b.c", "a b@c.d", "@b.c", "a@.c", "a@b..c",
        ] {
            assert!(email(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn passwords() {
        assert!(password("court", "a@b.c").is_err());
        assert!(password("aaaaaaaaaaaaaaaa", "a@b.c").is_err());
        assert!(password("alice-secret-2026!", "alice@b.c").is_err());
        assert!(password("correct horse battery", "alice@b.c").is_ok());
    }
}
