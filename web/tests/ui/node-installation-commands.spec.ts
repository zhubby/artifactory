/**
 * [INPUT]: 依赖命令生成器、临时目录及隔离 Shell 与 PowerShell 进程。
 * [OUTPUT]: 验证真实传参、摘要拒绝、进程策略恢复和注册 CA 保留。
 * [POS]: 浏览器命令渲染的执行契约，不安装宿主服务。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import { mkdtempSync, writeFileSync, readFileSync, existsSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { installationCommand, registrationCommand, type NodeInstallationMaterial } from "../../src/lib/node-installation";

test("Bash installer commands preserve literal arguments and reject a mismatched installer", () => {
  const directory = mkdtempSync(join(tmpdir(), "artifactory-command-"));
  try {
    const installer = "#!/bin/sh\nprintf '%s\\0' \"$@\" > \"$OUTPUT_ARGS\"\nprintf '%s' \"$ARTIFACTORY_ENROLLMENT_TOKEN\" > \"$OUTPUT_TOKEN\"\n";
    writeFileSync(join(directory, "fixture.sh"), installer);
    writeFileSync(join(directory, "curl"), '#!/bin/sh\nwhile [ "$#" -gt 0 ]; do\nif [ "$1" = "--output" ]; then cp "$INSTALLER_FIXTURE" "$2"; printf 200; exit; fi\nshift\ndone\nexit 1\n', { mode: 0o700 });
    const material: NodeInstallationMaterial = {
      token: "literal'$(touch injected)`value", expires_at: "2099-01-01T00:00:00Z", device_endpoint: "host'$(touch injected):8443",
      ca_pem: "trusted CA\n", ca_sha256: "c".repeat(64), version: "v1", os: "linux", arch: "amd64",
      download_url: "https://build.example/object?ticket=literal'$(touch injected)", sha256: "b".repeat(64), size: 1234,
      public_keys: ["a2V5"], installer_url: "https://build.example/installer.sh", installer_sha256: createHash("sha256").update(installer).digest("hex"),
    };
    const env = { ...process.env, PATH: `${directory}:${process.env.PATH}`, INSTALLER_FIXTURE: join(directory, "fixture.sh"), OUTPUT_ARGS: join(directory, "args"), OUTPUT_TOKEN: join(directory, "token") };
    const result = spawnSync("bash", ["-s"], { input: installationCommand(material), cwd: directory, env, encoding: "utf8" });
    expect(result.status, result.stderr).toBe(0);
    expect(readFileSync(join(directory, "token"), "utf8")).toBe(material.token);
    const args = readFileSync(join(directory, "args"), "utf8").split("\0");
    expect(args[args.indexOf("--endpoint") + 1]).toBe(material.device_endpoint);
    expect(args[args.indexOf("--download-url") + 1]).toBe(material.download_url);
    expect(existsSync(join(directory, "injected"))).toBe(false);
    rmSync(join(directory, "args"));
    const rejected = spawnSync("bash", ["-s"], { input: installationCommand({ ...material, installer_sha256: "0".repeat(64) }), cwd: directory, env, encoding: "utf8" });
    expect(rejected.status).toBe(1);
    expect(rejected.stderr).toContain("Installer checksum mismatch");
    expect(existsSync(join(directory, "args"))).toBe(false);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test("registration command retains the pinned CA for future service connections", () => {
  const directory = mkdtempSync(join(tmpdir(), "artifactory-register-command-"));
  try {
    writeFileSync(join(directory, "artifactory-emissary"), '#!/bin/bash\nprintf "%s" "$ARTIFACTORY_ENROLLMENT_TOKEN" > "$OUTPUT_TOKEN"\nwhile [ "$#" -gt 0 ]; do\nif [ "$1" = "--ca" ]; then printf "%s" "$2" > "$CA_PATH"; cp "$2" "$CA_COPY"; exit; fi\nshift\ndone\n', { mode: 0o700 });
    const material = { token: "register-token", device_endpoint: "agents.example.com:8443", ca_pem: "trusted CA\n", os: "linux" } as NodeInstallationMaterial;
    const env = { ...process.env, XDG_CONFIG_HOME: join(directory, "config"), PATH: `${directory}:${process.env.PATH}`, OUTPUT_TOKEN: join(directory, "token"), CA_PATH: join(directory, "ca-path"), CA_COPY: join(directory, "ca-copy") };
    const result = spawnSync("bash", ["-s"], { input: registrationCommand(material), cwd: directory, env, encoding: "utf8" });
    expect(result.status, result.stderr).toBe(0);
    expect(readFileSync(join(directory, "ca-copy"), "utf8")).toBe(material.ca_pem);
    expect(readFileSync(join(directory, "token"), "utf8")).toBe(material.token);
    expect(existsSync(readFileSync(join(directory, "ca-path"), "utf8"))).toBe(true);
    writeFileSync(join(directory, "artifactory-emissary"), '#!/bin/sh\nwhile [ "$#" -gt 0 ]; do\nif [ "$1" = "--ca" ]; then printf "%s" "$2" > "$CA_PATH"; exit 1; fi\nshift\ndone\n', { mode: 0o700 });
    const failed = spawnSync("bash", ["-s"], { input: registrationCommand(material), cwd: directory, env, encoding: "utf8" });
    expect(failed.status).toBe(1);
    expect(existsSync(readFileSync(join(directory, "ca-path"), "utf8"))).toBe(true);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test("PowerShell bootstrap validates before execution, escapes arguments and restores process policy", () => {
  const pwsh = process.env.ARTIFACTORY_TEST_PWSH || "pwsh";
  test.skip(spawnSync(pwsh, ["-NoProfile", "-Command", "exit 0"]).status !== 0, "PowerShell runtime is unavailable");
  const directory = mkdtempSync(join(tmpdir(), "artifactory-powershell-"));
  try {
    const installer = `param([string]$DownloadUrl,[string]$Sha256,[long]$Size,[string]$Endpoint,[string]$CaBase64,[string]$CaSha256,[string]$PublicKeys,[string]$Os,[string]$Arch)
[IO.File]::WriteAllText($env:OUTPUT_ARGS, (@{ endpoint=$Endpoint; url=$DownloadUrl; token=$env:ARTIFACTORY_ENROLLMENT_TOKEN } | ConvertTo-Json))
`;
    writeFileSync(join(directory, "fixture.ps1"), installer);
    const material = {
      token: "literal'$(Write-Error injected)`value", device_endpoint: "host'$(Write-Error injected):8443",
      ca_pem: "trusted CA\n", ca_sha256: "c".repeat(64), os: "windows", arch: "arm64",
      download_url: "https://build.example/object?ticket=literal'$(Write-Error injected)", sha256: "b".repeat(64), size: 1234,
      public_keys: ["a2V5"], installer_url: "https://build.example/installer.ps1", installer_sha256: createHash("sha256").update(installer).digest("hex"),
    } as NodeInstallationMaterial;
    const prelude = `function Get-ExecutionPolicy { param($Scope) 'Restricted' }
function Set-ExecutionPolicy { param($Scope,$ExecutionPolicy,[switch]$Force) if ($Scope -ne 'Process') { throw 'Unexpected persistent policy change' }; Add-Content -LiteralPath $env:OUTPUT_POLICY -Value $ExecutionPolicy }
function Invoke-WebRequest { param([switch]$UseBasicParsing,$Uri,$MaximumRedirection,$TimeoutSec,$OutFile) if (!$UseBasicParsing -or $MaximumRedirection -ne 0 -or $TimeoutSec -ne 120) { throw 'Missing request constraints' }; Copy-Item -LiteralPath $env:INSTALLER_FIXTURE -Destination $OutFile }
`;
    const env = { ...process.env, INSTALLER_FIXTURE: join(directory, "fixture.ps1"), OUTPUT_ARGS: join(directory, "args.json"), OUTPUT_POLICY: join(directory, "policy") };
    function run(value: NodeInstallationMaterial) {
      writeFileSync(join(directory, "command.ps1"), installationCommand(value));
      const wrapper = `${prelude}
$parseTokens=$null; $parseErrors=$null
$ast=[System.Management.Automation.Language.Parser]::ParseFile((Join-Path $PWD 'command.ps1'),[ref]$parseTokens,[ref]$parseErrors)
if ($parseErrors.Count) { throw ($parseErrors | Out-String) }
& $ast.GetScriptBlock()
if (Test-Path Env:\\ARTIFACTORY_ENROLLMENT_TOKEN) { throw 'Token was not cleared' }
`;
      writeFileSync(join(directory, "wrapper.ps1"), wrapper);
      return spawnSync(pwsh, ["-NoProfile", "-NonInteractive", "-File", join(directory, "wrapper.ps1")], { cwd: directory, env, encoding: "utf8" });
    }
    const result = run(material);
    expect(result.status, result.stdout + result.stderr).toBe(0);
    expect(JSON.parse(readFileSync(join(directory, "args.json"), "utf8"))).toEqual({ endpoint: material.device_endpoint, url: material.download_url, token: material.token });
    expect(readFileSync(join(directory, "policy"), "utf8").trim().split(/\r?\n/)).toEqual(["Bypass", "Restricted"]);
    rmSync(join(directory, "args.json"));
    const rejected = run({ ...material, installer_sha256: "0".repeat(64) });
    expect(rejected.status).not.toBe(0);
    expect(rejected.stderr).toContain("Installer checksum mismatch");
    expect(existsSync(join(directory, "args.json"))).toBe(false);
    const http = run({ ...material, installer_url: "http://build.example/installer.ps1" });
    expect(http.status).not.toBe(0);
    expect(http.stderr).toContain("Installer URL must use HTTPS");
  } finally { rmSync(directory, { recursive: true, force: true }); }
});
