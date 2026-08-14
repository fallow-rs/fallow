import { findCycles } from "./graph-algorithms.mjs";
import { projectSourceFiles, sourceFileIdentity } from "./semantic-identity.mjs";

const compareText = (left, right) => Buffer.compare(Buffer.from(left), Buffer.from(right));

const percentile = (values, ratio) => {
  if (values.length === 0) return 0;
  const ordered = [...values].toSorted((left, right) => left - right);
  return ordered[Math.min(ordered.length - 1, Math.floor((ordered.length - 1) * ratio))];
};

const couplingCollection = () => ({
  edges: [],
  projectFiles: new Set(),
  resolvedEntryPoints: new Set(),
});

const collectProjectCoupling = (root, query, collected, state, services) => {
  projectSourceFiles(state.project).forEach((sourceFile) =>
    collected.projectFiles.add(sourceFileIdentity(sourceFile)),
  );
  const entries = services.discoverEntryPoints(root, state.project, query.entryPoints);
  entries.forEach((entryPoint) =>
    collected.resolvedEntryPoints.add(sourceFileIdentity(entryPoint)),
  );
  collected.edges.push(
    ...services
      .publicApiGraph(root, state.project, entries)
      .edges.filter((edge) => edge.source.path !== edge.target.path),
  );
};

const collectTypeCoupling = (root, query, ready, services) => {
  const collected = couplingCollection();
  ready.forEach((state) => collectProjectCoupling(root, query, collected, state, services));
  collected.edges = services.uniqueSorted(collected.edges, (edge) => [edge.source, edge.target]);
  return collected;
};

const connectionMaps = (edges) => {
  const outgoing = new Map();
  const incoming = new Map();
  for (const edge of edges) {
    const source = edge.source.path;
    const target = edge.target.path;
    outgoing.set(source, (outgoing.get(source) ?? new Set()).add(target));
    incoming.set(target, (incoming.get(target) ?? new Set()).add(source));
  }
  return { outgoing, incoming };
};

const couplingFiles = ({ outgoing, incoming }) => {
  const files = new Set([...outgoing.keys(), ...incoming.keys()]);
  const perFile = [...files]
    .map((file) => ({
      path: file,
      outgoing_label: "public API depends on",
      outgoing_files: [...(outgoing.get(file) ?? [])].toSorted(compareText),
      incoming_label: "public types used by",
      incoming_files: [...(incoming.get(file) ?? [])].toSorted(compareText),
    }))
    .toSorted((left, right) => compareText(left.path, right.path));
  return { files, perFile };
};

const connectionDegree = (entry) => entry.outgoing_files.length + entry.incoming_files.length;

const topCouplingContributors = (perFile) =>
  [...perFile]
    .toSorted(
      (left, right) =>
        connectionDegree(right) - connectionDegree(left) || compareText(left.path, right.path),
    )
    .slice(0, 10);

const boundCoupledFile = (entry, evidenceLimit) => ({
  ...entry,
  outgoing_files: entry.outgoing_files.slice(0, evidenceLimit),
  total_outgoing_file_count: entry.outgoing_files.length,
  incoming_files: entry.incoming_files.slice(0, evidenceLimit),
  total_incoming_file_count: entry.incoming_files.length,
});

const nestedCouplingOmissionCount = (perFile, evidenceLimit) =>
  perFile.reduce(
    (count, entry) =>
      count +
      Math.max(0, entry.outgoing_files.length - evidenceLimit) +
      Math.max(0, entry.incoming_files.length - evidenceLimit),
    0,
  );

const percentage = (numerator, denominator) =>
  denominator === 0 ? null : (numerator / denominator) * 100;

const couplingConcentration = (edges, contributors) => {
  if (edges.length === 0) return 0;
  return contributors.reduce((sum, entry) => sum + connectionDegree(entry), 0) / (edges.length * 2);
};

const couplingEvidence = (edges) =>
  edges.map((edge) => ({
    source: edge.source.path,
    target: edge.target.path,
    relation: edge.relation,
    evidence: edge.evidence,
  }));

export const analyzeTypeCoupling = ({ root, query, states, evidenceLimit }, services) => {
  const readiness = services.readyProjectStates(query, states);
  if (readiness.error) return readiness.error;
  const collected = collectTypeCoupling(root, query, readiness.ready, services);
  const { edges, projectFiles, resolvedEntryPoints } = collected;
  const { files, perFile } = couplingFiles(connectionMaps(edges));
  const degrees = perFile.map(connectionDegree);
  const outgoingDegrees = perFile.map((entry) => entry.outgoing_files.length);
  const incomingDegrees = perFile.map((entry) => entry.incoming_files.length);
  const highCouplingThreshold = percentile(degrees, 0.9);
  const topContributors = topCouplingContributors(perFile);
  const cycles = query.includeCycles ? findCycles(edges) : [];
  const missingEntryPointCount = services.missingEntryPoints(query, resolvedEntryPoints);
  const nestedFileOmissionCount = nestedCouplingOmissionCount(perFile, evidenceLimit);
  const boundFile = (entry) => boundCoupledFile(entry, evidenceLimit);
  return services.boundedResult({
    query,
    assertion: edges.length > 0 ? "coupling-found" : "no-coupling-found",
    evidence: couplingEvidence(edges),
    evidenceLimit,
    data: {
      scope: "project-local-public-signatures",
      direction: "directed",
      project_size: projectFiles.size,
      files_analyzed: projectFiles.size,
      distinct_coupled_files: files.size,
      edge_count: edges.length,
      coupled_file_percentage: percentage(files.size, projectFiles.size),
      p50_distinct_connections: percentile(degrees, 0.5),
      p90_distinct_connections: percentile(degrees, 0.9),
      p95_public_api_depends_on: percentile(outgoingDegrees, 0.95),
      p95_public_types_used_by: percentile(incomingDegrees, 0.95),
      high_coupling_percentage: percentage(
        degrees.filter((degree) => degree > highCouplingThreshold).length,
        projectFiles.size,
      ),
      concentration: couplingConcentration(edges, topContributors),
      files: perFile.slice(0, evidenceLimit).map(boundFile),
      total_file_count: perFile.length,
      top_contributors: topContributors.map(boundFile),
      edges: edges.slice(0, evidenceLimit),
      cycles: cycles.slice(0, evidenceLimit),
      total_cycle_count: cycles.length,
    },
    omissions: [
      {
        reason_code: "evidence-limit",
        count:
          Math.max(0, perFile.length - evidenceLimit) +
          Math.max(0, cycles.length - evidenceLimit) +
          nestedFileOmissionCount,
      },
      ...services.unavailableProjectOmissions(states),
      { reason_code: "unknown-entry-point", count: missingEntryPointCount },
    ],
  });
};
