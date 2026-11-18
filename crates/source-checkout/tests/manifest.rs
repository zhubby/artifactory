//! [INPUT]: 依赖固定清单及来源校验。
//! [OUTPUT]: 验证元数据篡改与路径越界被拒绝。
//! [POS]: 共享数据边界的回归测试。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_source_checkout::{
    SourceManifest, SourceRepository, resolve_submodule, same_origin, validate_repository,
};
fn repository() -> SourceRepository {
    SourceRepository {
        path: String::new(),
        upstream_url: "https://git.example/team/root.git".into(),
        commit_sha: "a".repeat(40),
        tags: Default::default(),
        lfs_objects: Vec::new(),
    }
}
#[test]
fn tampered_manifest_and_duplicate_paths_are_rejected() {
    let mut manifest = SourceManifest::new(uuid::Uuid::new_v4(), vec![repository()]).unwrap();
    manifest.repositories[0].commit_sha = "b".repeat(40);
    assert!(manifest.validate().is_err());
    assert!(SourceManifest::new(uuid::Uuid::new_v4(), vec![repository(), repository()]).is_err());
}
#[test]
fn unsafe_urls_and_cross_origin_credentials_are_rejected() {
    for url in [
        "file:///tmp/repo",
        "ext::command",
        "https://user:secret@git.example/repo",
        "https://git.example/repo?token=x",
    ] {
        assert!(validate_repository(url).is_err(), "{url}");
    }
    assert!(validate_repository("http://git.example/team/repo.git").is_ok());
    let child = resolve_submodule("https://git.example/team/root.git", "../child.git").unwrap();
    assert_eq!(child, "https://git.example/team/child.git");
    assert!(same_origin("https://git.example/root.git", &child));
    assert!(!same_origin(
        "https://git.example/root.git",
        "http://git.example/root.git"
    ));
    assert!(!same_origin(
        "https://git.example/root.git",
        "https://other.example/root.git"
    ));
}
