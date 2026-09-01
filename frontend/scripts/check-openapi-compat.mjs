import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parse } from "yaml";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const frontendRoot = resolve(scriptDir, "..");
const repoRoot = resolve(frontendRoot, "..");
const defaultCurrent = resolve(repoRoot, "openapi", "openapi.yaml");
const httpMethods = new Set(["get", "put", "post", "delete", "options", "head", "patch", "trace"]);

function usage() {
  console.log(`Usage:
  pnpm openapi:compat --base-ref origin/main [--current ../openapi/openapi.yaml]
  pnpm openapi:compat --base ../openapi/previous.yaml [--current ../openapi/openapi.yaml]
  pnpm openapi:compat --self-test

Checks that the current OpenAPI contract does not remove existing paths, methods,
responses, parameters, schema properties, required response fields, enum values,
or change existing schema types/formats in the active compatibility surface.`);
}

function parseArgs(argv) {
  const args = {
    base: null,
    baseRef: null,
    current: defaultCurrent,
    scopePrefix: null,
    selfTest: false,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--") {
      continue;
    }
    if (arg === "--help" || arg === "-h") {
      usage();
      process.exit(0);
    }
    if (arg === "--self-test") {
      args.selfTest = true;
      continue;
    }
    if (arg === "--base") {
      args.base = resolve(frontendRoot, requiredValue(argv, ++i, arg));
      continue;
    }
    if (arg === "--base-ref") {
      args.baseRef = requiredValue(argv, ++i, arg);
      continue;
    }
    if (arg === "--current") {
      args.current = resolve(frontendRoot, requiredValue(argv, ++i, arg));
      continue;
    }
    if (arg === "--scope-prefix") {
      args.scopePrefix = requiredValue(argv, ++i, arg);
      continue;
    }
    throw new Error(`unknown argument: ${arg}`);
  }
  return args;
}

function requiredValue(argv, index, flag) {
  const value = argv[index];
  if (!value || value.startsWith("--")) {
    throw new Error(`${flag} requires a value`);
  }
  return value;
}

function loadYaml(source, label) {
  const spec = parse(source);
  if (!spec || typeof spec !== "object") {
    throw new Error(`${label} is not an OpenAPI object`);
  }
  return spec;
}

function readSpec(path) {
  return loadYaml(readFileSync(path, "utf8"), path);
}

function readSpecFromGit(ref) {
  try {
    const source = execFileSync("git", ["show", `${ref}:openapi/openapi.yaml`], {
      cwd: repoRoot,
      encoding: "utf8",
      maxBuffer: 20 * 1024 * 1024,
      stdio: ["ignore", "pipe", "pipe"],
    });
    return loadYaml(source, `${ref}:openapi/openapi.yaml`);
  } catch (error) {
    const stderr = error?.stderr?.toString?.() ?? "";
    throw new Error(`unable to read openapi/openapi.yaml from ${ref}. ${stderr.trim()}`);
  }
}

function checkCompatibility(baseSpec, currentSpec, options = {}) {
  const problems = [];
  checkOperations(baseSpec, currentSpec, problems, options);
  checkComponentSchemas(baseSpec, currentSpec, problems);
  return problems;
}

function checkOperations(baseSpec, currentSpec, problems, options) {
  const basePaths = objectEntries(baseSpec.paths);
  const currentPaths = currentSpec.paths ?? {};
  for (const [path, basePathItem] of basePaths) {
    if (options.scopePrefix && !path.startsWith(options.scopePrefix)) {
      continue;
    }
    const currentPathItem = currentPaths[path];
    if (!currentPathItem) {
      problems.push(`removed path ${path}`);
      continue;
    }

    const basePathParameters = arrayValue(basePathItem.parameters);
    const currentPathParameters = arrayValue(currentPathItem.parameters);
    for (const method of Object.keys(basePathItem).filter((key) => httpMethods.has(key))) {
      const baseOperation = basePathItem[method];
      const currentOperation = currentPathItem[method];
      const operation = `${method.toUpperCase()} ${path}`;
      if (!currentOperation) {
        problems.push(`removed operation ${operation}`);
        continue;
      }
      checkParameters(
        baseSpec,
        currentSpec,
        operation,
        [...basePathParameters, ...arrayValue(baseOperation.parameters)],
        [...currentPathParameters, ...arrayValue(currentOperation.parameters)],
        problems,
      );
      checkRequestBody(baseSpec, currentSpec, operation, baseOperation.requestBody, currentOperation.requestBody, problems);
      checkResponses(baseSpec, currentSpec, operation, baseOperation.responses, currentOperation.responses, problems);
    }
  }
}

function checkParameters(baseSpec, currentSpec, operation, baseParameters, currentParameters, problems) {
  const currentByKey = new Map();
  for (const raw of currentParameters) {
    const parameter = resolveMaybeRef(currentSpec, raw);
    if (parameter?.name && parameter?.in) {
      currentByKey.set(`${parameter.in}:${parameter.name}`, parameter);
    }
  }

  for (const raw of baseParameters) {
    const baseParameter = resolveMaybeRef(baseSpec, raw);
    if (!baseParameter?.name || !baseParameter?.in) {
      continue;
    }
    const key = `${baseParameter.in}:${baseParameter.name}`;
    const currentParameter = currentByKey.get(key);
    if (!currentParameter) {
      problems.push(`${operation} removed parameter ${key}`);
      continue;
    }
    if (baseParameter.required !== true && currentParameter.required === true) {
      problems.push(`${operation} made parameter ${key} required`);
    }
    compareSchema(
      baseSpec,
      currentSpec,
      `${operation} parameter ${key}`,
      baseParameter.schema,
      currentParameter.schema,
      problems,
      { request: true },
    );
  }

  const baseKeys = new Set();
  for (const raw of baseParameters) {
    const parameter = resolveMaybeRef(baseSpec, raw);
    if (parameter?.name && parameter?.in) {
      baseKeys.add(`${parameter.in}:${parameter.name}`);
    }
  }
  for (const raw of currentParameters) {
    const parameter = resolveMaybeRef(currentSpec, raw);
    if (!parameter?.name || !parameter?.in) {
      continue;
    }
    const key = `${parameter.in}:${parameter.name}`;
    if (!baseKeys.has(key) && parameter.required === true) {
      problems.push(`${operation} added required parameter ${key}`);
    }
  }
}

function checkRequestBody(baseSpec, currentSpec, operation, baseRequestBody, currentRequestBody, problems) {
  if (!baseRequestBody) {
    if (resolveMaybeRef(currentSpec, currentRequestBody)?.required === true) {
      problems.push(`${operation} added a required request body`);
    }
    return;
  }

  const baseBody = resolveMaybeRef(baseSpec, baseRequestBody);
  const currentBody = resolveMaybeRef(currentSpec, currentRequestBody);
  if (!currentBody) {
    problems.push(`${operation} removed request body`);
    return;
  }
  if (baseBody.required !== true && currentBody.required === true) {
    problems.push(`${operation} made request body required`);
  }

  for (const [mediaType, baseMedia] of objectEntries(baseBody.content)) {
    const currentMedia = currentBody.content?.[mediaType];
    if (!currentMedia) {
      problems.push(`${operation} request body removed media type ${mediaType}`);
      continue;
    }
    compareSchema(
      baseSpec,
      currentSpec,
      `${operation} request body ${mediaType}`,
      baseMedia.schema,
      currentMedia.schema,
      problems,
      { request: true },
    );
  }
}

function checkResponses(baseSpec, currentSpec, operation, baseResponses, currentResponses, problems) {
  const current = currentResponses ?? {};
  for (const [status, rawBaseResponse] of objectEntries(baseResponses)) {
    const rawCurrentResponse = current[status];
    if (!rawCurrentResponse) {
      problems.push(`${operation} removed response ${status}`);
      continue;
    }
    const baseResponse = resolveMaybeRef(baseSpec, rawBaseResponse);
    const currentResponse = resolveMaybeRef(currentSpec, rawCurrentResponse);
    for (const [mediaType, baseMedia] of objectEntries(baseResponse.content)) {
      const currentMedia = currentResponse.content?.[mediaType];
      if (!currentMedia) {
        problems.push(`${operation} response ${status} removed media type ${mediaType}`);
        continue;
      }
      compareSchema(
        baseSpec,
        currentSpec,
        `${operation} response ${status} ${mediaType}`,
        baseMedia.schema,
        currentMedia.schema,
        problems,
        { request: false },
      );
    }
  }
}

function checkComponentSchemas(baseSpec, currentSpec, problems) {
  for (const [name, baseSchema] of objectEntries(baseSpec.components?.schemas)) {
    const currentSchema = currentSpec.components?.schemas?.[name];
    if (!currentSchema) {
      problems.push(`removed schema components.schemas.${name}`);
      continue;
    }
    compareSchema(
      baseSpec,
      currentSpec,
      `components.schemas.${name}`,
      baseSchema,
      currentSchema,
      problems,
      { request: false },
    );
  }
}

function compareSchema(baseSpec, currentSpec, location, rawBaseSchema, rawCurrentSchema, problems, options, seen = new Set()) {
  if (!rawBaseSchema) {
    return;
  }
  if (!rawCurrentSchema) {
    problems.push(`${location} removed schema`);
    return;
  }

  const baseSchema = resolveMaybeRef(baseSpec, rawBaseSchema);
  const currentSchema = resolveMaybeRef(currentSpec, rawCurrentSchema);
  const seenKey = `${location}:${refName(rawBaseSchema) ?? ""}:${refName(rawCurrentSchema) ?? ""}`;
  if (seen.has(seenKey)) {
    return;
  }
  seen.add(seenKey);

  const baseTypes = schemaTypes(baseSchema);
  const currentTypes = schemaTypes(currentSchema);
  if (baseTypes.size || currentTypes.size) {
    if (!setsEqual(baseTypes, currentTypes)) {
      problems.push(`${location} changed type ${formatSet(baseTypes)} -> ${formatSet(currentTypes)}`);
      return;
    }
  }

  if ((baseSchema.format ?? null) !== (currentSchema.format ?? null)) {
    problems.push(`${location} changed format ${baseSchema.format ?? "none"} -> ${currentSchema.format ?? "none"}`);
  }

  if (Array.isArray(baseSchema.enum)) {
    const currentEnum = new Set(arrayValue(currentSchema.enum));
    for (const value of baseSchema.enum) {
      if (!currentEnum.has(value)) {
        problems.push(`${location} removed enum value ${JSON.stringify(value)}`);
      }
    }
  }

  if (baseSchema.items || currentSchema.items) {
    compareSchema(baseSpec, currentSpec, `${location}.items`, baseSchema.items, currentSchema.items, problems, options, seen);
  }

  const baseProperties = baseSchema.properties ?? {};
  const currentProperties = currentSchema.properties ?? {};
  for (const [propertyName, baseProperty] of objectEntries(baseProperties)) {
    const currentProperty = currentProperties[propertyName];
    if (!currentProperty) {
      problems.push(`${location} removed property ${propertyName}`);
      continue;
    }
    compareSchema(
      baseSpec,
      currentSpec,
      `${location}.properties.${propertyName}`,
      baseProperty,
      currentProperty,
      problems,
      options,
      seen,
    );
  }

  const baseRequired = new Set(arrayValue(baseSchema.required));
  const currentRequired = new Set(arrayValue(currentSchema.required));
  for (const name of baseRequired) {
    if (!currentRequired.has(name)) {
      problems.push(`${location} made required property optional: ${name}`);
    }
  }
  if (options.request) {
    for (const name of currentRequired) {
      if (!baseRequired.has(name) && currentProperties[name]) {
        problems.push(`${location} added required request property ${name}`);
      }
    }
  }

  for (const keyword of ["allOf", "oneOf", "anyOf"]) {
    if (baseSchema[keyword] || currentSchema[keyword]) {
      compareSchemaArray(baseSpec, currentSpec, `${location}.${keyword}`, baseSchema[keyword], currentSchema[keyword], problems, options, seen);
    }
  }

  if (baseSchema.additionalProperties && typeof baseSchema.additionalProperties === "object") {
    compareSchema(
      baseSpec,
      currentSpec,
      `${location}.additionalProperties`,
      baseSchema.additionalProperties,
      currentSchema.additionalProperties,
      problems,
      options,
      seen,
    );
  }
}

function compareSchemaArray(baseSpec, currentSpec, location, baseItems, currentItems, problems, options, seen) {
  if (!Array.isArray(baseItems)) {
    return;
  }
  if (!Array.isArray(currentItems)) {
    problems.push(`${location} removed schema list`);
    return;
  }
  if (currentItems.length < baseItems.length) {
    problems.push(`${location} removed ${baseItems.length - currentItems.length} schema option(s)`);
  }
  for (let i = 0; i < baseItems.length; i += 1) {
    compareSchema(baseSpec, currentSpec, `${location}[${i}]`, baseItems[i], currentItems[i], problems, options, seen);
  }
}

function resolveMaybeRef(spec, value) {
  if (!value || typeof value !== "object" || !value.$ref) {
    return value;
  }
  return resolveRef(spec, value.$ref);
}

function resolveRef(spec, ref) {
  if (!ref.startsWith("#/")) {
    throw new Error(`external refs are not supported by this checker: ${ref}`);
  }
  const parts = ref
    .slice(2)
    .split("/")
    .map((part) => part.replaceAll("~1", "/").replaceAll("~0", "~"));
  let cursor = spec;
  for (const part of parts) {
    cursor = cursor?.[part];
  }
  if (!cursor) {
    throw new Error(`unresolved OpenAPI ref: ${ref}`);
  }
  return cursor;
}

function schemaTypes(schema) {
  if (!schema || typeof schema !== "object") {
    return new Set();
  }
  const raw = schema.type;
  const types = Array.isArray(raw) ? raw : raw ? [raw] : [];
  if (schema.nullable === true) {
    types.push("null");
  }
  return new Set(types);
}

function refName(value) {
  if (!value || typeof value !== "object" || !value.$ref) {
    return null;
  }
  return value.$ref;
}

function objectEntries(value) {
  return value && typeof value === "object" ? Object.entries(value) : [];
}

function arrayValue(value) {
  return Array.isArray(value) ? value : [];
}

function setsEqual(left, right) {
  if (left.size !== right.size) {
    return false;
  }
  for (const value of left) {
    if (!right.has(value)) {
      return false;
    }
  }
  return true;
}

function formatSet(values) {
  return values.size ? [...values].sort().join("|") : "unspecified";
}

function runSelfTest() {
  const base = {
    openapi: "3.1.0",
    paths: {
      "/api/v1/projects": {
        get: {
          parameters: [{ name: "limit", in: "query", required: false, schema: { type: "integer" } }],
          responses: {
            "200": {
              description: "OK",
              content: {
                "application/json": {
                  schema: { type: "array", items: { $ref: "#/components/schemas/Project" } },
                },
              },
            },
          },
        },
      },
    },
    components: {
      schemas: {
        Project: {
          type: "object",
          required: ["id", "name", "status"],
          properties: {
            id: { type: "string", format: "uuid" },
            name: { type: "string" },
            status: { type: "string", enum: ["active", "archived"] },
          },
        },
      },
    },
  };
  const additive = structuredClone(base);
  additive.paths["/api/v1/projects"].get.responses["202"] = { description: "Accepted" };
  additive.paths["/api/v1/projects/{project_id}"] = { get: { responses: { "200": { description: "OK" } } } };
  additive.components.schemas.Project.required.push("created_at");
  additive.components.schemas.Project.properties.created_at = { type: "string", format: "date-time" };
  additive.components.schemas.Project.properties.status.enum.push("paused");

  assertNoBreaks("additive response/schema changes", base, additive);
  assertBreaks("removed path", base, mutate(base, (spec) => delete spec.paths["/api/v1/projects"]), "removed path");
  assertBreaks("removed method", base, mutate(base, (spec) => delete spec.paths["/api/v1/projects"].get), "removed operation");
  assertBreaks(
    "removed response",
    base,
    mutate(base, (spec) => delete spec.paths["/api/v1/projects"].get.responses["200"]),
    "removed response 200",
  );
  assertBreaks(
    "type change",
    base,
    mutate(base, (spec) => { spec.components.schemas.Project.properties.name.type = "integer"; }),
    "changed type",
  );
  assertBreaks(
    "required parameter",
    base,
    mutate(base, (spec) => {
      spec.paths["/api/v1/projects"].get.parameters.push({ name: "sort", in: "query", required: true, schema: { type: "string" } });
    }),
    "added required parameter",
  );
  assertBreaks(
    "optional parameter became required",
    base,
    mutate(base, (spec) => { spec.paths["/api/v1/projects"].get.parameters[0].required = true; }),
    "made parameter query:limit required",
  );
  assertBreaks(
    "removed required response field",
    base,
    mutate(base, (spec) => delete spec.components.schemas.Project.properties.status),
    "removed property status",
  );
  console.log("OK: OpenAPI compatibility checker self-test passed");
}

function mutate(spec, fn) {
  const copy = structuredClone(spec);
  fn(copy);
  return copy;
}

function assertNoBreaks(name, base, current) {
  const problems = checkCompatibility(base, current);
  if (problems.length) {
    throw new Error(`self-test expected no breaks for ${name}, got: ${problems.join("; ")}`);
  }
}

function assertBreaks(name, base, current, expected) {
  const problems = checkCompatibility(base, current);
  if (!problems.some((problem) => problem.includes(expected))) {
    throw new Error(`self-test expected ${name} to report ${expected}, got: ${problems.join("; ") || "no problems"}`);
  }
}

try {
  const args = parseArgs(process.argv.slice(2));
  if (args.selfTest) {
    runSelfTest();
    process.exit(0);
  }
  if (!args.base && !args.baseRef) {
    throw new Error("provide --base <file> or --base-ref <git-ref>");
  }
  if (args.base && args.baseRef) {
    throw new Error("use only one of --base or --base-ref");
  }
  const baseSpec = args.baseRef ? readSpecFromGit(args.baseRef) : readSpec(args.base);
  const currentSpec = readSpec(args.current);
  const problems = checkCompatibility(baseSpec, currentSpec, { scopePrefix: args.scopePrefix });
  if (problems.length) {
    console.error("OpenAPI compatibility check failed:");
    for (const problem of problems) {
      console.error(`  - ${problem}`);
    }
    process.exit(1);
  }
  console.log("OK: OpenAPI compatibility check passed");
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
