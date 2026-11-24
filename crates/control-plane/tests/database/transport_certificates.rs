//! [INPUT]: 依赖 rcgen 与独立测试密钥。
//! [OUTPUT]: 提供设备传输和终端路由共用的 CA、服务与设备证书。
//! [POS]: 仅供隔离传输验收，不读取部署 PKI。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use rcgen::{BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair};

pub(super) struct Certificates {
    pub(super) ca: String,
    issuer: rcgen::Issuer<'static, KeyPair>,
    pub(super) server: String,
    pub(super) server_key: String,
}

impl Certificates {
    pub(super) fn new() -> Self {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::default();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params
            .distinguished_name
            .push(DnType::CommonName, "Artifactory test CA");
        let ca = params.self_signed(&key).unwrap().pem();
        let issuer = rcgen::Issuer::new(params, key);
        let key = KeyPair::generate().unwrap();
        let mut params =
            CertificateParams::new(vec!["localhost".into(), "127.0.0.1".into()]).unwrap();
        params
            .distinguished_name
            .push(DnType::CommonName, "localhost");
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let server = params.signed_by(&key, &issuer).unwrap().pem();
        Self {
            ca,
            issuer,
            server,
            server_key: key.serialize_pem(),
        }
    }

    pub(super) fn device(&self) -> (String, String) {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::default();
        params
            .distinguished_name
            .push(DnType::CommonName, "Artifactory test device");
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        (
            params.signed_by(&key, &self.issuer).unwrap().pem(),
            key.serialize_pem(),
        )
    }
}
