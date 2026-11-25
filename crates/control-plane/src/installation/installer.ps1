# [INPUT]: 固定资产地址、摘要、设备地址、CA、公钥与环境注册凭证。
# [OUTPUT]: 以本地专用账户注册节点、初始化 OTA 并启动 Windows 服务。
# [POS]: 控制平面分发的 Windows 首次安装入口；不覆盖已有身份。
# [PROTOCOL]: Update this header on change, then check AGENTS.md
param(
    [Parameter(Mandatory = $true)][string]$DownloadUrl,
    [Parameter(Mandatory = $true)][string]$Sha256,
    [Parameter(Mandatory = $true)][long]$Size,
    [Parameter(Mandatory = $true)][string]$Endpoint,
    [Parameter(Mandatory = $true)][string]$CaBase64,
    [Parameter(Mandatory = $true)][string]$CaSha256,
    [Parameter(Mandatory = $true)][string]$PublicKeys,
    [Parameter(Mandatory = $true)][ValidateSet('windows')][string]$Os,
    [Parameter(Mandatory = $true)][ValidateSet('amd64', 'arm64')][string]$Arch,
    [string]$User,
    [string]$Directory,
    [string]$BinaryDirectory
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$enrollmentToken = $env:ARTIFACTORY_ENROLLMENT_TOKEN
Remove-Item Env:\ARTIFACTORY_ENROLLMENT_TOKEN -ErrorAction SilentlyContinue
$temporary = $null
$credential = $null

# ---- 身份切换与目录权限共用边界 ----
function Set-PrivateDirectory([string]$Path, [Security.Principal.SecurityIdentifier]$ServiceSid, [Security.AccessControl.FileSystemRights]$ServiceRights) {
    $acl = [Security.AccessControl.DirectorySecurity]::new()
    $acl.SetOwner([Security.Principal.WindowsIdentity]::GetCurrent().User)
    $acl.SetAccessRuleProtection($true, $false)
    $inherit = [Security.AccessControl.InheritanceFlags]'ContainerInherit,ObjectInherit'
    foreach ($sid in @('S-1-5-18', 'S-1-5-32-544')) {
        $rule = [Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new($sid), [Security.AccessControl.FileSystemRights]::FullControl, $inherit, [Security.AccessControl.PropagationFlags]::None, [Security.AccessControl.AccessControlType]::Allow)
        $acl.AddAccessRule($rule)
    }
    if ($ServiceSid) {
        $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($ServiceSid, $ServiceRights, $inherit, [Security.AccessControl.PropagationFlags]::None, [Security.AccessControl.AccessControlType]::Allow))
    }
    Set-Acl -LiteralPath $Path -AclObject $acl
}
function Assert-NewPath([string]$Path) {
    if (![IO.Path]::IsPathRooted($Path) -or $Path -match '[\x00-\x1f]' -or $Path.StartsWith('\\')) { throw 'Installation paths must be absolute local paths.' }
    $full = [IO.Path]::GetFullPath($Path)
    if ($full.TrimEnd('\') -eq [IO.Path]::GetPathRoot($full).TrimEnd('\')) { throw 'Installation cannot use a drive root.' }
    if (Test-Path -LiteralPath $full) { throw "Installation path already exists; existing identities are preserved: $full" }
    $parent = [IO.DirectoryInfo]::new($full).Parent
    while ($parent) {
        if ($parent.Exists -and ($parent.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw "Installation parent is a reparse point: $($parent.FullName)" }
        $parent = $parent.Parent
    }
}
function Quote-NativeArgument([string]$Value) {
    if ($Value -match '[\x00\r\n]') { throw 'Invalid native argument.' }
    $escaped = [regex]::Replace($Value, '(\\*)"', '$1$1\"')
    $escaped = [regex]::Replace($escaped, '(\\+)$', '$1$1')
    return '"' + $escaped + '"'
}
function Invoke-ServiceAccount([string[]]$Arguments, [bool]$Enroll = $false) {
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $binary
    $info.Arguments = (($Arguments | ForEach-Object { Quote-NativeArgument $_ }) -join ' ')
    $info.WorkingDirectory = $Directory
    $info.UseShellExecute = $false
    $info.LoadUserProfile = $true
    $info.Domain = $env:COMPUTERNAME
    $info.UserName = $User
    $info.Password = $credential.Password
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.EnvironmentVariables.Remove('ARTIFACTORY_ENROLLMENT_TOKEN')
    $info.EnvironmentVariables.Remove('ARTIFACTORY_SERVICE_PASSWORD')
    if ($Enroll) { $info.EnvironmentVariables['ARTIFACTORY_ENROLLMENT_TOKEN'] = $enrollmentToken }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $info
    try {
        if (!$process.Start()) { throw 'Could not start Emissary as the dedicated service account.' }
        $outputTask = $process.StandardOutput.ReadToEndAsync()
        $errorTask = $process.StandardError.ReadToEndAsync()
        if (!$process.WaitForExit(180000)) {
            $process.Kill()
            $process.WaitForExit()
            throw 'Emissary setup timed out under the service account.'
        }
        $output = $outputTask.GetAwaiter().GetResult()
        $errorText = $errorTask.GetAwaiter().GetResult()
        if ($process.ExitCode -ne 0) {
            # 注册失败只保留错误类别，避免下游错误输出意外回显凭证。
            if ($Enroll) { throw "Device enrollment failed (exit $($process.ExitCode)). Existing identity files were preserved; do not register over them." }
            throw "Emissary setup failed (exit $($process.ExitCode)). Verify the architecture-matched Microsoft Visual C++ v14 runtime and the service account's SDK access. $errorText"
        }
        if (!$Enroll -and $output) { Write-Host $output.TrimEnd() }
    } finally {
        $info.EnvironmentVariables.Remove('ARTIFACTORY_ENROLLMENT_TOKEN')
        $process.Dispose()
    }
}
function Invoke-Administrator([string[]]$Arguments) {
    & $binary @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Native service operation failed (exit $LASTEXITCODE)." }
}

try {
    if (!$enrollmentToken -or $enrollmentToken.Length -gt 4096) { throw 'ARTIFACTORY_ENROLLMENT_TOKEN is required; generate a fresh installation command.' }
    if ($env:OS -ne 'Windows_NT') { throw 'The Windows installer requires a Windows host.' }
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    if (!([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Run the command from an Administrator PowerShell window.' }
    if ($Sha256 -cnotmatch '^[0-9a-f]{64}$' -or $CaSha256 -cnotmatch '^[0-9a-f]{64}$' -or $Size -lt 1 -or $Size -gt 134217728) { throw 'Invalid binary size or SHA-256 digest.' }
    $uri = [Uri]$DownloadUrl
    if (!$uri.IsAbsoluteUri -or $uri.Scheme -ne 'https' -or !$uri.Host -or $uri.UserInfo -or $uri.Fragment) { throw 'The binary download URL must use HTTPS without embedded credentials.' }
    if ($Endpoint -notmatch '^(?:[A-Za-z0-9.-]+|\[[0-9A-Fa-f:.]+\]):([1-9][0-9]{0,4})$' -or [int]$Matches[1] -gt 65535) { throw 'The device endpoint must contain only a host and TCP port.' }
    $ca = [Convert]::FromBase64String($CaBase64)
    if ($ca.Length -gt 65536 -or $ca.Length -eq 0) { throw 'Invalid CA size.' }
    $hasher = [Security.Cryptography.SHA256]::Create()
    try { $actualCaHash = [BitConverter]::ToString($hasher.ComputeHash($ca)).Replace('-', '').ToLowerInvariant() } finally { $hasher.Dispose() }
    if ($actualCaHash -ne $CaSha256) { throw 'CA SHA-256 mismatch.' }
    $pem = [Text.Encoding]::ASCII.GetString($ca)
    $certificateMatch = [regex]::Match($pem, '-----BEGIN CERTIFICATE-----\s*([A-Za-z0-9+/=\s]+)-----END CERTIFICATE-----')
    if (!$certificateMatch.Success) { throw 'The supplied CA is not a PEM certificate.' }
    $certificate = [Security.Cryptography.X509Certificates.X509Certificate2]::new([Convert]::FromBase64String($certificateMatch.Groups[1].Value))
    $certificate.Dispose()
    $keys = $PublicKeys.Split(',')
    if ($keys.Count -eq 0) { throw 'Release verification keys are required.' }
    foreach ($key in $keys) { if ($key.Length -ne 44 -or [Convert]::FromBase64String($key).Length -ne 32) { throw 'Invalid release verification key.' } }

    # ---- 系统 API 返回原生架构，避免 WOW64 或 ARM64 模拟层误判 ----
    if (-not ('ArtifactInstallationNative' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Security.Principal;
public static class ArtifactInstallationNative {
    [StructLayout(LayoutKind.Sequential)] struct SystemInfo {
        public ushort Architecture, Reserved;
        public uint PageSize;
        public IntPtr MinimumAddress, MaximumAddress, ActiveProcessorMask;
        public uint NumberOfProcessors, ProcessorType, AllocationGranularity;
        public ushort ProcessorLevel, ProcessorRevision;
    }
    [DllImport("kernel32.dll")] static extern void GetNativeSystemInfo(out SystemInfo info);
    public static ushort Architecture() { SystemInfo info; GetNativeSystemInfo(out info); return info.Architecture; }
    [StructLayout(LayoutKind.Sequential)] struct Attributes {
        public uint Length;
        public IntPtr RootDirectory, ObjectName;
        public uint Flags;
        public IntPtr SecurityDescriptor, SecurityQualityOfService;
    }
    [StructLayout(LayoutKind.Sequential)] struct UnicodeString {
        public ushort Length, MaximumLength;
        public IntPtr Buffer;
    }
    [DllImport("advapi32.dll")] static extern uint LsaOpenPolicy(IntPtr system, ref Attributes attributes, uint access, out IntPtr policy);
    [DllImport("advapi32.dll")] static extern uint LsaAddAccountRights(IntPtr policy, byte[] sid, UnicodeString[] rights, uint count);
    [DllImport("advapi32.dll")] static extern uint LsaNtStatusToWinError(uint status);
    [DllImport("advapi32.dll")] static extern uint LsaClose(IntPtr policy);
    static void Check(uint status) { if (status != 0) throw new Win32Exception((int)LsaNtStatusToWinError(status)); }
    public static void GrantServiceLogon(string value) {
        var attributes = new Attributes();
        attributes.Length = (uint)Marshal.SizeOf(typeof(Attributes));
        IntPtr policy;
        Check(LsaOpenPolicy(IntPtr.Zero, ref attributes, 0x810, out policy));
        var right = "SeServiceLogonRight";
        var buffer = Marshal.StringToHGlobalUni(right);
        try {
            var sid = new SecurityIdentifier(value);
            var bytes = new byte[sid.BinaryLength]; sid.GetBinaryForm(bytes, 0);
            var text = new UnicodeString { Length = (ushort)(right.Length * 2), MaximumLength = (ushort)((right.Length + 1) * 2), Buffer = buffer };
            Check(LsaAddAccountRights(policy, bytes, new[] { text }, 1));
        } finally { Marshal.FreeHGlobal(buffer); LsaClose(policy); }
    }
}
'@
    }
    $nativeArch = switch ([ArtifactInstallationNative]::Architecture()) { 9 { 'amd64' }; 12 { 'arm64' }; default { throw 'Unsupported Windows architecture.' } }
    if ($Arch -ne $nativeArch) { throw "Selected platform windows/$Arch does not match windows/$nativeArch." }
    if (!$User) { $User = Read-Host 'Existing dedicated local non-administrator service account' }
    if ($User -notmatch '^[A-Za-z0-9_.-]+$' -or $User.StartsWith('-')) { throw 'Provide a local service account name without a domain prefix.' }
    $account = Get-LocalUser -Name $User
    if (!$account.Enabled -or $account.SID.Value.EndsWith('-500') -or $account.SID.Value -eq $identity.User.Value) { throw 'Use an enabled dedicated non-administrator account.' }
    $administrators = @(Get-LocalGroupMember -SID 'S-1-5-32-544')
    if ($administrators | Where-Object { $_.SID.Value -eq $account.SID.Value }) { throw 'The service account must not belong to Administrators.' }
    if (!$Directory) { $Directory = Join-Path $env:ProgramData 'Artifactory\Emissary\state' }
    if (!$BinaryDirectory) { $BinaryDirectory = Join-Path $env:ProgramFiles 'Artifactory\Emissary\bin' }
    $Directory = [IO.Path]::GetFullPath($Directory)
    $BinaryDirectory = [IO.Path]::GetFullPath($BinaryDirectory)
    Assert-NewPath $Directory
    Assert-NewPath $BinaryDirectory
    if (Get-Service -Name 'artifactory-emissary' -ErrorAction SilentlyContinue) { throw 'The Emissary service already exists.' }
    $credential = Get-Credential -UserName "$env:COMPUTERNAME\$User" -Message 'Password stays on this machine and is used to register and run Emissary as this account.'
    if (!$credential -or $credential.UserName -ine "$env:COMPUTERNAME\$User") { throw 'The credential must match the selected local service account.' }

    # ---- 下载严格限制大小，禁止重定向，校验后才执行 ----
    $temporary = Join-Path ([IO.Path]::GetTempPath()) ('artifactory-install-' + [Guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($temporary) | Out-Null
    Set-PrivateDirectory $temporary $null ([Security.AccessControl.FileSystemRights]::None)
    $download = Join-Path $temporary 'artifactory-emissary.exe'
    $request = [Net.HttpWebRequest]::Create($uri)
    $request.AllowAutoRedirect = $false
    $request.Timeout = 300000
    $request.ReadWriteTimeout = 300000
    $response = $request.GetResponse()
    try {
        if ([int]$response.StatusCode -ne 200 -or ($response.ContentLength -ge 0 -and $response.ContentLength -ne $Size)) { throw 'Binary download returned an unexpected HTTP status or size.' }
        $stream = $response.GetResponseStream()
        $file = [IO.File]::Open($download, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try {
            $buffer = [byte[]]::new(65536)
            $received = [long]0
            while (($count = $stream.Read($buffer, 0, $buffer.Length)) -gt 0) {
                $received += $count
                if ($received -gt $Size) { throw 'Binary exceeds the declared size.' }
                $file.Write($buffer, 0, $count)
            }
            if ($received -ne $Size) { throw 'Binary size mismatch.' }
        } finally { $file.Dispose(); $stream.Dispose() }
    } finally { $response.Dispose() }
    if ((Get-FileHash -LiteralPath $download -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Sha256) { throw 'Binary SHA-256 mismatch.' }
    # 目录存在即停止；保留注册失败产生的身份，禁止覆盖重试。
    Assert-NewPath $Directory
    Assert-NewPath $BinaryDirectory
    [IO.Directory]::CreateDirectory($Directory) | Out-Null
    Set-PrivateDirectory $Directory $account.SID ([Security.AccessControl.FileSystemRights]::Modify)
    [IO.Directory]::CreateDirectory($BinaryDirectory) | Out-Null
    Set-PrivateDirectory $BinaryDirectory $account.SID ([Security.AccessControl.FileSystemRights]::ReadAndExecute)
    $binary = Join-Path $BinaryDirectory 'artifactory-emissary.exe'
    [IO.File]::Copy($download, $binary, $false)
    $caPath = Join-Path $Directory 'ca.pem'
    [IO.File]::WriteAllBytes($caPath, $ca)
    $config = Join-Path $Directory 'emissary.json'
    $root = Join-Path $Directory 'data'
    Invoke-ServiceAccount -Arguments @('version')
    [ArtifactInstallationNative]::GrantServiceLogon($account.SID.Value)
    try { Invoke-ServiceAccount -Arguments @('register', '--endpoint', $Endpoint, '--ca', $caPath, '--config', $config, '--root', $root) -Enroll $true }
    finally { $enrollmentToken = $null }
    Invoke-ServiceAccount -Arguments @('doctor', '--config', $config, '--root', $root)
    Invoke-ServiceAccount -Arguments @('ota-init', '--config', $config, '--public-keys', $PublicKeys)
    try {
        $env:ARTIFACTORY_SERVICE_PASSWORD = $credential.GetNetworkCredential().Password
        Invoke-Administrator -Arguments @('service', '--action', 'install', '--user', ".\$User", '--config', $config)
    } finally { Remove-Item Env:\ARTIFACTORY_SERVICE_PASSWORD -ErrorAction SilentlyContinue }
    Invoke-Administrator -Arguments @('service', '--action', 'start', '--config', $config)
    Write-Host 'Emissary service started. Check the node console for its authenticated heartbeat.'
} finally {
    $enrollmentToken = $null
    $credential = $null
    Remove-Item Env:\ARTIFACTORY_ENROLLMENT_TOKEN -ErrorAction SilentlyContinue
    Remove-Item Env:\ARTIFACTORY_SERVICE_PASSWORD -ErrorAction SilentlyContinue
    if ($temporary -and (Test-Path -LiteralPath $temporary)) { Remove-Item -LiteralPath $temporary -Recurse -Force }
}
