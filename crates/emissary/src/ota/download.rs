//! [INPUT]: 签名资产、受限HTTP与OTA存储/进度原语。
//! [OUTPUT]: 在固定期限内重试或严格续传，返回已完整校验的原子资产。
//! [POS]: 固定launcher的下载实现；部分文件不授权跨进程恢复。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{
    Asset, Image, Manifest, OtaConfig, Request, binary_name, progress::Writer, storage, verify,
};
use crate::source;
use anyhow::{Context, Result, bail, ensure};
use futures::StreamExt;
use reqwest::{StatusCode, header};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);
const NETWORK_RETRIES: u32 = 3;

pub async fn download(
    cfg: &OtaConfig,
    request: &Request,
    cancel: &CancellationToken,
) -> Result<Image> {
    ensure!(!cancel.is_cancelled(), "release download canceled");
    let (os, arch) = crate::platform();
    let asset = verify(
        request.manifest.as_bytes(),
        &request.signature,
        &cfg.public_keys,
        os,
        arch,
    )?;
    let manifest: Manifest = serde_json::from_str(&request.manifest)?;
    let _guard = storage::lock(&cfg.directory)?;
    let mut progress = Writer::new(&cfg.directory, request, &asset)?;
    let operation = transfer(cfg, request, &asset, &manifest.version, &mut progress);
    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(anyhow::anyhow!("release download canceled")),
        result = tokio::time::timeout(DOWNLOAD_TIMEOUT, operation) => result.unwrap_or_else(|_| Err(anyhow::anyhow!("release download deadline exceeded"))),
    };
    if let Err(error) = &result {
        let code = if cancel.is_cancelled() {
            "canceled"
        } else if error.to_string().contains("insufficient_space") {
            "insufficient_space"
        } else if error.to_string().contains("space_check_unavailable") {
            "space_check_unavailable"
        } else {
            "download_failed"
        };
        let _ = progress.fail(code);
    }
    result
}

async fn transfer(
    cfg: &OtaConfig,
    request: &Request,
    asset: &Asset,
    version: &str,
    progress: &mut Writer,
) -> Result<Image> {
    let versions = cfg.directory.join("versions");
    storage::safe_dir(&versions)?;
    let directory = versions.join(&asset.sha256);
    storage::safe_dir(&directory)?;
    storage::protect_download(&cfg.directory, request, &directory)?;
    let target = directory.join(binary_name());
    storage::collect_locked(
        &cfg.directory,
        std::slice::from_ref(&directory),
        storage::GcPressure::Normal,
    )?;
    progress.phase("space_check")?;
    if target.try_exists()? {
        storage::ensure_regular(&target)?;
        verify_file(&target, asset)?;
        storage::record_use(&directory)?;
        progress.bytes(asset.size)?;
        progress.phase("verified")?;
        return Ok(Image {
            version: version.into(),
            path: target.to_string_lossy().into(),
        });
    }
    check_space(cfg, asset, 0, &directory)?;
    let mut file = tempfile::Builder::new()
        .prefix("download-")
        .suffix(".part")
        .tempfile_in(&directory)?;
    let client = reqwest::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut url = source::validate_url(&request.url, cfg.allow_http)?;
    let mut offset = 0u64;
    let mut etag: Option<String> = None;
    let mut retries = 0;
    let mut restarted = false;
    let mut refreshed = false;
    loop {
        progress.phase("download")?;
        let mut get = client
            .get(url.clone())
            .header(header::ACCEPT_ENCODING, "identity");
        if offset > 0 {
            get = get.header(header::RANGE, format!("bytes={offset}-"));
            if let Some(etag) = &etag {
                get = get.header(header::IF_RANGE, etag);
            }
        }
        let response = match get.send().await {
            Ok(response) => response,
            Err(_) if retries < NETWORK_RETRIES => {
                retry(&mut retries).await;
                continue;
            }
            Err(_) => bail!("release download interrupted"),
        };
        let status = response.status();
        if status == StatusCode::FORBIDDEN {
            ensure!(!refreshed, "release URL refresh exhausted");
            refreshed = true;
            progress.phase("refresh_url")?;
            url = super::url_refresh::refresh(cfg, request, asset).await?;
            continue;
        }
        if status.is_server_error() {
            if retries < NETWORK_RETRIES {
                retry(&mut retries).await;
                continue;
            }
            bail!("release download server unavailable");
        }
        if status == StatusCode::RANGE_NOT_SATISFIABLE {
            if offset == asset.size {
                verify_file(file.path(), asset)?;
                break;
            }
            ensure!(!restarted, "release range rejected after restart");
            restarted = true;
            offset = 0;
            etag = None;
            reset(&mut file)?;
            progress.value.download_attempt += 1;
            progress.bytes(0)?;
            continue;
        }
        ensure!(
            status == StatusCode::OK || status == StatusCode::PARTIAL_CONTENT,
            "release download returned HTTP {}",
            status.as_u16()
        );
        if let Some(encoding) = response.headers().get(header::CONTENT_ENCODING) {
            ensure!(
                encoding.to_str().ok() == Some("identity"),
                "release content encoding rejected"
            );
        }
        ensure!(
            !response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.to_ascii_lowercase().starts_with("multipart/")),
            "multipart release response rejected"
        );
        if status == StatusCode::PARTIAL_CONTENT {
            validate_range(
                response
                    .headers()
                    .get(header::CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok()),
                offset,
                asset.size,
            )?;
        } else if offset > 0 {
            ensure!(!restarted, "release server repeatedly ignored range");
            restarted = true;
            offset = 0;
            etag = None;
            reset(&mut file)?;
            progress.value.download_attempt += 1;
            progress.bytes(0)?;
        }
        let expected = asset
            .size
            .checked_sub(offset)
            .context("release offset exceeds size")?;
        if let Some(length) = response.headers().get(header::CONTENT_LENGTH) {
            ensure!(
                length.to_str()?.parse::<u64>()? == expected,
                "release response length mismatch"
            );
        }
        let response_etag = response
            .headers()
            .get(header::ETAG)
            .and_then(|v| v.to_str().ok())
            .filter(|v| v.starts_with('"') && v.ends_with('"') && v.len() <= 256)
            .map(str::to_owned);
        if status == StatusCode::PARTIAL_CONTENT
            && let (Some(old), Some(new)) = (&etag, &response_etag)
        {
            ensure!(old == new, "release object changed during range transfer");
        }
        if response_etag.is_some() {
            etag = response_etag;
        }
        check_space(cfg, asset, offset, &directory)?;
        let mut body = response.bytes_stream();
        let mut interrupted = false;
        while let Some(chunk) = body.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(_) => {
                    interrupted = true;
                    break;
                }
            };
            let next = offset
                .checked_add(chunk.len() as u64)
                .context("release size overflow")?;
            ensure!(next <= asset.size, "binary size or SHA-256 mismatch");
            file.write_all(&chunk).context("release write failed")?;
            offset = next;
            progress.bytes(offset)?;
        }
        if interrupted || offset < asset.size {
            if retries < NETWORK_RETRIES {
                retry(&mut retries).await;
                continue;
            }
            bail!("release download interrupted");
        }
        ensure!(offset == asset.size, "binary size or SHA-256 mismatch");
        break;
    }
    progress.phase("verify")?;
    verify_file(file.path(), asset)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o700))?;
    }
    file.as_file().sync_all()?;
    file.persist_noclobber(&target).map_err(|e| e.error)?;
    storage::record_use(&directory)?;
    storage::sync(&directory)?;
    progress.bytes(asset.size)?;
    progress.phase("verified")?;
    Ok(Image {
        version: version.into(),
        path: target.to_string_lossy().into(),
    })
}

fn check_space(cfg: &OtaConfig, asset: &Asset, offset: u64, active: &Path) -> Result<()> {
    let needed = storage::required_space(asset.size, offset)?;
    let mut free = fs2::available_space(active).context("space_check_unavailable")?;
    if free < needed {
        storage::collect_locked(
            &cfg.directory,
            &[active.into()],
            storage::GcPressure::LowSpace,
        )?;
        free = fs2::available_space(active).context("space_check_unavailable")?;
    }
    ensure!(
        free >= needed,
        "insufficient_space: needed={needed} available={free}"
    );
    Ok(())
}
fn reset(file: &mut tempfile::NamedTempFile) -> Result<()> {
    file.as_file_mut().set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    Ok(())
}
async fn retry(retries: &mut u32) {
    *retries += 1;
    tokio::time::sleep(Duration::from_millis(100u64 << (*retries - 1))).await;
}
fn validate_range(value: Option<&str>, offset: u64, total: u64) -> Result<()> {
    let expected = format!(
        "bytes {offset}-{}/{total}",
        total.checked_sub(1).context("empty release asset")?
    );
    ensure!(
        value == Some(expected.as_str()),
        "release Content-Range mismatch"
    );
    Ok(())
}
fn verify_file(path: &Path, asset: &Asset) -> Result<()> {
    storage::ensure_regular(path)?;
    let mut file = fs::File::open(path)?;
    ensure!(
        file.metadata()?.len() == asset.size,
        "binary size or SHA-256 mismatch"
    );
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    ensure!(
        hex::encode(hash.finalize()) == asset.sha256,
        "binary size or SHA-256 mismatch"
    );
    Ok(())
}
