//! Deterministic graph algorithms shared by semantic capabilities.

const compareText = (left, right) => Buffer.compare(Buffer.from(left), Buffer.from(right));

const edgeAdjacency = (edges) => {
  const adjacency = new Map();
  for (const edge of edges) {
    const source = edge.source.path;
    const target = edge.target.path;
    const targets = adjacency.get(source) ?? new Set();
    targets.add(target);
    adjacency.set(source, targets);
  }
  return adjacency;
};

const graphNodes = (adjacency) =>
  [
    ...new Set([
      ...adjacency.keys(),
      ...[...adjacency.values()].flatMap((targets) => [...targets]),
    ]),
  ].toSorted(compareText);

const finishingOrder = (adjacency) => {
  const visited = new Set();
  const finished = [];
  for (const root of graphNodes(adjacency)) {
    if (visited.has(root)) continue;
    visited.add(root);
    const stack = [
      { node: root, targets: [...(adjacency.get(root) ?? [])].toSorted(compareText), i: 0 },
    ];
    while (stack.length > 0) {
      const frame = stack.at(-1);
      if (frame.i >= frame.targets.length) {
        finished.push(frame.node);
        stack.pop();
        continue;
      }
      const target = frame.targets[frame.i];
      frame.i += 1;
      if (visited.has(target)) continue;
      visited.add(target);
      stack.push({
        node: target,
        targets: [...(adjacency.get(target) ?? [])].toSorted(compareText),
        i: 0,
      });
    }
  }
  return finished;
};

const reverseAdjacency = (adjacency) => {
  const reversed = new Map(graphNodes(adjacency).map((node) => [node, new Set()]));
  for (const [source, targets] of adjacency) {
    targets.forEach((target) => reversed.get(target).add(source));
  }
  return reversed;
};

const stronglyConnectedComponents = (adjacency) => {
  const reversed = reverseAdjacency(adjacency);
  const visited = new Set();
  const components = [];
  for (const root of finishingOrder(adjacency).toReversed()) {
    if (visited.has(root)) continue;
    const component = [];
    const pending = [root];
    visited.add(root);
    while (pending.length > 0) {
      const node = pending.pop();
      component.push(node);
      for (const target of [...(reversed.get(node) ?? [])].toSorted(compareText).toReversed()) {
        if (visited.has(target)) continue;
        visited.add(target);
        pending.push(target);
      }
    }
    components.push(component.toSorted(compareText));
  }
  return components;
};

const componentCycle = (adjacency, component) => {
  const allowed = new Set(component);
  const start = component[0];
  const queue = [...(adjacency.get(start) ?? [])]
    .filter((target) => allowed.has(target))
    .toSorted(compareText)
    .map((target) => [start, target]);
  const visited = new Set(queue.map((route) => route.at(-1)));
  while (queue.length > 0) {
    const current = queue.shift();
    const node = current.at(-1);
    for (const target of [...(adjacency.get(node) ?? [])].toSorted(compareText)) {
      if (target === start) return [...current, start];
      if (!allowed.has(target) || visited.has(target)) continue;
      visited.add(target);
      queue.push([...current, target]);
    }
  }
  return [];
};

export const findCycles = (edges) => {
  const adjacency = edgeAdjacency(edges);
  return stronglyConnectedComponents(adjacency)
    .filter((component) => component.length > 1)
    .map((component) => componentCycle(adjacency, component))
    .filter((cycle) => cycle.length > 0)
    .toSorted((left, right) => compareText(left.join("\0"), right.join("\0")));
};
