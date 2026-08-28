import type { Edit, SgNode, SgRoot, TypesMap } from "@codemod.com/jssg-types/main";

// Keep this module free of runtime imports: it executes inside the
// `codemod jssg run` sandbox, whose module loader only reliably resolves
// relative files and plain node built-ins.

export interface JssgReplacement {
  /** Exact source within the selected syntax node. */
  from: string;
  /**
   * Idempotency marker: when present in the file (or the selected node) the
   * replacement is considered already applied. Required whenever `to`
   * contains `from`.
   */
  marker?: string;
  /** Replacement source. */
  to: string;
}

export interface JssgNodePatch {
  /**
   * Stable text contained by the syntax node before and after transformation.
   * The tightest matching node is selected.
   */
  contains: string;
  /** Optional tree-sitter node kinds that constrain the structural match. */
  kinds?: readonly string[];
  replacements: readonly JssgReplacement[];
}

interface PendingNodeEdit<M extends TypesMap> {
  node: SgNode<M>;
  source: string;
}

/**
 * Apply exact edits inside structurally selected syntax nodes.
 *
 * Text is only replaced after JSSG has selected the tightest AST node matching
 * `contains` and (when provided) `kinds`. This keeps the codemod concise while
 * retaining the JSSG guarantees: language parsing, structural targeting,
 * non-overlapping edits, idempotency, and loud upstream-drift failures — an
 * anchor that no longer matches throws instead of silently emitting nothing.
 */
export function applyJssgNodePatches<M extends TypesMap>(
  root: SgRoot<M>,
  patches: readonly JssgNodePatch[],
): string | null {
  const rootNode = root.root();
  const pendingByRange = new Map<string, PendingNodeEdit<M>>();
  const rootSource = rootNode.text();

  for (const patch of patches) {
    // A patch whose replacements have all landed (every marker present) is
    // skipped BEFORE structural matching, so re-running over an already
    // patched file is a no-op even when a replacement rewrote its own anchor.
    if (
      patch.replacements.length > 0 &&
      patch.replacements.every(
        (replacement) =>
          replacement.marker !== undefined && rootSource.includes(replacement.marker),
      )
    ) {
      continue;
    }
    const node = findTightestNode(rootNode, patch);
    const range = node.range();
    const rangeKey = `${range.start.index}:${range.end.index}`;
    const pending = pendingByRange.get(rangeKey) ?? {
      node,
      source: node.text(),
    };

    for (const replacement of patch.replacements) {
      if (replacement.marker !== undefined && pending.source.includes(replacement.marker)) {
        continue;
      }
      if (replacement.to.length > 0 && pending.source.includes(replacement.to)) {
        continue;
      }

      const firstIndex = pending.source.indexOf(replacement.from);
      if (firstIndex === -1) {
        if (replacement.to.length === 0) {
          // A deletion has no positive marker; absence is its idempotent state.
          continue;
        }
        throw new Error(
          `JSSG patch anchor drifted in ${root.filename()}: ${JSON.stringify(
            replacement.from.slice(0, 160),
          )}`,
        );
      }

      const secondIndex = pending.source.indexOf(
        replacement.from,
        firstIndex + replacement.from.length,
      );
      if (secondIndex !== -1) {
        throw new Error(
          `JSSG patch anchor is ambiguous in ${root.filename()}: ${JSON.stringify(
            replacement.from.slice(0, 160),
          )}`,
        );
      }

      pending.source =
        pending.source.slice(0, firstIndex) +
        replacement.to +
        pending.source.slice(firstIndex + replacement.from.length);
    }

    pendingByRange.set(rangeKey, pending);
  }

  const changed = [...pendingByRange.values()].filter(({ node, source }) => source !== node.text());
  assertNonOverlapping(changed, root.filename());
  if (changed.length === 0) {
    return null;
  }

  const edits: Edit[] = changed.map(({ node, source }) => node.replace(source));
  return rootNode.commitEdits(edits);
}

function findTightestNode<M extends TypesMap>(
  rootNode: SgNode<M>,
  patch: JssgNodePatch,
): SgNode<M> {
  const regex = escapeRustRegex(patch.contains);
  // The runtime language comes from patch.json while this helper is generic
  // over every tree-sitter language, so the kind strings cannot be typed.
  const candidates = (
    patch.kinds === undefined
      ? rootNode.findAll({ rule: { regex } })
      : patch.kinds.flatMap((kind) =>
          rootNode.findAll({ rule: { kind: kind as never, regex } }),
        )
  ) as unknown as SgNode<M>[];
  const exactCandidates = candidates
    .filter((node) => node.text().includes(patch.contains))
    .sort((left, right) => left.text().length - right.text().length);

  const node = exactCandidates[0];
  if (node === undefined) {
    throw new Error(
      `JSSG selector drifted: no syntax node contains ${JSON.stringify(patch.contains)}`,
    );
  }
  return node;
}

function assertNonOverlapping<M extends TypesMap>(
  edits: PendingNodeEdit<M>[],
  filename: string,
): void {
  const ordered = edits.toSorted(
    (left, right) => left.node.range().start.index - right.node.range().start.index,
  );
  for (let index = 1; index < ordered.length; index += 1) {
    const previous = ordered[index - 1];
    const current = ordered[index];
    if (
      previous !== undefined &&
      current !== undefined &&
      previous.node.range().end.index > current.node.range().start.index
    ) {
      throw new Error(`Overlapping JSSG edits selected in ${filename}`);
    }
  }
}

function escapeRustRegex(value: string): string {
  return value.replaceAll(/[.*+?^${}()|[\]\\]/g, String.raw`\$&`);
}
