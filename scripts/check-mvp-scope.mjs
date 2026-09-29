import { execFileSync, spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";

const packageJson = JSON.parse(readFileSync("package.json", "utf8"));
const tauriConfig = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
const capabilities = JSON.parse(readFileSync("src-tauri/capabilities/default.json", "utf8"));
const allowedRuntimeDependencies = new Set(["@tauri-apps/api", "react", "react-dom"]);
const forbiddenCargoDependencies = new Set([
  "reqwest",
  "ureq",
  "hyper",
  "sentry",
  "segment",
  "amplitude",
  "posthog",
]);
const problems = [];

for (const dependency of Object.keys(packageJson.dependencies ?? {})) {
  if (!allowedRuntimeDependencies.has(dependency)) {
    problems.push(`unexpected frontend runtime dependency: ${dependency}`);
  }
}

if (tauriConfig.bundle?.active !== false) {
  problems.push("Tauri bundling must remain disabled for the local MVP");
}
if (tauriConfig.plugins && Object.keys(tauriConfig.plugins).length > 0) {
  problems.push("Tauri distribution/update plugins are outside the MVP");
}
if (JSON.stringify(capabilities.permissions) !== JSON.stringify(["core:default"])) {
  problems.push("main-window capabilities exceed the reviewed core-only permission set");
}

const metadata = JSON.parse(
  execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps", "--locked"], {
    encoding: "utf8",
  }),
);
for (const packageEntry of metadata.packages) {
  for (const dependency of packageEntry.dependencies) {
    if (forbiddenCargoDependencies.has(dependency.name)) {
      problems.push(`${packageEntry.name} has out-of-scope dependency: ${dependency.name}`);
    }
  }
}

const sourceScan = spawnSync(
  "rg",
  [
    "-n",
    "fetch\\s*\\(|XMLHttpRequest|WebSocket|navigator\\.sendBeacon",
    "src",
    "crates",
    "src-tauri/src",
    "--glob",
    "!*.test.ts*",
    "--glob",
    "!**/tests/**",
  ],
  { encoding: "utf8" },
);
if (sourceScan.status !== 0 && sourceScan.status !== 1) {
  problems.push(`could not scan production source: ${sourceScan.stderr.trim()}`);
}
const productionSources = sourceScan.stdout;
if (productionSources.trim()) {
  problems.push(`network API found in production source:\n${productionSources.trim()}`);
}

if (problems.length > 0) {
  console.error("MVP scope check failed:");
  for (const problem of problems) console.error(`- ${problem}`);
  process.exitCode = 1;
} else {
  console.log("MVP scope is local-only: no bundling, updater, telemetry, or network client.");
}
