use argon2::{password_hash::{rand_core::OsRng, PasswordHasher, SaltString}, Argon2};

use crate::domain::errors::DomainError;

pub fn hash_password(password: &str) -> Result<String, DomainError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| DomainError::Configuration(format!("password hashing failed: {e}")))
}
