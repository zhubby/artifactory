/**
 * [INPUT]: 依赖产品统一版本安装材料与浏览器 UTF-8 编码。
 * [OUTPUT]: 提供安装类型、Shell 和 PowerShell 安装及注册命令。
 * [POS]: 弹窗的纯命令渲染边界；不请求或持久化凭证。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
export type InstallationOS = "linux" | "darwin" | "windows";
export type InstallationArch = "amd64" | "arm64";
export interface NodeInstallationOptions {
  product_version: string;
  device_endpoint: string;
  releases: { id: string; version: string; platforms: string[] }[];
}
export interface NodeInstallationSettings {
  device_endpoint: string;
  can_edit: boolean;
}
export interface NodeInstallationMaterial {
  token: string;
  expires_at: string;
  device_endpoint: string;
  ca_pem: string;
  ca_sha256: string;
  version: string;
  os: InstallationOS;
  arch: InstallationArch;
  download_url: string;
  sha256: string;
  size: number;
  public_keys: string[];
  installer_url: string;
  installer_sha256: string;
}

const shell = (value: string) => `'${value.replaceAll("'", "'\"'\"'")}'`;
const powershell = (value: string) => `'${value.replaceAll("'", "''")}'`;
function base64(value: string) {
  return btoa(Array.from(new TextEncoder().encode(value), byte => String.fromCharCode(byte)).join(""));
}

export function installationCommand(material: NodeInstallationMaterial): string {
  const values: [string, string][] = [
    ["download-url", material.download_url], ["sha256", material.sha256], ["size", String(material.size)],
    ["endpoint", material.device_endpoint], ["ca-base64", base64(material.ca_pem)],
    ["ca-sha256", material.ca_sha256], ["public-keys", material.public_keys.join(",")],
    ["os", material.os], ["arch", material.arch],
  ];
  if (material.os === "windows") {
    const parameters = values.map(([name, value]) => `    -${name.split("-").map(part => part[0].toUpperCase() + part.slice(1)).join("")} ${powershell(value)}`).join(" `\n");
    return `& {
  $ErrorActionPreference = 'Stop'
  $installer = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName() + '.ps1')
  $previousPolicy = Get-ExecutionPolicy -Scope Process
  $policyChanged = $false
  try {
    $uri = [Uri]${powershell(material.installer_url)}
    if (!$uri.IsAbsoluteUri -or $uri.Scheme -ne 'https' -or !$uri.Host -or $uri.UserInfo -or $uri.Fragment) { throw 'Installer URL must use HTTPS' }
    Invoke-WebRequest -UseBasicParsing -Uri $uri -MaximumRedirection 0 -TimeoutSec 120 -OutFile $installer
    if ((Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant() -ne ${powershell(material.installer_sha256)}) { throw 'Installer checksum mismatch' }
    Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass -Force
    $policyChanged = $true
    $env:ARTIFACTORY_ENROLLMENT_TOKEN = ${powershell(material.token)}
    & $installer \`
${parameters}
  } finally {
    Remove-Item Env:\\ARTIFACTORY_ENROLLMENT_TOKEN -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $installer -Force -ErrorAction SilentlyContinue
    if ($policyChanged) { Set-ExecutionPolicy -Scope Process -ExecutionPolicy $previousPolicy -Force }
  }
}`;
  }
  const parameters = values.map(([name, value]) => `    --${name} ${shell(value)}`).join(" \\\n");
  return `(
  set -eu
  umask 077
  installer=$(mktemp)
  trap 'unset ARTIFACTORY_ENROLLMENT_TOKEN; rm -f "$installer"' EXIT
  status=$(curl --fail --silent --show-error --proto '=https' --tlsv1.2 --connect-timeout 20 --max-time 120 --max-filesize 262144 ${shell(material.installer_url)} --output "$installer" --write-out '%{http_code}')
  [ "$status" = 200 ] || { echo 'Installer download must return HTTP 200 without redirects' >&2; exit 1; }
  if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$installer" | cut -d ' ' -f 1)
  else
    actual=$(shasum -a 256 "$installer" | cut -d ' ' -f 1)
  fi
  [ "$actual" = ${shell(material.installer_sha256)} ] || { echo 'Installer checksum mismatch' >&2; exit 1; }
  export ARTIFACTORY_ENROLLMENT_TOKEN=${shell(material.token)}
  sh "$installer" \\
${parameters}
)`;
}

export function registrationCommand(material: NodeInstallationMaterial): string {
  const ca = base64(material.ca_pem);
  if (material.os === "windows") {
    return `& {
  $ErrorActionPreference = 'Stop'
  $base = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Artifactory'
  $directory = Join-Path $base ('registration-' + [Guid]::NewGuid().ToString('N'))
  try {
    $null = New-Item -ItemType Directory -Path $directory
    $acl = New-Object Security.AccessControl.DirectorySecurity
    $acl.SetAccessRuleProtection($true, $false)
    $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User
    $rule = New-Object Security.AccessControl.FileSystemAccessRule($sid, 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow')
    $acl.AddAccessRule($rule)
    Set-Acl -LiteralPath $directory -AclObject $acl
    $ca = Join-Path $directory 'ca.pem'
    [IO.File]::WriteAllBytes($ca, [Convert]::FromBase64String(${powershell(ca)}))
    $env:ARTIFACTORY_ENROLLMENT_TOKEN = ${powershell(material.token)}
    & artifactory-emissary.exe register --endpoint ${powershell(material.device_endpoint)} --ca $ca
    if ($LASTEXITCODE -ne 0) { throw 'Node registration failed; preserve the CA directory and inspect existing identity files before retrying' }
  } finally {
    Remove-Item Env:\\ARTIFACTORY_ENROLLMENT_TOKEN -ErrorAction SilentlyContinue
  }
}`;
  }
  const base = material.os === "darwin" ? '"$HOME/Library/Application Support/artifactory"' : '"\${XDG_CONFIG_HOME:-$HOME/.config}/artifactory"';
  return `(
  set -eu
  umask 077
  base=${base}
  mkdir -p "$base"
  directory=$(mktemp -d "$base/registration.XXXXXXXX")
  trap 'unset ARTIFACTORY_ENROLLMENT_TOKEN' EXIT
  ca="$directory/ca.pem"
  printf '%s' ${shell(ca)} | ${material.os === "darwin" ? "base64 -D" : "base64 -d"} > "$ca"
  export ARTIFACTORY_ENROLLMENT_TOKEN=${shell(material.token)}
  artifactory-emissary register --endpoint ${shell(material.device_endpoint)} --ca "$ca"
)`;
}
