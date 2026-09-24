import { execFileSync } from "node:child_process";

const allowedWorkspaceDependencies = new Map([
  ["panedeck-domain", []],
  ["panedeck-fs", ["panedeck-domain"]],
  ["panedeck-jobs", ["panedeck-domain", "panedeck-fs"]],
  ["panedeck-platform", ["panedeck-domain", "panedeck-fs"]],
  ["panedeck-app", ["panedeck-domain", "panedeck-fs", "panedeck-jobs", "panedeck-platform"]],
  ["panedeck", ["panedeck-app"]],
]);

const metadata = JSON.parse(
  execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps"], {
    encoding: "utf8",
  }),
);

const workspacePackages = new Set(metadata.packages.map((entry) => entry.name));
const problems = [];

for (const packageEntry of metadata.packages) {
  const allowed = allowedWorkspaceDependencies.get(packageEntry.name);

  if (!allowed) {
    problems.push(`unregistered workspace crate: ${packageEntry.name}`);
    continue;
  }

  const actual = packageEntry.dependencies
    .map((dependency) => dependency.name)
    .filter((name) => workspacePackages.has(name))
    .sort();
  const expected = [...allowed].sort();

  const unexpected = actual.filter((name) => !expected.includes(name));
  const missing = expected.filter((name) => !actual.includes(name));

  if (unexpected.length > 0) {
    problems.push(`${packageEntry.name} has forbidden dependencies: ${unexpected.join(", ")}`);
  }

  if (missing.length > 0) {
    problems.push(
      `${packageEntry.name} is missing declared layer dependencies: ${missing.join(", ")}`,
    );
  }
}

for (const packageName of allowedWorkspaceDependencies.keys()) {
  if (!workspacePackages.has(packageName)) {
    problems.push(`expected workspace crate is missing: ${packageName}`);
  }
}

if (problems.length > 0) {
  console.error("Cargo workspace boundary check failed:");
  for (const problem of problems) {
    console.error(`- ${problem}`);
  }
  process.exitCode = 1;
} else {
  console.log("Cargo workspace boundaries are valid.");
}
