import { t } from "../i18n";
import { GlassCard } from "./GlassCard";
import { ProgressRing } from "./ProgressRing";

/** Live transfer card: percentage in the ring, size line below, optional file name. */
export function TransferCard({
  fraction,
  detail,
  fileName,
}: {
  /** 0–1, or null while unknown. */
  fraction: number | null;
  /** "2,7 MB von 4,2 MB" or "Datei 3 von 5". */
  detail: string;
  fileName?: string;
}) {
  const percent = `${Math.round((fraction ?? 0) * 100)} %`;
  return (
    <GlassCard>
      <ProgressRing
        value={fraction}
        label={t("a11y.transfer")}
        valueText={fraction === null ? detail : `${percent}, ${detail}`}
      >
        <span className="tabular text-[40px] leading-none font-bold tracking-tight">{percent}</span>
        <span className="tabular mt-1.5 text-caption font-medium text-[var(--text-secondary)]">
          {detail}
        </span>
        {fileName ? (
          <span className="mt-0.5 max-w-[150px] truncate text-[12px] text-[var(--text-tertiary)]">
            {fileName}
          </span>
        ) : null}
      </ProgressRing>
    </GlassCard>
  );
}
