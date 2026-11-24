//! [INPUT]: 依赖真实 HTTP 票据与浏览器 WebSocket。
//! [OUTPUT]: 提供同渲染器重附着和可恢复关闭码断言。
//! [POS]: 三副本终端测试的浏览器恢复夹具。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

pub(super) async fn reconnect(
    client: &reqwest::Client,
    address: std::net::SocketAddr,
    id: Uuid,
    cookie: &str,
    tenant: Uuid,
    renderer: Uuid,
    offset: usize,
) -> WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>> {
    let attach: Value = client
        .post(format!(
            "http://{address}/api/v1/terminal-sessions/{id}/attach"
        ))
        .header("cookie", cookie)
        .header("x-artifactory-tenant", tenant.to_string())
        .header("x-artifactory-csrf", "1")
        .json(&json!({"renderer_id":renderer,"offset":offset}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut request = format!("ws://{address}/api/v1/terminal-sessions/{id}/stream")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    request
        .headers_mut()
        .insert("origin", "http://localhost:3000".parse().unwrap());
    let (mut browser, _) = connect_async(request).await.unwrap();
    browser.send(Message::Text(json!({"type":"authenticate","ticket":attach["ticket"],"renderer_id":renderer,"offset":offset}).to_string().into())).await.unwrap();
    browser
}

pub(super) async fn close_code(
    browser: &mut WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
) -> u16 {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match browser.next().await.unwrap().unwrap() {
                Message::Close(Some(close)) => break u16::from(close.code),
                Message::Text(text) => {
                    let value: Value = serde_json::from_str(&text).unwrap();
                    assert_ne!(
                        value["type"], "exit",
                        "a broken browser relay must preserve the shell"
                    );
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap()
}
