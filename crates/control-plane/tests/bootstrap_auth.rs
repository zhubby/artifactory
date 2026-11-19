//! [INPUT]: 依赖首次管理员和常规密码散列入口。
//! [OUTPUT]: 验证初始密码可登录且不放宽注册和改密策略。
//! [POS]: 首次管理员密码策略的独立回归验收。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use argon2::{Argon2, PasswordHash, PasswordVerifier};
use artifactory_control_plane::auth::{bootstrap_password_hash, password_hash};

#[test]
fn fixed_initial_password_is_verifiable_and_salted() {
    let first = bootstrap_password_hash("123456").unwrap();
    let second = bootstrap_password_hash("123456").unwrap();
    assert_ne!(first, second);
    let hash = PasswordHash::new(&first).unwrap();
    assert!(Argon2::default().verify_password(b"123456", &hash).is_ok());
    assert!(
        Argon2::default()
            .verify_password(b"wrong-password", &hash)
            .is_err()
    );
}

#[test]
fn initial_password_exception_does_not_relax_other_passwords() {
    assert!(password_hash("123456").is_err());
    for password in ["", "654321", "short-pass"] {
        assert!(bootstrap_password_hash(password).is_err());
    }
    assert!(bootstrap_password_hash(&"a".repeat(257)).is_err());
    let custom = "custom-admin-password";
    let encoded = bootstrap_password_hash(custom).unwrap();
    assert!(
        Argon2::default()
            .verify_password(custom.as_bytes(), &PasswordHash::new(&encoded).unwrap())
            .is_ok()
    );
}
