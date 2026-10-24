import type { FileKind } from "../lib/tauri";
import { FileBadge } from "./FileBadge";

export interface HeaderContent {
  kind: FileKind;
  title: string;
  meta: string;
  /** Full title on hover when it is shortened; omit for secrets. */
  tooltip?: string;
}

/**
 * Header row below the title bar: type tile, name and one meta line. Keeps
 * its height when empty (`content` null) so the card never jumps.
 */
export function WindowHeader({ content }: { content: HeaderContent | null }) {
  return (
    <div className="mt-2.5 flex h-[60px] shrink-0 items-center gap-3 px-1 pb-2.5">
      {content ? (
        <>
          <FileBadge kind={content.kind} />
          <div className="min-w-0 flex-1">
            <p
              className="truncate text-filename leading-tight font-semibold"
              title={content.tooltip}
            >
              {content.title}
            </p>
            <p className="tabular mt-0.5 truncate text-caption text-[var(--text-secondary)]">
              {content.meta}
            </p>
          </div>
        </>
      ) : null}
    </div>
  );
}
