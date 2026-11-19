"""[INPUT]: 认证后的仓库读函数、固定 digest 和请求平台。
[OUTPUT]: 验证 OCI 图、Release 元数据、ELF 架构与真实 RUN。
[POS]: 内建 Gateway 和外部仓库共用的只读 OCI 图校验。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import hashlib
import io
import json
import tarfile

def registry_graph(read, repository, digest, platforms, *, release_version=None, commit_sha=None):
    def download(path, expected):
        status, _, contents = read('/v2/' + repository + path)
        assert status == 200, f'Registry download HTTP {status}'
        assert 'sha256:' + hashlib.sha256(contents).hexdigest() == expected
        return contents
    index = json.loads(download('/manifests/' + digest, digest))
    assert index['mediaType'] == 'application/vnd.oci.image.index.v1+json'
    assert len(index['manifests']) == len(platforms)
    assert sorted('linux/' + item['platform']['architecture'] for item in index['manifests']) == sorted(platforms)
    verified = []
    for child in index['manifests']:
        arch = child['platform']['architecture']
        assert child['platform'] == {'architecture': arch, 'os': 'linux'}
        manifest_bytes = download('/manifests/' + child['digest'], child['digest'])
        assert len(manifest_bytes) == child['size']
        manifest = json.loads(manifest_bytes)
        config_bytes = download('/blobs/' + manifest['config']['digest'], manifest['config']['digest'])
        assert len(config_bytes) == manifest['config']['size']
        config = json.loads(config_bytes)
        assert config['architecture'] == arch and config['os'] == 'linux'
        labels = config.get('config', {}).get('Labels', {})
        if release_version is not None:
            assert labels.get('org.opencontainers.image.version') == release_version, 'OCI version differs from Release'
        if commit_sha is not None:
            assert labels.get('org.opencontainers.image.revision') == commit_sha, 'OCI revision differs from pinned source'
        blobs, files = [], {}
        for layer in manifest['layers']:
            path = '/blobs/' + layer['digest']
            contents = download(path, layer['digest'])
            assert len(contents) == layer['size']
            status, headers, _ = read('/v2/' + repository + path, method='HEAD')
            assert status == 200 and int(headers['Content-Length']) == len(contents)
            status, _, part = read('/v2/' + repository + path, headers={'Range': 'bytes=0-15'})
            assert status == 206 and part == contents[:16]
            blobs.append({'digest': layer['digest'], 'size': len(contents), 'head': True, 'range': True})
            with tarfile.open(fileobj=io.BytesIO(contents), mode='r:*') as archive:
                for entry in archive.getmembers():
                    name = entry.name.removeprefix('./').removeprefix('/')
                    if name in ['arch-check', 'arch-result', 'hello.txt'] and entry.isfile():
                        files[name] = archive.extractfile(entry).read()
        assert files['arch-result'] == ('executed-linux-' + arch + '\n').encode(), 'Actual RUN architecture marker missing'
        executable = files['arch-check']
        assert executable[:4] == b'\x7fELF' and executable[5] == 1
        assert int.from_bytes(executable[18:20], 'little') == {'arm64': 183, 'amd64': 62}[arch]
        assert files['hello.txt'] == b'image-source-init-ok\n'
        verified.append({'platform': 'linux/' + arch, 'manifest': child['digest'], 'config': manifest['config']['digest'], 'run_marker': files['arch-result'].decode().strip(), 'executable_sha256': hashlib.sha256(executable).hexdigest(), 'blobs': blobs})
    return {'repository': repository, 'digest': digest, 'platforms': platforms, 'verified': verified}
