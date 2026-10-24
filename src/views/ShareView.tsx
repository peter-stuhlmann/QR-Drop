import { Clock, Smartphone } from "lucide-react";
import { CancelButton, CloseButton, RenewButton } from "../components/ActionButtons";
import { GlassWindow } from "../components/GlassWindow";
import { HintLine, ReadyHint, ScanHint } from "../components/HintLine";
import { PhaseStage } from "../components/PhaseStage";
import { QrCard, QrSkeleton } from "../components/QrCard";
import { ReadyFooter } from "../components/ReadyFooter";
import { StatusCard } from "../components/StatusCard";
import { AnimatedCheck } from "../components/StatusIcon";
import { TitleBar } from "../components/TitleBar";
import { TransferCard } from "../components/TransferCard";
import { WindowFooter } from "../components/WindowFooter";
import { WindowHeader, type HeaderContent } from "../components/WindowHeader";
import { t } from "../i18n";
import { formatBytes, formatEta, shareHeader } from "../lib/format";
import type { Os } from "../lib/platform";
import type { ShareInfo } from "../lib/tauri";
import { toneOf } from "../state/session";
import { etaSeconds, progressFraction, type SharePhase } from "../state/shareMachine";
import type { SessionViewProps } from "../state/useShareSession";
import { errorSlots, type PhaseSlots } from "./phaseSlots";

export interface ShareViewProps extends SessionViewProps {
  os: Os;
  state: SharePhase;
  /** Seconds until the finished window closes itself. */
  autoCloseIn: number;
}

/** Header: file, text or link; while preparing only the name (and how many files). */
function headerFor(info: ShareInfo | null, state: SharePhase): HeaderContent | null {
  // Failed before anything was known (e.g. empty clipboard): no header at all.
  if (!info || (!info.name && info.content === "file")) return null;
  const { title, meta } =
    state.phase === "preparing"
      ? {
          title: info.name,
          meta: info.fileCount > 1 ? t("files.many", { n: info.fileCount }) : "…",
        }
      : shareHeader(info);
  return {
    kind: info.isZip ? "archive" : info.kind,
    title,
    meta,
    tooltip: info.secret ? undefined : title,
  };
}

/** The two lines of the done card: file, text or password. */
function doneLines(info: ShareInfo | null): string[] {
  if (info?.secret) return [t("share.done.secretLine1"), t("share.done.secretLine2")];
  if (info?.content === "text") return [t("share.done.textLine1"), t("share.done.textLine2")];
  return [t("share.done.line1"), t("share.done.line2")];
}

/** Card, hint, buttons and announcement for the current phase. */
function slotsFor(p: ShareViewProps): PhaseSlots {
  const { info, state } = p;
  const cancel = <CancelButton onClick={p.onCancel} />;
  switch (state.phase) {
    case "preparing":
      return {
        card: <QrSkeleton />,
        hint: (
          <ScanHint>
            {info && info.fileCount > 1
              ? t("share.preparingMany", { n: info.fileCount })
              : t("share.preparing")}
          </ScanHint>
        ),
        actions: cancel,
      };
    case "ready": {
      const isLink = info?.content === "url";
      return {
        card: <QrCard svg={info?.qrSvg ?? null} label={t("a11y.qr", { name: info?.name ?? "" })} />,
        hint: (
          <ReadyHint showFirewallHint={p.showFirewallHint}>
            {isLink ? t("share.scanLink") : t("share.scan")}
          </ReadyHint>
        ),
        // A link QR does not expire and needs no server: just close.
        actions: isLink ? (
          <CloseButton onClick={p.onClose} />
        ) : (
          <ReadyFooter
            remainingMs={p.remainingMs}
            ttlMs={info?.ttlMs}
            networks={info?.networks ?? []}
            selectedIp={info?.selectedIp ?? null}
            onSelectNetwork={p.onSelectNetwork}
            onCancel={p.onCancel}
          />
        ),
        announcement: p.showFirewallHint ? t("firewall.hint") : undefined,
      };
    }
    case "connected": {
      const line = state.interrupted ? t("share.interrupted") : t("share.connected.text");
      return {
        card: (
          <StatusCard
            tone="accent"
            icon={<Smartphone size={30} color="white" />}
            title={t("share.connected.title")}
            lines={[line]}
          />
        ),
        hint: <ScanHint>{t("share.connected.hint")}</ScanHint>,
        actions: cancel,
        announcement: state.interrupted ? line : t("share.connected.title"),
      };
    }
    case "transferring": {
      const eta = etaSeconds(state);
      const speed = formatBytes(state.bytesPerSec);
      return {
        card: (
          <TransferCard
            fraction={progressFraction(state)}
            detail={t("share.transfer.of", {
              sent: formatBytes(state.sent),
              total: formatBytes(state.total),
            })}
          />
        ),
        hint: (
          <HintLine>
            {eta === null
              ? `${speed}/s`
              : info?.isZip
                ? t("share.transfer.zip", { speed })
                : t("share.transfer.speed", { speed, eta: formatEta(eta) })}
          </HintLine>
        ),
        actions: cancel,
      };
    }
    case "done":
      return {
        card: (
          <StatusCard
            tone="success"
            icon={<AnimatedCheck />}
            title={t("share.done.title")}
            lines={doneLines(info)}
          />
        ),
        // Ticks every second, hence not part of the announcement.
        hint: <HintLine>{t("share.done.autoclose", { s: p.autoCloseIn })}</HintLine>,
        actions: <CloseButton onClick={p.onClose} />,
        announcement: t("share.done.title"),
      };
    case "expired":
      return {
        card: (
          <StatusCard
            tone="warning"
            icon={<Clock size={32} color="white" />}
            title={t("share.expired.title")}
            lines={[t("share.expired.text")]}
          />
        ),
        hint: <HintLine>{t("share.expired.hint")}</HintLine>,
        actions: (
          <>
            <CloseButton onClick={p.onClose} />
            <RenewButton onClick={p.onRenew} />
          </>
        ),
        announcement: t("share.expired.title"),
      };
    case "error":
      return errorSlots(state.reason, p);
    case "cancelled":
      // The window is closing; keep the button row so nothing jumps.
      return { card: null, actions: cancel };
  }
}

/** Share window (computer → phone): QR code, live progress and the end states. */
export function ShareView(p: ShareViewProps) {
  const { phase } = p.state;
  const slots = slotsFor(p);
  return (
    <GlassWindow>
      <TitleBar os={p.os} onClose={p.onDismiss} />
      <WindowHeader content={headerFor(p.info, p.state)} />
      <PhaseStage phase={phase} tone={toneOf(phase)} announcement={slots.announcement}>
        {slots.card}
      </PhaseStage>
      <WindowFooter hint={slots.hint} actions={slots.actions} />
    </GlassWindow>
  );
}
