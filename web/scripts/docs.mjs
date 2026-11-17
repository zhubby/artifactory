/**
 * [INPUT]: 依赖固定版本 mdBook、Mermaid 和本地字体。
 * [OUTPUT]: 生成公开文档及字体资产，提供预览命令。
 * [POS]: Web 构建与 mdBook 预览共用的资产入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { spawnSync, spawn } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const mode = process.argv[2] || "build";
if (!["build", "serve"].includes(mode)) {
  throw new Error(`Unknown docs command: ${mode}`);
}

for (const [command, version] of [
  ["mdbook", "mdbook v0.4.40"],
  ["mdbook-mermaid", "mdbook-mermaid 0.14.0"],
]) {
  const result = spawnSync(command, ["--version"], { encoding: "utf8" });
  if (result.status !== 0 || result.stdout.trim() !== version) {
    console.error(`Documentation requires ${version}. See docs/operations.md#文档站点.`);
    process.exit(1);
  }
}

function run(command, args) {
  const result = spawnSync(command, args, { cwd: root, stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} exited with ${result.status}`);
}

// The pinned preprocessor embeds Mermaid. Generate its assets outside the
// tracked sources so neither local builds nor Docker need a CDN or vendored JS.
const scratch = mkdtempSync(join(tmpdir(), "artifactory-mdbook-"));
try {
  writeFileSync(join(scratch, "book.toml"), '[book]\ntitle = "Mermaid assets"\n');
  run("mdbook-mermaid", ["install", scratch]);
  for (const name of ["mermaid.min.js", "mermaid-init.js"]) {
    cpSync(join(scratch, name), join(root, name));
  }

  // Keep generated fonts in the book's source assets so mdbook serve also
  // retains them on rebuild. The dependency versions are pinned by npm.
  const fonts = join(root, "docs/fonts");
  mkdirSync(fonts, { recursive: true });
  for (const [packageName, file] of [
    ["@fontsource-variable/inter", "inter-latin-wght-normal.woff2"],
    ["@fontsource/ibm-plex-mono", "ibm-plex-mono-latin-400-normal.woff2"],
  ]) {
    cpSync(join(root, "web/node_modules", packageName, "files", file), join(fonts, file));
  }

  if (mode === "build") {
    const output = join(scratch, "book");
    run("mdbook", ["build", root, "--dest-dir", output]);
    const destination = join(root, "web/public/docs");
    mkdirSync(dirname(destination), { recursive: true });
    rmSync(destination, { recursive: true, force: true });
    cpSync(output, destination, { recursive: true });
    console.log(`Documentation built at ${destination}`);
  }
} finally {
  rmSync(scratch, { recursive: true, force: true });
}

if (mode === "serve") {
  const child = spawn("mdbook", [
    "serve", root, "--dest-dir", join(root, ".local/docs-preview"),
    "--hostname", "127.0.0.1", "--port", "3001", ...process.argv.slice(3),
  ], { cwd: root, stdio: "inherit" });
  for (const signal of ["SIGINT", "SIGTERM"]) {
    process.on(signal, () => child.kill(signal));
  }
  child.on("error", (error) => { console.error(error); process.exitCode = 1; });
  child.on("exit", (code, signal) => { process.exitCode = code ?? (signal ? 1 : 0); });
}
