use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use cicd_app::repository_sync::{Crypto, Error};
use hmac::{Hmac, Mac};
use sha2::Sha256;

pub struct Secrets(pub Option<[u8; 32]>);
impl Secrets {
    fn key(&self) -> Result<[u8; 32], Error> {
        self.0
            .ok_or(Error::Unavailable("CICD_SECRETS_KEY не настроен"))
    }
}
impl Crypto for Secrets {
    fn seal(&self, value: &str) -> Result<String, Error> {
        let key = self.key()?;
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|_| Error::Unavailable("Некорректный ключ шифрования"))?;
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(&nonce, value.as_bytes())
            .map_err(|_| Error::Unavailable("Не удалось зашифровать credential"))?;
        Ok(format!(
            "v1:{}:{}",
            BASE64.encode(nonce),
            BASE64.encode(ciphertext)
        ))
    }
    fn open(&self, value: &str) -> Result<String, Error> {
        let key = self.key()?;
        let parts: Vec<_> = value.splitn(3, ':').collect();
        if parts.len() != 3 || parts[0] != "v1" {
            return Err(Error::Unavailable("Неизвестный формат credential"));
        }
        let bytes = BASE64
            .decode(parts[1])
            .map_err(|_| Error::Unavailable("Повреждённый credential"))?;
        if bytes.len() != 12 {
            return Err(Error::Unavailable("Повреждённый nonce credential"));
        }
        let ciphertext = BASE64
            .decode(parts[2])
            .map_err(|_| Error::Unavailable("Повреждённый credential"))?;
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|_| Error::Unavailable("Некорректный ключ шифрования"))?;
        let plain = cipher
            .decrypt(Nonce::from_slice(&bytes), ciphertext.as_ref())
            .map_err(|_| Error::Unavailable("Не удалось расшифровать credential"))?;
        String::from_utf8(plain).map_err(|_| Error::Unavailable("Некорректный credential"))
    }
    fn fingerprint(&self, value: &[u8]) -> Result<String, Error> {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.key()?)
            .map_err(|_| Error::Unavailable("Некорректный ключ"))?;
        mac.update(b"forge-repository-sync/v1\0");
        mac.update(value);
        Ok(mac
            .finalize()
            .into_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credential_encryption_roundtrip_and_corrupt_nonce() {
        let crypto = Secrets(Some([7; 32]));
        let value = crypto.seal("sample-token").unwrap();
        assert!(!value.contains("sample-token"));
        assert_eq!(crypto.open(&value).unwrap(), "sample-token");
        assert!(crypto.open("v1:YQ==:YQ==").is_err());
        assert!(Secrets(Some([8; 32])).open(&value).is_err());
        assert_ne!(
            crypto.fingerprint(b"a").unwrap(),
            crypto.fingerprint(b"b").unwrap()
        );
    }
}
