import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const specPath = resolve(root, "..", "openapi", "openapi.yaml");
const schemaPath = resolve(root, "src", "api", "schema.d.ts");
const tempDir = mkdtempSync(join(tmpdir(), "cicd-openapi-"));
const tempSchemaPath = join(tempDir, "schema-check.d.ts");
const require = createRequire(import.meta.url);
const openapiTypescriptRoot = resolve(dirname(require.resolve("openapi-typescript")), "..");
const openapiTypescriptCli = resolve(openapiTypescriptRoot, "bin", "cli.js");

const result = spawnSync(process.execPath, [openapiTypescriptCli, specPath, "-o", tempSchemaPath], {
  cwd: root,
  stdio: "inherit",
});

if (result.status !== 0) {
  rmSync(tempDir, { force: true, recursive: true });
  process.exit(result.status ?? 1);
}

const current = readFileSync(schemaPath, "utf8");
const generated = readFileSync(tempSchemaPath, "utf8");
rmSync(tempDir, { force: true, recursive: true });

if (current !== generated) {
  console.error("frontend/src/api/schema.d.ts is out of date. Run pnpm openapi:generate.");
  process.exit(1);
}
