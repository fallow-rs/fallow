const requireValid = (condition, message, fail) => {
  if (!condition) fail(message);
};

const validateGate = (gates, gate, fail) => {
  requireValid(
    [Number.isFinite(gates[gate]), gates[gate] >= 0].every(Boolean),
    `manifest gates.${gate} must be a non-negative number`,
    fail,
  );
};

const validateProjectIdentity = (project, seen, dependencies) => {
  const { fail, isObject, nonEmptyString, requiredProjects } = dependencies;
  requireValid(isObject(project), "every manifest project must be an object", fail);
  nonEmptyString(project.id, "project.id");
  requireValid(!seen.has(project.id), `duplicate project id: ${project.id}`, fail);
  seen.add(project.id);
  const expectedRole = requiredProjects.get(project.id);
  requireValid(Boolean(expectedRole), `unexpected corpus project: ${project.id}`, fail);
  requireValid(project.role === expectedRole, `${project.id} role must be ${expectedRole}`, fail);
};

const validateProjectSource = (project, dependencies) => {
  const { fail, nonEmptyString, normalizedRelativePath } = dependencies;
  nonEmptyString(project.label, `${project.id}.label`);
  nonEmptyString(project.repo, `${project.id}.repo`);
  nonEmptyString(project.ref, `${project.id}.ref`);
  requireValid(
    /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(project.repo),
    `${project.id}.repo must be a public owner/repository slug`,
    fail,
  );
  const fixture = normalizedRelativePath(project.fixture, `${project.id}.fixture`);
  const expectedFixture = `benchmarks/fixtures/real-world/${project.id}`;
  requireValid(
    fixture === expectedFixture,
    `${project.id}.fixture must be ${expectedFixture}`,
    fail,
  );
};

const expectedCandidates = (role) => new Map([["zero-control", "zero"]]).get(role) ?? "nonzero";

const validFeatureBuckets = (buckets) =>
  [
    Array.isArray(buckets),
    buckets.every((bucket) => [typeof bucket === "string", bucket.trim() !== ""].every(Boolean)),
  ].every(Boolean);

const validateProjectContract = (project, fail) => {
  const expectation = expectedCandidates(project.role);
  requireValid(
    project.candidate_expectation === expectation,
    `${project.id}.candidate_expectation must be ${expectation}`,
    fail,
  );
  requireValid(
    validFeatureBuckets(project.feature_buckets),
    `${project.id}.feature_buckets must be an array of non-empty strings`,
    fail,
  );
  const accuracyNeedsBucket = [
    project.role === "accuracy-core",
    project.feature_buckets.length === 0,
  ].every(Boolean);
  requireValid(
    !accuracyNeedsBucket,
    `${project.id} must declare at least one semantic feature bucket`,
    fail,
  );
  requireValid(
    new Set(project.feature_buckets).size === project.feature_buckets.length,
    `${project.id}.feature_buckets must be unique`,
    fail,
  );
};

const validateProject = (project, seen, dependencies) => {
  validateProjectIdentity(project, seen, dependencies);
  validateProjectSource(project, dependencies);
  validateProjectContract(project, dependencies.fail);
};

export const validateManifest = (manifest, dependencies) => {
  const { fail, isObject, requiredGates, requiredProjects } = dependencies;
  requireValid(
    [isObject(manifest), manifest.schema_version === 1].every(Boolean),
    "manifest schema_version must be 1",
    fail,
  );
  requireValid(
    manifest.artifact_directory === "target/type-aware-corpus",
    "manifest artifact_directory must be target/type-aware-corpus",
    fail,
  );
  requireValid(isObject(manifest.gates), "manifest gates must be an object", fail);
  requiredGates.forEach((gate) => validateGate(manifest.gates, gate, fail));
  requireValid(
    [Array.isArray(manifest.projects), manifest.projects.length === requiredProjects.size].every(
      Boolean,
    ),
    `manifest must contain exactly ${requiredProjects.size} corpus projects`,
    fail,
  );
  const seen = new Set();
  manifest.projects.forEach((project) => validateProject(project, seen, dependencies));
  requiredProjects.forEach((_role, id) =>
    requireValid(seen.has(id), `manifest is missing required project: ${id}`, fail),
  );
  return manifest;
};

const resolvePathOption = (options, field, value, resolve) => {
  options[field] = resolve(value);
};

const positiveIntegerOption = (options, field, value, parsePositiveInteger, option) => {
  options[field] = parsePositiveInteger(value, option);
};

const optionHandlers = (dependencies) => ({
  "--manifest": (options, value) =>
    resolvePathOption(options, "manifest", value, dependencies.resolve),
  "--fallow-bin": (options, value) =>
    resolvePathOption(options, "fallowBin", value, dependencies.resolve),
  "--sidecar-bin": (options, value) =>
    resolvePathOption(options, "sidecarBin", value, dependencies.resolve),
  "--out-dir": (options, value) => {
    resolvePathOption(options, "outDir", value, dependencies.resolve);
    options.outDirExplicit = true;
  },
  "--discovery-runs": (options, value, option) =>
    positiveIntegerOption(
      options,
      "discoveryRuns",
      value,
      dependencies.parsePositiveInteger,
      option,
    ),
  "--runs": (options, value, option) =>
    positiveIntegerOption(options, "runs", value, dependencies.parsePositiveInteger, option),
  "--warmups": (options, value, option) =>
    positiveIntegerOption(options, "warmups", value, dependencies.parsePositiveInteger, option),
  "--project": (options, value) => options.projects.push(value),
});

const parseOption = (argv, index, options, handlers, dependencies) => {
  const option = argv[index];
  const handler = handlers[option];
  requireValid(Boolean(handler), `unknown option: ${option}`, dependencies.fail);
  const value = dependencies.readOptionValue(argv, index, option);
  handler(options, value, option);
};

export const parseArgs = (argv, dependencies) => {
  const command = argv[0];
  if (dependencies.helpCommands.has(command)) return { command: "help" };
  requireValid(
    dependencies.commands.has(command),
    `unknown command: ${command}`,
    dependencies.fail,
  );
  const options = { command, ...dependencies.defaults(), projects: [], outDirExplicit: false };
  const handlers = optionHandlers(dependencies);
  for (let index = 1; index < argv.length; index += 2) {
    parseOption(argv, index, options, handlers, dependencies);
  }
  return options;
};
