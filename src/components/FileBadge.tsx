import {
  File,
  FileText,
  Film,
  FolderArchive,
  Headphones,
  Image,
  KeyRound,
  Link,
  MonitorDown,
  TextAlignStart,
} from "lucide-react";
import type { FileKind } from "../lib/tauri";

const LOOK: Record<FileKind, { icon: typeof File; from: string; to: string }> = {
  image: { icon: Image, from: "var(--color-accent-from)", to: "var(--color-badge-to)" },
  archive: { icon: FolderArchive, from: "var(--color-warning)", to: "var(--color-danger)" },
  video: { icon: Film, from: "var(--color-glow-magenta)", to: "var(--color-danger)" },
  document: { icon: FileText, from: "var(--color-glow-cyan)", to: "var(--color-accent)" },
  audio: { icon: Headphones, from: "var(--color-glow-cyan)", to: "var(--color-glow-magenta)" },
  file: { icon: File, from: "var(--color-file-from)", to: "var(--color-file-to)" },
  text: { icon: TextAlignStart, from: "var(--color-glow-cyan)", to: "var(--color-accent)" },
  secret: { icon: KeyRound, from: "var(--color-glow-magenta)", to: "var(--color-accent)" },
  link: { icon: Link, from: "var(--color-glow-cyan)", to: "var(--color-glow-teal)" },
  receive: { icon: MonitorDown, from: "var(--color-accent-from)", to: "var(--color-glow-cyan)" },
};

/** Size of the tile in the window header; `--radius-badge` is drawn for it. */
const DEFAULT_SIZE = 44;

/** Gradient tile with a type icon */
export function FileBadge({ kind, size = DEFAULT_SIZE }: { kind: FileKind; size?: number }) {
  // Fallback for kinds a newer backend might send.
  const look = LOOK[kind] ?? LOOK.file;
  const Icon = look.icon;
  return (
    <div
      aria-hidden
      className="grid shrink-0 place-items-center"
      style={{
        width: size,
        height: size,
        // Smaller tiles (received list) keep the proportions of the big one.
        borderRadius: `calc(var(--radius-badge) * ${size / DEFAULT_SIZE})`,
        background: `linear-gradient(135deg, ${look.from}, ${look.to})`,
        boxShadow: `inset 0 1px 0 rgb(255 255 255 / 0.35), 0 6px 16px color-mix(in oklab, ${look.from} 45%, transparent)`,
      }}
    >
      <Icon size={size / 2} color="white" strokeWidth={2} />
    </div>
  );
}
