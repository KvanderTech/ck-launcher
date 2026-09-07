use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use sha2::{Digest, Sha256};

pub struct Pkce {
    verifier: String,
    challenge: String,
}

impl Pkce {
    pub fn verifier(&self) -> &str {
        &self.verifier
    }

    pub fn challenge(&self) -> &str {
        &self.challenge
    }
}

pub fn generate_pkce() -> Pkce {
    let mut entropy = [0_u8; 32];
    rand::rng().fill_bytes(&mut entropy);
    let verifier = URL_SAFE_NO_PAD.encode(entropy);
    let challenge = derive_challenge(&verifier);

    Pkce {
        verifier,
        challenge,
    }
}

pub fn derive_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{derive_challenge, generate_pkce};

    #[test]
    fn known_verifier_produces_sha256_base64url_challenge() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";

        assert_eq!(
            derive_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn generated_pkce_has_enough_entropy_and_no_padding() {
        let pkce = generate_pkce();

        assert!((43..=128).contains(&pkce.verifier().len()));
        assert_eq!(pkce.challenge().len(), 43);
        assert!(!pkce.verifier().contains('='));
        assert!(!pkce.challenge().contains('='));
        assert!(pkce
            .verifier()
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-._~".contains(character)));
    }
}
