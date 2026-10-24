import { ClipboardX, FileX, TriangleAlert, WifiOff, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { CloseButton } from "../components/ActionButtons";
import { Button } from "../components/Button";
import { HintLine } from "../components/HintLine";
import { StatusCard } from "../components/StatusCard";
import { t } from "../i18n";
import { describeError, type ErrorMessageKey } from "../lib/errors";
import type { ErrorReason } from "../lib/tauri";

/** What a share/receive window shows in one phase. */
export interface PhaseSlots {
  /** Main card in the middle (crossfades on phase change). */
  card: ReactNode;
  /** Line or banner below the card. */
  hint?: ReactNode;
  /** Buttons at the bottom. */
  actions: ReactNode;
  /** What screen readers announce when the phase starts. */
  announcement?: string;
}

const ERROR_ICONS: Record<ErrorMessageKey, LucideIcon> = {
  no_network: WifiOff,
  not_found: FileX,
  clipboard_empty: ClipboardX,
  text_too_large: ClipboardX,
  generic: TriangleAlert,
};

/** Error phase of both windows: human text (§3.4), Close and – if it can help – Retry. */
export function errorSlots(
  reason: ErrorReason,
  { onClose, onRetry }: { onClose: () => void; onRetry: () => void },
): PhaseSlots {
  const { key, retryable } = describeError(reason);
  const Icon = ERROR_ICONS[key];
  const title = t(`error.${key}.title`);
  return {
    card: (
      <StatusCard
        tone="danger"
        icon={<Icon size={32} color="white" />}
        title={title}
        lines={[t(`error.${key}.text`)]}
      />
    ),
    hint: <HintLine>{t(`error.${key}.hint`)}</HintLine>,
    actions: (
      <>
        <CloseButton onClick={onClose} />
        {retryable ? (
          <Button variant="primary" onClick={onRetry}>
            {t("action.retry")}
          </Button>
        ) : null}
      </>
    ),
    announcement: title,
  };
}
