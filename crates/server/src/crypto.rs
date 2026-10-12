//! Chiffrement des identifiants au repos (AES-256-GCM, nonce aléatoire de 96 bits).
//! L'identifiant de l'enregistrement est lié au chiffré (données associées) : un secret
//! ne peut pas être recopié sur un autre identifiant.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};

const NONCE_LEN: usize = 12;

pub struct Vault {
    cipher: Aes256Gcm,
}

impl Vault {
    pub fn new(key: &[u8; 32]) -> Self {
        Self {
            cipher: Aes256Gcm::new(key.into()),
        }
    }

    pub fn encrypt(&self, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, String> {
        let mut nonce = [0u8; NONCE_LEN];
        getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
        let mut out = nonce.to_vec();
        let ct = self
            .cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| "chiffrement impossible".to_string())?;
        out.extend(ct);
        Ok(out)
    }

    pub fn decrypt(&self, data: &[u8], aad: &[u8]) -> Result<Vec<u8>, String> {
        if data.len() <= NONCE_LEN {
            return Err("identifiant chiffré invalide".into());
        }
        let (nonce, ct) = data.split_at(NONCE_LEN);
        self.cipher
            .decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad })
            .map_err(|_| "déchiffrement impossible (clé CREDENTIALS_KEY modifiée ?)".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::Vault;

    #[test]
    fn roundtrip_and_binding() {
        let v = Vault::new(&[7u8; 32]);
        let c = v.encrypt(b"secret", b"id-1").unwrap();
        assert_ne!(&c[12..], b"secret");
        assert_eq!(v.decrypt(&c, b"id-1").unwrap(), b"secret");
        assert!(v.decrypt(&c, b"id-2").is_err(), "lié à l'identifiant");
        assert!(
            Vault::new(&[8u8; 32]).decrypt(&c, b"id-1").is_err(),
            "autre clé"
        );
        let mut tampered = c.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert!(v.decrypt(&tampered, b"id-1").is_err(), "intégrité");
    }
}
