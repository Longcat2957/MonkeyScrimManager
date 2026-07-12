use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::{
    aead::{self, Aad, LessSafeKey, Nonce, UnboundKey},
    digest::{SHA256, digest},
    hmac,
    rand::{SecureRandom, SystemRandom},
};
use secrecy::{ExposeSecret, SecretString};
use thiserror::Error;
use uuid::Uuid;

use super::CookiePolicy;

const SECRET_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;
const ENVELOPE_VERSION: u8 = 1;
const ENVELOPE_HEADER_BYTES: usize = 1 + NONCE_BYTES;
const TAG_BYTES: usize = 16;

#[derive(Clone)]
pub struct AuthService {
    rng: SystemRandom,
    pkce_key: LessSafeKey,
    cookies: CookiePolicy,
}

impl AuthService {
    pub fn new(
        encryption_key: &[u8; SECRET_BYTES],
        cookies: CookiePolicy,
    ) -> Result<Self, AuthCryptoError> {
        let key = UnboundKey::new(&aead::AES_256_GCM, encryption_key)
            .map_err(|_| AuthCryptoError::InvalidEncryptionKey)?;
        Ok(Self {
            rng: SystemRandom::new(),
            pkce_key: LessSafeKey::new(key),
            cookies,
        })
    }

    pub fn begin_oauth(&self, attempt_id: Uuid) -> Result<OAuthStartMaterial, AuthCryptoError> {
        let state = self.random_secret()?;
        let verifier = self.random_secret()?;
        let state_hash = Self::hash_secret(state.expose_secret());
        let pkce_challenge = pkce_s256(verifier.expose_secret());
        let pkce_verifier_ciphertext = self.encrypt_pkce(attempt_id, &verifier)?;

        Ok(OAuthStartMaterial {
            state,
            state_hash,
            pkce_challenge,
            pkce_verifier_ciphertext,
        })
    }

    pub fn decrypt_pkce(
        &self,
        attempt_id: Uuid,
        ciphertext: &[u8],
    ) -> Result<SecretString, AuthCryptoError> {
        if ciphertext.len() < ENVELOPE_HEADER_BYTES + TAG_BYTES || ciphertext[0] != ENVELOPE_VERSION
        {
            return Err(AuthCryptoError::InvalidCiphertext);
        }

        let nonce_bytes: [u8; NONCE_BYTES] = ciphertext[1..ENVELOPE_HEADER_BYTES]
            .try_into()
            .map_err(|_| AuthCryptoError::InvalidCiphertext)?;
        let mut encrypted = ciphertext[ENVELOPE_HEADER_BYTES..].to_vec();
        let plaintext = self
            .pkce_key
            .open_in_place(
                Nonce::assume_unique_for_key(nonce_bytes),
                Aad::from(attempt_id.as_bytes()),
                &mut encrypted,
            )
            .map_err(|_| AuthCryptoError::InvalidCiphertext)?;
        let verifier = std::str::from_utf8(plaintext)
            .map_err(|_| AuthCryptoError::InvalidCiphertext)?
            .to_owned();
        if !is_pkce_verifier(&verifier) {
            return Err(AuthCryptoError::InvalidCiphertext);
        }
        Ok(SecretString::from(verifier))
    }

    pub fn new_session(&self) -> Result<NewSessionMaterial, AuthCryptoError> {
        let token = self.random_secret()?;
        let csrf_token = self.random_secret()?;
        let token_hash = Self::hash_secret(token.expose_secret());
        let csrf_secret_hash = Self::hash_secret(csrf_token.expose_secret());

        Ok(NewSessionMaterial {
            token,
            token_hash,
            csrf_token,
            csrf_secret_hash,
        })
    }

    pub fn hash_secret(value: &str) -> Vec<u8> {
        digest(&SHA256, value.as_bytes()).as_ref().to_vec()
    }

    pub fn secrets_equal(left: &str, right: &str) -> bool {
        let key = hmac::Key::new(hmac::HMAC_SHA256, b"monkeybot-secret-comparison-v1");
        let expected = hmac::sign(&key, left.as_bytes());
        hmac::verify(&key, right.as_bytes(), expected.as_ref()).is_ok()
    }

    pub fn hash_matches(value: &str, expected_hash: &[u8]) -> bool {
        let key = hmac::Key::new(hmac::HMAC_SHA256, b"monkeybot-hash-comparison-v1");
        let actual = Self::hash_secret(value);
        let expected_tag = hmac::sign(&key, expected_hash);
        hmac::verify(&key, &actual, expected_tag.as_ref()).is_ok()
    }

    pub const fn cookies(&self) -> &CookiePolicy {
        &self.cookies
    }

    fn random_secret(&self) -> Result<SecretString, AuthCryptoError> {
        let mut bytes = [0_u8; SECRET_BYTES];
        self.rng
            .fill(&mut bytes)
            .map_err(|_| AuthCryptoError::RandomnessUnavailable)?;
        Ok(SecretString::from(URL_SAFE_NO_PAD.encode(bytes)))
    }

    fn encrypt_pkce(
        &self,
        attempt_id: Uuid,
        verifier: &SecretString,
    ) -> Result<Vec<u8>, AuthCryptoError> {
        let mut nonce_bytes = [0_u8; NONCE_BYTES];
        self.rng
            .fill(&mut nonce_bytes)
            .map_err(|_| AuthCryptoError::RandomnessUnavailable)?;
        let mut encrypted = verifier.expose_secret().as_bytes().to_vec();
        self.pkce_key
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce_bytes),
                Aad::from(attempt_id.as_bytes()),
                &mut encrypted,
            )
            .map_err(|_| AuthCryptoError::EncryptionFailed)?;

        let mut envelope = Vec::with_capacity(ENVELOPE_HEADER_BYTES + encrypted.len());
        envelope.push(ENVELOPE_VERSION);
        envelope.extend_from_slice(&nonce_bytes);
        envelope.extend_from_slice(&encrypted);
        Ok(envelope)
    }
}

pub struct OAuthStartMaterial {
    pub state: SecretString,
    pub state_hash: Vec<u8>,
    pub pkce_challenge: String,
    pub pkce_verifier_ciphertext: Vec<u8>,
}

pub struct NewSessionMaterial {
    pub token: SecretString,
    pub token_hash: Vec<u8>,
    pub csrf_token: SecretString,
    pub csrf_secret_hash: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AuthCryptoError {
    #[error("OAuth encryption key is invalid")]
    InvalidEncryptionKey,
    #[error("operating system randomness is unavailable")]
    RandomnessUnavailable,
    #[error("OAuth verifier encryption failed")]
    EncryptionFailed,
    #[error("OAuth verifier ciphertext is invalid")]
    InvalidCiphertext,
}

fn is_pkce_verifier(value: &str) -> bool {
    (43..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'))
}

fn pkce_s256(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(digest(&SHA256, verifier.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> AuthService {
        AuthService::new(
            &[7_u8; SECRET_BYTES],
            CookiePolicy::new(false, std::time::Duration::from_secs(3600)),
        )
        .expect("valid key")
    }

    #[test]
    fn creates_pkce_s256_material_and_round_trips_the_verifier() {
        let service = service();
        let attempt_id = Uuid::now_v7();
        let material = service.begin_oauth(attempt_id).expect("OAuth material");
        let verifier = service
            .decrypt_pkce(attempt_id, &material.pkce_verifier_ciphertext)
            .expect("decrypt verifier");

        assert_eq!(material.state.expose_secret().len(), 43);
        assert_eq!(material.state_hash.len(), 32);
        assert_eq!(material.pkce_challenge.len(), 43);
        assert!(is_pkce_verifier(verifier.expose_secret()));
        assert_eq!(material.pkce_challenge, pkce_s256(verifier.expose_secret()));
        assert!(
            !material
                .pkce_verifier_ciphertext
                .windows(verifier.expose_secret().len())
                .any(|window| window == verifier.expose_secret().as_bytes())
        );
    }

    #[test]
    fn pkce_s256_matches_the_rfc_7636_vector() {
        assert_eq!(
            pkce_s256("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn ciphertext_is_bound_to_the_attempt_and_rejects_tampering() {
        let service = service();
        let attempt_id = Uuid::now_v7();
        let material = service.begin_oauth(attempt_id).expect("OAuth material");

        assert!(matches!(
            service.decrypt_pkce(Uuid::now_v7(), &material.pkce_verifier_ciphertext),
            Err(AuthCryptoError::InvalidCiphertext)
        ));

        let mut tampered = material.pkce_verifier_ciphertext;
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(matches!(
            service.decrypt_pkce(attempt_id, &tampered),
            Err(AuthCryptoError::InvalidCiphertext)
        ));
    }

    #[test]
    fn session_material_uses_independent_high_entropy_values() {
        let material = service().new_session().expect("session material");

        assert_eq!(material.token.expose_secret().len(), 43);
        assert_eq!(material.csrf_token.expose_secret().len(), 43);
        assert_ne!(
            material.token.expose_secret(),
            material.csrf_token.expose_secret()
        );
        assert_eq!(material.token_hash.len(), 32);
        assert_eq!(material.csrf_secret_hash.len(), 32);
        assert!(AuthService::secrets_equal("same", "same"));
        assert!(!AuthService::secrets_equal("same", "different"));
        assert!(AuthService::hash_matches(
            material.csrf_token.expose_secret(),
            &material.csrf_secret_hash
        ));
        assert!(!AuthService::hash_matches(
            "wrong",
            &material.csrf_secret_hash
        ));
    }
}
