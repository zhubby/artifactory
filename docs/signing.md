<!--
[INPUT]: 显式应用平台、固定签名绑定与原生验签证据。
[OUTPUT]: 定义托管材料、默认方案和验签边界。
[POS]: macOS 与 Android 托管签名的配置入口。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 托管应用签名

Artifactory 支持加密托管 macOS 和 Android 签名材料。
构建和签名在同一节点执行。
租户配置凭据和方案，项目配置默认方案。
构建创建时固定凭据版本，后续轮换不改变已有构建。

## 配置顺序

1. 为控制平面配置独立签名密钥环。
2. 打开租户设置的「应用签名」。
3. 导入含私钥的证书文件和密码。
4. 创建方案，例如 `managed:desktop-release`。
5. 在节点设置的签名授权中加入该方案名称。
6. 在项目设置的「应用签名」中选择默认方案。
7. 创建构建。向导显示当前身份和版本预览。

项目默认方案自动加入项目签名授权。
节点仍需单独授权，且必须满足项目节点范围。
项目成员能读取默认方案和公开证书信息。
凭据导入、停用和默认方案变更要求租户管理员。
平台没有私钥导出接口。

## 支持范围

| 应用 | 导入材料 | 最终制品 | 节点独立校验 |
|---|---|---|---|
| macOS | Developer ID Application PKCS12 | ZIP、DMG | 包内 `.app` 的签名、证书链、指纹、Team ID |
| Android APK | 应用签名密钥的 JKS、PKCS12 | APK | `apksigner verify` 和唯一签名者指纹 |
| Android AAB | 上传密钥的 JKS、PKCS12 | AAB | `jarsigner -verify -strict`、`keytool` 指纹和唯一签名者 |

文件上限为 2 MiB。导入会校验密码、别名、证书有效期和公私钥匹配。
PKCS12 的 store password 与 key password 必须相同。
APK 应用签名密钥和 AAB 上传密钥是不同用途。
Google Play 后续使用应用签名密钥签署分发 APK。

同一 Android 凭据的新版本须保留指纹和用途。
更换应用身份时，新建凭据并显式切换项目默认方案。
平台不生成替代密钥，也不自动完成 Android 密钥升级。

macOS 要求原生 macOS 节点。
Android 支持可运行相应工具的原生节点。
节点需升级至支持 `managed-signing-v1` 的 Emissary。
计划校验会补齐平台所需工具的能力约束。
原有节点本机签名方案继续可用。
`managed:` 名称保留给托管方案，不回退到本机材料。

## 独立签名密钥环

密钥环与源码凭据的 `ARTIFACTORY_MASTER_KEY` 分开保存。
以下命令创建文件，终端不输出密钥：

```bash
python3 - <<'PY'
import base64, json, os, secrets
path = 'signing-keys.json'
fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
with os.fdopen(fd, 'w') as output:
    json.dump({'active': 'v1', 'keys': {
        'v1': base64.b64encode(secrets.token_bytes(32)).decode()
    }}, output)
PY
```

设置 `ARTIFACTORY_SIGNING_KEYS_FILE` 为文件绝对路径。
Unix 文件禁止组和其他用户读取。
启动时加载密钥环；变更后滚动重启控制平面。
每个凭据版本生成随机数据密钥。
AES-GCM 将租户、凭据、版本和密钥 ID 绑定到密文。
数据库只保存材料密文和包装后的数据密钥。

轮换时添加新的密钥 ID，并将 `active` 指向新 ID。
保留所有仍被历史密文引用的旧密钥。
轮换只影响新导入版本，不自动重加密旧版本。
所有控制平面副本使用相同密钥环。
备份数据库及密钥环，并将两份备份分开控制访问。
丢失密钥环会使对应历史版本无法解密。

Compose 使用 `.deploy/signing/keys.json`，文件归运行 UID 所有。
设置 `ARTIFACTORY_SIGNING_KEYS_FILE=/run/artifactory-signing/keys.json`。
该目录只挂载到控制平面。
不配置变量时，普通构建继续工作，托管导入显示未配置。

Helm 使用预先创建的 Secret：

```bash
kubectl create secret generic artifactory-signing --from-file=keys.json=signing-keys.json
helm upgrade --install artifactory charts/artifactory \
  --set controlPlane.signingKeysSecret=artifactory-signing
```

Chart 将 Secret 复制到内存卷，并设置文件模式为 `0600`。
材料不进入 Web、规划 Worker、规划协调器或源码准备进程。
Secret 内容更新后需滚动重启控制平面。

## 执行和清理

领取同时校验节点 mTLS、当前 session、attempt、租约和 sign 步骤。
平台复核租户、项目、节点和固定版本的当前授权。
凭据、版本或方案被停用后，新的领取和证据登记被拒绝。
已发出的材料依赖任务取消及租约失效回收。
停用不会从运行中的进程内存中远程擦除私钥。

节点将材料写入任务私有目录，置于源码和制品收集范围外。
Android 使用私有 keystore 文件。
macOS 使用随机密码的临时 Keychain，并显式指定 Keychain 路径。
节点不调用 `list-keychains -s` 或 `default-keychain` 修改共享配置。
相邻 sign 步骤共享本任务材料。
进入普通 verify 步骤前删除材料。
步骤失败和取消也触发清理。
启动清扫使用每任务文件锁，跳过仍在使用的目录。
清理失败写入闭锁标记，节点停止接单。
修复目录访问或原生工具故障后，重启节点完成清扫。

可信节点和构建脚本是安全边界。
临时文件和环境变量不能隔离同账户下的恶意脚本。
只将生产签名授权授予受信任节点和项目。
日志在写入本机持久化缓存前脱敏，并覆盖跨分块的明文秘密。
脱敏不能阻止恶意脚本编码或通过网络泄露材料。

## 计划与环境变量

`application_platform` 表示应用平台，取值为
`macos`、`ios`、`android`、`linux`、`windows`。
`os` 和 `arch` 仍表示执行宿主。
CLI、库、服务端和 OCI 目标使用 `null`。
AI 计划必须显式填写，不能省略。
默认方案仅匹配显式应用平台。
Darwin 宿主同时支持 macOS、iOS 和非应用目标。
iOS 分类不提供 iOS 托管签名。
计划与脚本属于可信项目输入，应用分类不是制品类型鉴定。
平台不通过宿主或扩展名识别伪报类别。
匹配默认方案时，计划必须使用固定方案并包含 sign、verify 阶段。
`signing_outputs` 为最终制品的显式相对路径，不接受 glob。
每个托管制品都必须声明验签输出。
macOS 的每项还需声明包内 `.app` 的 `bundle` 路径。

```yaml
application_platform: android
signing_profiles: [managed:android-release]
signing_outputs:
  - path: dist/app.apk
artifacts: [dist/app.apk]
```

仅 sign 步骤注入下列环境变量：

| 平台 | 变量 |
|---|---|
| Android | `ARTIFACTORY_ANDROID_KEYSTORE` |
| Android | `ARTIFACTORY_ANDROID_STORE_PASSWORD` |
| Android | `ARTIFACTORY_ANDROID_KEY_PASSWORD` |
| Android | `ARTIFACTORY_ANDROID_KEY_ALIAS` |
| macOS | `ARTIFACTORY_APPLE_KEYCHAIN` |
| macOS | `ARTIFACTORY_APPLE_IDENTITY` |
| macOS 公证 | `ARTIFACTORY_APPLE_NOTARY_KEY` |
| macOS 公证 | `ARTIFACTORY_APPLE_NOTARY_KEY_ID` |
| macOS 公证 | `ARTIFACTORY_APPLE_NOTARY_ISSUER` |

APK 使用 `apksigner --ks-pass env:变量名 --key-pass env:变量名`。
AAB 使用 `jarsigner -storepass:env 变量名 -keypass:env 变量名`。
先对齐 APK，再签名；签名后不改变包内容。
macOS 使用 `codesign --keychain "$ARTIFACTORY_APPLE_KEYCHAIN"`，
并启用 hardened runtime 和安全时间戳。
后台服务账号不要求已有默认 Keychain。
提取证书链使用 `codesign --display --extract-certificates=前缀`。
该选项的前缀必须通过等号连接。
证书链信任使用 `security verify-cert -p codeSign`。

方案要求公证时，sign 脚本须使用 `xcrun notarytool submit --wait`，
确认 Accepted，再对 `.app` 装订，最后生成最终 ZIP 或 DMG。
DMG 同时须装订镜像。
节点独立检查最终包内 `.app` 的装订票据。
DMG 还须通过镜像自身的装订票据校验。
脚本不应回显密码、私钥或完整环境变量。

## 制品证据与接口

所有步骤成功后，节点复制最终制品，验证副本并计算 SHA-256。
上传使用同一副本。
平台要求登记的摘要、路径和固定证书版本完全匹配。
缺少任一声明制品的证据时，任务不能成功。
控制台显示指纹、方案和独立的公证状态。
证据表示受信任节点完成原生校验，不是平台独立重跑签名工具。

| 接口 | 权限和用途 |
|---|---|
| `GET/POST /api/v1/signing/credentials` | 租户管理员，列出公开元数据或导入 |
| `POST /api/v1/signing/credentials/{id}/versions` | 租户管理员，新增不可变版本 |
| `GET/POST /api/v1/signing/profiles` | 租户管理员，读取或创建方案 |
| `PATCH /api/v1/signing/{credentials,versions,profiles}/{id}` | 租户管理员，启用或停用 |
| `GET/PUT /api/v1/projects/{id}/signing-defaults` | 项目读取；变更另需租户管理员 |
| `GET /api/v1/projects/{id}/signing-preview` | 项目读取，当前默认版本预览 |

管理变更、材料领取和验签证据写入租户审计。
租户删除流程同时清除托管凭据密文。
项目删除清除其任务证据，不删除租户共享凭据。

生产 Developer ID 公证和真实 Android 覆盖升级，
分别需要正式账户材料与对应设备验收。

真实证书链路见 [AI-Workspace macOS 托管签名验收](ai-workspace-signing-acceptance-2026-11-17.md)。
该记录覆盖正式签名、S3 下载验签、材料清理和未公证状态。

[PROTOCOL]: Update this header on change, then check AGENTS.md
