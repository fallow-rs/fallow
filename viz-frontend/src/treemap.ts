import type { VizData, VizFile, TreeNode, LayoutNode } from "./types";
import type { AppState } from "./state";
import type { ColorTheme } from "./colors";
import { splitPath, formatSize, contrastText } from "./utils";

// ── Deep hierarchy builder ──────────────────────────────────────

const buildTree = (
  files: VizFile[],
  currentPath: string[],
): TreeNode[] => {
  const prefix = currentPath.join("/");
  const root = new Map<string, TreeNode>();

  for (let i = 0; i < files.length; i++) {
    const file = files[i];
    const rel = prefix ? file.path.slice(prefix.length + 1) : file.path;
    if (prefix && !file.path.startsWith(prefix + "/")) continue;
    if (!rel) continue;

    insertIntoTree(root, splitPath(rel), file, i, prefix);
  }

  const result = Array.from(root.values());
  // Flatten single-child directory chains: src → src/components becomes "src/components"
  return result.map(flattenSingleChild).sort((a, b) => b.size - a.size);
};

const insertIntoTree = (
  level: Map<string, TreeNode>,
  parts: string[],
  file: VizFile,
  fileIndex: number,
  prefix: string,
): void => {
  const name = parts[0];
  const isLeaf = parts.length === 1;

  if (isLeaf) {
    level.set(name, {
      name,
      path: file.path,
      size: file.size,
      children: [],
      fileIndex,
    });
    return;
  }

  let node = level.get(name);
  if (!node) {
    const nodePath = prefix ? `${prefix}/${name}` : name;
    node = { name, path: nodePath, size: 0, children: [], fileIndex: null };
    level.set(name, node);
  }

  // Build children map lazily
  if (!(node as TreeNode & { _childMap?: Map<string, TreeNode> })._childMap) {
    (node as TreeNode & { _childMap: Map<string, TreeNode> })._childMap = new Map();
  }
  const childMap = (node as TreeNode & { _childMap: Map<string, TreeNode> })._childMap;

  insertIntoTree(childMap, parts.slice(1), file, fileIndex, node.path);

  // Sync children array and size from child map
  node.children = Array.from(childMap.values()).sort((a, b) => b.size - a.size);
  node.size = node.children.reduce((sum, c) => sum + c.size, 0);
};

/** Collapse single-child directory chains: a/b/c with only one child each → "a/b/c" */
const flattenSingleChild = (node: TreeNode): TreeNode => {
  if (node.fileIndex !== null) return node; // leaf file

  while (
    node.children.length === 1 &&
    node.children[0].fileIndex === null
  ) {
    const child = node.children[0];
    node = {
      ...child,
      name: `${node.name}/${child.name}`,
    };
  }

  node.children = node.children.map(flattenSingleChild);
  return node;
};

// ── Squarify layout ─────────────────────────────────────────────

interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

const squarify = (nodes: TreeNode[], rect: Rect): LayoutNode[] => {
  if (nodes.length === 0) return [];
  const totalSize = nodes.reduce((sum, n) => sum + n.size, 0);
  if (totalSize === 0) return [];

  const result: LayoutNode[] = [];
  layoutStrip(nodes, rect, totalSize, result);
  return result;
};

const layoutStrip = (
  nodes: TreeNode[],
  rect: Rect,
  totalSize: number,
  result: LayoutNode[],
): void => {
  if (nodes.length === 0 || totalSize === 0) return;

  if (nodes.length === 1) {
    result.push({ x: rect.x, y: rect.y, w: rect.w, h: rect.h, node: nodes[0] });
    return;
  }

  const isWide = rect.w >= rect.h;
  const side = isWide ? rect.h : rect.w;

  let row: TreeNode[] = [];
  let rowSize = 0;
  let bestAspect = Infinity;
  let i = 0;

  while (i < nodes.length) {
    const testSize = rowSize + nodes[i].size;
    const testAspect = worstAspect(row.concat(nodes[i]), testSize, side, totalSize, rect);

    if (testAspect <= bestAspect || row.length === 0) {
      row.push(nodes[i]);
      rowSize = testSize;
      bestAspect = testAspect;
      i++;
    } else {
      break;
    }
  }

  const rowFraction = rowSize / totalSize;
  const rowRect: Rect = isWide
    ? { x: rect.x, y: rect.y, w: rect.w * rowFraction, h: rect.h }
    : { x: rect.x, y: rect.y, w: rect.w, h: rect.h * rowFraction };

  let offset = 0;
  for (const node of row) {
    const fraction = node.size / rowSize;
    if (isWide) {
      const h = rowRect.h * fraction;
      result.push({ x: rowRect.x, y: rowRect.y + offset, w: rowRect.w, h, node });
      offset += h;
    } else {
      const w = rowRect.w * fraction;
      result.push({ x: rowRect.x + offset, y: rowRect.y, w, h: rowRect.h, node });
      offset += w;
    }
  }

  const remaining = nodes.slice(i);
  if (remaining.length > 0) {
    const remainRect: Rect = isWide
      ? { x: rect.x + rowRect.w, y: rect.y, w: rect.w - rowRect.w, h: rect.h }
      : { x: rect.x, y: rect.y + rowRect.h, w: rect.w, h: rect.h - rowRect.h };
    layoutStrip(remaining, remainRect, totalSize - rowSize, result);
  }
};

const worstAspect = (
  row: TreeNode[],
  rowSize: number,
  side: number,
  totalSize: number,
  rect: Rect,
): number => {
  const isWide = rect.w >= rect.h;
  const rowLength = isWide
    ? (rowSize / totalSize) * rect.w
    : (rowSize / totalSize) * rect.h;

  if (rowLength === 0) return Infinity;

  let worst = 0;
  for (const node of row) {
    const fraction = node.size / rowSize;
    const nodeLength = side * fraction;
    const aspect = Math.max(rowLength / nodeLength, nodeLength / rowLength);
    if (aspect > worst) worst = aspect;
  }
  return worst;
};

// ── Nested rendering ────────────────────────────────────────────

const DIR_HEADER_HEIGHT = 20;
const DIR_PADDING = 2;
const MIN_LABEL_WIDTH = 36;
const MIN_LABEL_HEIGHT = 14;
const FONT = '11px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
const FONT_SMALL = '9px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';

/** Severity order: unused (worst) > hasUnusedExports > clean > entryPoint (best) */
const STATUS_SEVERITY: Record<string, number> = {
  unused: 3,
  hasUnusedExports: 2,
  clean: 1,
  entryPoint: 0,
};

/** For narrow cells: pick the worst (most severe) status color from any child */
const getWorstColor = (node: TreeNode, data: VizData, theme: ColorTheme): string => {
  let worstSeverity = -1;
  let worstColor = theme.statusColors.clean;

  const walk = (n: TreeNode): void => {
    if (n.fileIndex !== null) {
      const file = data.files[n.fileIndex];
      const severity = STATUS_SEVERITY[file.status] ?? 0;
      if (severity > worstSeverity) {
        worstSeverity = severity;
        worstColor = theme.statusColors[file.status];
      }
    } else {
      for (const child of n.children) walk(child);
    }
  };

  walk(node);
  return worstColor;
};

export const renderTreemap = (state: AppState): void => {
  const { canvas, ctx, data, theme, dpr } = state;

  // Calculate available space: full viewport minus the header
  const header = document.getElementById("fallow-header");
  const headerHeight = header ? header.offsetHeight : 0;
  const containerWidth = window.innerWidth;
  const containerHeight = window.innerHeight - headerHeight;

  // Set both CSS and pixel dimensions
  canvas.style.width = `${containerWidth}px`;
  canvas.style.height = `${containerHeight}px`;
  canvas.width = containerWidth * dpr;
  canvas.height = containerHeight * dpr;
  ctx.scale(dpr, dpr);

  ctx.fillStyle = theme.bg;
  ctx.fillRect(0, 0, containerWidth, containerHeight);

  // Build nested tree
  const nodes = buildTree(data.files, state.currentPath);
  const rootRect: Rect = { x: 1, y: 1, w: containerWidth - 2, h: containerHeight - 2 };

  // Reset layout for hit-testing
  state.layout = [];

  // Layout top-level nodes, then recursively render their children
  const topLayout = squarify(nodes, rootRect);
  for (const cell of topLayout) {
    renderCell(state, cell, 0);
  }
};

const renderCell = (state: AppState, cell: LayoutNode, depth: number): void => {
  const { ctx, data, theme } = state;
  const isFile = cell.node.fileIndex !== null;
  const hasChildren = cell.node.children.length > 0;
  const layoutIndex = state.layout.length;
  state.layout.push(cell);

  const isHovered = state.hoveredIndex === layoutIndex;
  const isSearchMatch = state.searchQuery !== "" && state.searchResults.has(layoutIndex);

  if (isFile) {
    // Leaf file, fill with status color
    const file = data.files[cell.node.fileIndex!];
    ctx.fillStyle = theme.statusColors[file.status];
    ctx.fillRect(cell.x, cell.y, cell.w, cell.h);

    if (isHovered) {
      ctx.fillStyle = theme.hover;
      ctx.fillRect(cell.x, cell.y, cell.w, cell.h);
    }

    // Border
    ctx.strokeStyle = theme.border;
    ctx.lineWidth = 0.5;
    ctx.strokeRect(cell.x, cell.y, cell.w, cell.h);

    // Label
    if (cell.w > MIN_LABEL_WIDTH && cell.h > MIN_LABEL_HEIGHT) {
      ctx.fillStyle = contrastText(theme.statusColors[file.status]);
      ctx.font = FONT;
      ctx.textBaseline = "top";
      const label = truncateText(ctx, cell.node.name, cell.w - 6);
      ctx.fillText(label, cell.x + 3, cell.y + 3);

      if (cell.h > 30 && cell.w > 50) {
        ctx.font = FONT_SMALL;
        ctx.globalAlpha = 0.7;
        ctx.fillText(formatSize(cell.node.size), cell.x + 3, cell.y + 16);
        ctx.globalAlpha = 1;
      }
    }
  } else if (hasChildren) {
    // Directory with children, render as container with header + nested children
    const tooNarrow = cell.w < 30 || cell.h < 30;
    const showHeader = !tooNarrow && cell.w > 50 && cell.h > DIR_HEADER_HEIGHT + 10;
    const headerH = showHeader ? DIR_HEADER_HEIGHT : 0;

    if (tooNarrow) {
      // Cell too small to nest children, show dominant status color as summary
      const dominantColor = getWorstColor(cell.node, data, theme);
      ctx.fillStyle = dominantColor;
      ctx.fillRect(cell.x, cell.y, cell.w, cell.h);

      ctx.strokeStyle = theme.border;
      ctx.lineWidth = 0.5;
      ctx.strokeRect(cell.x, cell.y, cell.w, cell.h);

      if (isHovered) {
        ctx.fillStyle = theme.hover;
        ctx.fillRect(cell.x, cell.y, cell.w, cell.h);
      }
    } else {
      // Directory background
      ctx.fillStyle = theme.directory;
      ctx.fillRect(cell.x, cell.y, cell.w, cell.h);

      // Directory header
      if (showHeader) {
        ctx.fillStyle = depth === 0 ? "rgba(0,0,0,0.06)" : "rgba(0,0,0,0.04)";
        ctx.fillRect(cell.x, cell.y, cell.w, headerH);

        ctx.fillStyle = theme.directoryText;
        ctx.font = FONT;
        ctx.textBaseline = "top";
        const dirLabel = truncateText(ctx, cell.node.name, cell.w - 6);
        ctx.fillText(dirLabel, cell.x + 4, cell.y + 4);
      }

      // Border around directory
      ctx.strokeStyle = depth === 0 ? "rgba(0,0,0,0.2)" : theme.border;
      ctx.lineWidth = depth === 0 ? 1.5 : 0.5;
      ctx.strokeRect(cell.x, cell.y, cell.w, cell.h);

      // Recursively render children inside the remaining space
      const innerRect: Rect = {
        x: cell.x + DIR_PADDING,
        y: cell.y + headerH + DIR_PADDING,
        w: cell.w - DIR_PADDING * 2,
        h: cell.h - headerH - DIR_PADDING * 2,
      };

      if (innerRect.w > 8 && innerRect.h > 8) {
        const childLayout = squarify(cell.node.children, innerRect);
        for (const child of childLayout) {
          renderCell(state, child, depth + 1);
        }
      }

      if (isHovered) {
        ctx.fillStyle = theme.hover;
        ctx.fillRect(cell.x, cell.y, cell.w, headerH || cell.h);
      }
    }
  } else {
    // Empty directory (no children at this level)
    ctx.fillStyle = theme.directory;
    ctx.fillRect(cell.x, cell.y, cell.w, cell.h);
    ctx.strokeStyle = theme.border;
    ctx.lineWidth = 0.5;
    ctx.strokeRect(cell.x, cell.y, cell.w, cell.h);

    if (cell.w > MIN_LABEL_WIDTH && cell.h > MIN_LABEL_HEIGHT) {
      ctx.fillStyle = theme.directoryText;
      ctx.font = FONT;
      ctx.textBaseline = "top";
      ctx.fillText(truncateText(ctx, cell.node.name, cell.w - 6), cell.x + 3, cell.y + 3);
    }
  }

  // Search highlight on top of everything
  if (isSearchMatch) {
    ctx.strokeStyle = "#FFD700";
    ctx.lineWidth = 2;
    ctx.strokeRect(cell.x + 1, cell.y + 1, cell.w - 2, cell.h - 2);
  }
};

const truncateText = (
  ctx: CanvasRenderingContext2D,
  text: string,
  maxWidth: number,
): string => {
  if (ctx.measureText(text).width <= maxWidth) return text;

  // Binary search for the longest fitting substring
  let lo = 0;
  let hi = text.length;
  while (lo < hi) {
    const mid = (lo + hi + 1) >>> 1;
    if (ctx.measureText(text.slice(0, mid) + "…").width <= maxWidth) {
      lo = mid;
    } else {
      hi = mid - 1;
    }
  }
  return lo > 0 ? text.slice(0, lo) + "…" : "…";
};
