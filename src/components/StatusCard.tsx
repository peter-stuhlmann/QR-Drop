import type { ReactNode } from "react";
import type { Tone } from "../state/session";
import { GlassCard } from "./GlassCard";
import { StatusIcon } from "./StatusIcon";

/** Glass card with a pulsing status icon, a title and short lines of text. */
export function StatusCard({
  tone,
  icon,
  title,
  lines,
}: {
  tone: Tone;
  icon: ReactNode;
  title: string;
  lines: string[];
}) {
  return (
    <GlassCard>
      <div className="flex flex-col items-center gap-1.5 px-4 text-center">
        <StatusIcon tone={tone}>{icon}</StatusIcon>
        <span className="h-1.5" />
        <p className="text-title font-semibold">{title}</p>
        {lines.map((line, i) => (
          // Static lines that never reorder; two may read the same.
          <p key={i} className="text-caption leading-snug text-[var(--text-secondary)]">
            {line}
          </p>
        ))}
      </div>
    </GlassCard>
  );
}
