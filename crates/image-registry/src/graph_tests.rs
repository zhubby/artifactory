//! [INPUT]: 依赖本机 HTTP OCI 夹具和摘要客户端。
//! [OUTPUT]: 验证完整图复制、真实架构与损坏拒绝。
//! [POS]: 无外部仓库依赖的镜像内容验收。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::Registry;
use artifactory_image_protocol::{
    MANIFEST_MEDIA_TYPE, PlatformResult, RegistryLogin, index, sha256,
};
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::State,
    http::{Request, Response, StatusCode},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Objects {
    manifests: BTreeMap<(String, String), Vec<u8>>,
    blobs: BTreeMap<(String, String), Vec<u8>>,
    writes: Vec<String>,
}
type Shared = Arc<Mutex<Objects>>;
async fn serve(State(objects): State<Shared>, request: Request<Body>) -> Response<Body> {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let path = uri.path().strip_prefix("/v2/").unwrap_or_default();
    let body = to_bytes(request.into_body(), 16 * 1024 * 1024)
        .await
        .unwrap();
    let mut objects = objects.lock().unwrap();
    let (status, body, location) = if let Some((repo, reference)) = path.split_once("/manifests/") {
        let key = (repo.to_owned(), reference.to_owned());
        if method == "GET" {
            match objects.manifests.get(&key) {
                Some(v) => (200, v.clone(), None),
                None => (404, vec![], None),
            }
        } else if method == "PUT" {
            let value: Value = serde_json::from_slice(&body).unwrap();
            let references_present = if let Some(children) = value["manifests"].as_array() {
                children.iter().all(|c| {
                    objects
                        .manifests
                        .contains_key(&(repo.to_owned(), c["digest"].as_str().unwrap().to_owned()))
                })
            } else {
                std::iter::once(&value["config"])
                    .chain(value["layers"].as_array().unwrap().iter())
                    .all(|c| {
                        objects.blobs.contains_key(&(
                            repo.to_owned(),
                            c["digest"].as_str().unwrap().to_owned(),
                        ))
                    })
            };
            if references_present {
                objects.manifests.insert(key, body.to_vec());
                objects.writes.push(reference.into());
                (201, vec![], None)
            } else {
                (400, vec![], None)
            }
        } else {
            (405, vec![], None)
        }
    } else if let Some((repo, id)) = path.split_once("/blobs/") {
        if id == "uploads/" && method == "POST" {
            (202, vec![], Some(format!("/v2/{repo}/blobs/uploads/test")))
        } else if id == "uploads/test" && method == "PUT" {
            let digest = url::form_urlencoded::parse(uri.query().unwrap_or_default().as_bytes())
                .find(|(k, _)| k == "digest")
                .unwrap()
                .1
                .into_owned();
            if sha256(&body) == digest {
                objects.blobs.insert((repo.into(), digest), body.to_vec());
                (201, vec![], None)
            } else {
                (400, vec![], None)
            }
        } else {
            match objects.blobs.get(&(repo.into(), id.into())) {
                Some(v) => (200, v.clone(), None),
                None => (404, vec![], None),
            }
        }
    } else {
        (404, vec![], None)
    };
    let size = body.len();
    let mut response = Response::builder()
        .status(StatusCode::from_u16(status).unwrap())
        .header("content-length", size);
    if let Some(location) = location {
        response = response.header("location", location);
    }
    response
        .body(if method == "HEAD" {
            Body::empty()
        } else {
            Body::from(body)
        })
        .unwrap()
}
struct Fixture {
    objects: Shared,
    registry: Registry,
    server: tokio::task::JoinHandle<()>,
    root: String,
    layer: String,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}
async fn fixture() -> Fixture {
    let objects = Shared::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/", listener.local_addr().unwrap());
    let app = Router::new().fallback(serve).with_state(objects.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let layer_bytes = b"shared layer contents".to_vec();
    let layer = sha256(&layer_bytes);
    let mut results = vec![];
    {
        let mut objects = objects.lock().unwrap();
        objects
            .blobs
            .insert(("source".into(), layer.clone()), layer_bytes.clone());
        for architecture in ["amd64", "arm64"] {
            let config =
                serde_json::to_vec(&json!({"os":"linux","architecture":architecture})).unwrap();
            let config_id = sha256(&config);
            let bytes=serde_json::to_vec(&json!({"schemaVersion":2,"mediaType":MANIFEST_MEDIA_TYPE,"config":{"mediaType":"application/vnd.oci.image.config.v1+json","digest":config_id,"size":config.len()},"layers":[{"mediaType":"application/vnd.oci.image.layer.v1.tar","digest":layer,"size":layer_bytes.len()}]})).unwrap();
            let id = sha256(&bytes);
            results.push(PlatformResult {
                platform: format!("linux/{architecture}"),
                digest: id.clone(),
                size: bytes.len() as u64,
                media_type: MANIFEST_MEDIA_TYPE.into(),
            });
            objects.blobs.insert(("source".into(), config_id), config);
            objects.manifests.insert(("source".into(), id), bytes);
        }
    }
    let bytes = index(&["linux/amd64".into(), "linux/arm64".into()], &results).unwrap();
    let root = sha256(&bytes);
    objects
        .lock()
        .unwrap()
        .manifests
        .insert(("source".into(), root.clone()), bytes);
    let registry = Registry::new(
        RegistryLogin {
            endpoint,
            username: "test".into(),
            password: "test".into(),
        },
        true,
    )
    .unwrap();
    Fixture {
        objects,
        registry,
        server,
        root,
        layer,
    }
}
#[tokio::test]
async fn copies_complete_graph_before_final_tags_and_checks_real_architecture() {
    let fixture = fixture().await;
    fixture
        .registry
        .verify_platform("source", &fixture.root, "linux/arm64")
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    fixture
        .registry
        .copy(
            &fixture.registry,
            "source",
            "destination",
            &fixture.root,
            &["latest".into()],
            dir.path(),
        )
        .await
        .unwrap();
    let objects = fixture.objects.lock().unwrap();
    assert_eq!(
        objects
            .blobs
            .keys()
            .filter(|(r, _)| r == "destination")
            .count(),
        3
    );
    assert_eq!(objects.writes.len(), 4);
    assert_eq!(objects.writes[2], fixture.root);
    assert_eq!(objects.writes[3], "latest");
    assert_eq!(
        objects.manifests[&("destination".into(), "latest".into())],
        objects.manifests[&("source".into(), fixture.root.clone())]
    );
}
#[tokio::test]
async fn corrupt_layer_never_publishes_a_final_tag() {
    let fixture = fixture().await;
    fixture.objects.lock().unwrap().blobs.insert(
        ("source".into(), fixture.layer.clone()),
        b"corrupted layer bytes".to_vec(),
    );
    let dir = tempfile::tempdir().unwrap();
    assert!(
        fixture
            .registry
            .copy(
                &fixture.registry,
                "source",
                "destination",
                &fixture.root,
                &["latest".into()],
                dir.path()
            )
            .await
            .is_err()
    );
    assert!(
        !fixture
            .objects
            .lock()
            .unwrap()
            .manifests
            .contains_key(&("destination".into(), "latest".into()))
    );
}
#[tokio::test]
async fn forged_architecture_and_descriptor_sizes_are_rejected() {
    let fixture = fixture().await;
    let bad_root = {
        let mut objects = fixture.objects.lock().unwrap();
        let root = objects.manifests[&("source".into(), fixture.root.clone())].clone();
        let mut value: Value = serde_json::from_slice(&root).unwrap();
        value["manifests"][0]["platform"]["architecture"] = json!("arm64");
        value["manifests"][1]["platform"]["architecture"] = json!("amd64");
        let bytes = serde_json::to_vec(&value).unwrap();
        let id = sha256(&bytes);
        objects
            .manifests
            .insert(("source".into(), id.clone()), bytes);
        id
    };
    assert!(
        fixture
            .registry
            .verify_platform("source", &bad_root, "linux/amd64")
            .await
            .is_err()
    );
    let bad_size = {
        let mut objects = fixture.objects.lock().unwrap();
        let mut value: Value =
            serde_json::from_slice(&objects.manifests[&("source".into(), fixture.root.clone())])
                .unwrap();
        value["manifests"][0]["size"] = json!(1);
        let bytes = serde_json::to_vec(&value).unwrap();
        let id = sha256(&bytes);
        objects
            .manifests
            .insert(("source".into(), id.clone()), bytes);
        id
    };
    assert!(
        fixture
            .registry
            .verify_platform("source", &bad_size, "linux/amd64")
            .await
            .is_err()
    );
}
