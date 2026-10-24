import { CircleCheck, Clock, FolderOpen, Smartphone } from "lucide-react";
import { motion } from "motion/react";
import { CancelButton, CloseButton, RenewButton } from "../components/ActionButtons";
import { Button } from "../components/Button";
import { FileBadge } from "../components/FileBadge";
import { GlassWindow } from "../components/GlassWindow";
import { HintBanner } from "../components/HintBanner";
import { HintLine, ReadyHint } from "../components/HintLine";
import { PhaseStage } from "../components/PhaseStage";
import { QrCard, QrSkeleton } from "../components/QrCard";
import { ReadyFooter } from "../components/ReadyFooter";
import { StatusCard } from "../components/StatusCard";
import { TitleBar } from "../components/TitleBar";
import { TransferCard } from "../components/TransferCard";
import { WindowFooter } from "../components/WindowFooter";
import { WindowHeader } from "../components/WindowHeader";
import { plural, t } from "../i18n";
import { formatBytes, formatEta, middleEllipsis } from "../lib/format";
import { EASE_OUT_EXPO } from "../lib/motion";
import type { Os } from "../lib/platform";
import {
  receivedCounts,
  uploadEta,
  uploadFraction,
  type ReceivedItem,
  type ReceiveState,
} from "../state/receiveMachine";
import { toneOf } from "../state/session";
import type { SessionViewProps } from "../state/useShareSession";
import { errorSlots, type PhaseSlots } from "./phaseSlots";

export interface ReceiveViewProps extends SessionViewProps {
  os: Os;
  state: ReceiveState;
  onOpenFolder: () => void;
}

/** "3 Dateien empfangen · 1 Text" */
function summaryTitle(items: ReceivedItem[]): string {
  const { files, texts } = receivedCounts(items);
  if (!files) return plural(texts, "receive.textsOnly.one", "receive.textsOnly.many");
  const f = plural(files, "receive.files.one", "receive.files.many");
  return texts ? `${f} · ${plural(texts, "receive.texts.one", "receive.texts.many")}` : f;
}

/** Name line of a received item: masked for passwords, first line for texts. */
function itemTitle(item: ReceivedItem): string {
  if (item.kind === "secret") return t("kind.secret");
  if (item.kind === "text") return item.name || t("kind.text");
  return middleEllipsis(item.name, 30);
}

/** What arrived so far, newest first -- the main card after a transfer. */
function ReceivedCard({ items }: { items: ReceivedItem[] }) {
  return (
    <div className="glass-card relative flex size-[284px] flex-col p-3">
      <div className="flex items-center gap-2 px-1.5 pt-0.5 pb-2.5">
        <CircleCheck size={20} color="var(--color-success)" />
        <p className="tabular text-[15px] font-semibold">{summaryTitle(items)}</p>
      </div>
      <ul className="flex min-h-0 flex-1 flex-col gap-1.5 overflow-y-auto pr-0.5 [scrollbar-width:thin]">
        {[...items].reverse().map((item, i) => {
          const isText = item.kind === "text" || item.kind === "secret";
          return (
            <motion.li
              key={item.key}
              className="flex items-center gap-2.5 rounded-xl px-2 py-1.5"
              style={{ background: "var(--surface-control)" }}
              // Only the newest item slides in.
              initial={i === 0 ? { opacity: 0, y: -6 } : false}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.25, ease: EASE_OUT_EXPO }}
            >
              <FileBadge kind={item.kind} size={30} />
              <div className="min-w-0 flex-1">
                <p className="truncate text-caption font-medium" title={item.name}>
                  {itemTitle(item)}
                </p>
                <p className="tabular text-[12px] text-[var(--text-secondary)]">
                  {isText ? t("receive.inClipboard") : formatBytes(item.size)}
                </p>
              </div>
            </motion.li>
          );
        })}
      </ul>
    </div>
  );
}

/** Card, hint, buttons and announcement for the current phase. */
function slotsFor(p: ReceiveViewProps): PhaseSlots {
  const { info, state } = p;
  const v = state.view;
  const cancel = <CancelButton onClick={p.onCancel} />;
  switch (v.phase) {
    case "preparing":
      return { card: <QrSkeleton />, actions: cancel };
    case "ready":
      return {
        card: (
          <QrCard svg={info?.qrSvg ?? null} label={t("a11y.qr", { name: t("receive.title") })} />
        ),
        hint: <ReadyHint showFirewallHint={p.showFirewallHint}>{t("share.scan")}</ReadyHint>,
        actions: (
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
    case "connected": {
      const line = v.interrupted ? t("receive.interrupted") : t("receive.connected.text");
      return {
        card: (
          <StatusCard
            tone="accent"
            icon={<Smartphone size={30} color="white" />}
            title={t("share.connected.title")}
            lines={[line]}
          />
        ),
        hint: <HintLine>{t("share.connected.hint")}</HintLine>,
        actions: cancel,
        announcement: v.interrupted ? line : t("share.connected.title"),
      };
    }
    case "uploading": {
      const eta = uploadEta(v);
      const speed = formatBytes(v.bytesPerSec);
      return {
        card: (
          <TransferCard
            fraction={uploadFraction(v)}
            detail={
              v.count > 1
                ? t("receive.fileOf", { i: v.index, n: v.count })
                : t("share.transfer.of", { sent: formatBytes(v.sent), total: formatBytes(v.total) })
            }
            fileName={v.name}
          />
        ),
        hint: (
          <HintLine>
            {eta === null
              ? `${speed}/s`
              : t("share.transfer.speed", { speed, eta: formatEta(eta) })}
          </HintLine>
        ),
        actions: cancel,
      };
    }
    case "received":
      return {
        card: <ReceivedCard items={state.items} />,
        hint: v.interrupted ? (
          <HintBanner>{t("receive.interrupted")}</HintBanner>
        ) : (
          <HintLine icon={<Smartphone size={16} color="var(--text-link)" />}>
            {t("receive.more")}
          </HintLine>
        ),
        actions: (
          <>
            <Button variant="primary" icon={<FolderOpen size={14} />} onClick={p.onOpenFolder}>
              {t("receive.openFolder")}
            </Button>
            <Button onClick={p.onClose}>{t("action.done")}</Button>
          </>
        ),
        announcement: v.interrupted ? t("receive.interrupted") : summaryTitle(state.items),
      };
    case "expired": {
      const hasItems = state.items.length > 0;
      return {
        card: (
          <StatusCard
            tone="warning"
            icon={<Clock size={32} color="white" />}
            title={t("share.expired.title")}
            lines={[t("receive.expired.text")]}
          />
        ),
        hint: hasItems ? <HintLine>{summaryTitle(state.items)}</HintLine> : null,
        actions: (
          <>
            {/* Icon only: three labelled buttons do not fit the 380 px window. */}
            {hasItems ? (
              <Button
                icon={<FolderOpen size={14} />}
                aria-label={t("receive.openFolder")}
                title={t("receive.openFolder")}
                onClick={p.onOpenFolder}
              />
            ) : null}
            <CloseButton onClick={p.onClose} />
            <RenewButton onClick={p.onRenew} />
          </>
        ),
        announcement: t("share.expired.title"),
      };
    }
    case "error":
      return errorSlots(v.reason, p);
    case "cancelled":
      // The window is closing; keep the button row so nothing jumps.
      return { card: null, actions: cancel };
  }
}

/** Receive window (phone → computer): QR code, live progress and what arrived. */
export function ReceiveView(p: ReceiveViewProps) {
  const { phase } = p.state.view;
  const slots = slotsFor(p);
  return (
    <GlassWindow>
      <TitleBar os={p.os} onClose={p.onDismiss} />
      <WindowHeader
        content={{
          kind: "receive",
          title: t("receive.title"),
          // Rust names the target folder; "…" until the first answer.
          meta: p.info?.name ? t("receive.meta", { folder: p.info.name }) : "…",
        }}
      />
      <PhaseStage phase={phase} tone={toneOf(phase)} announcement={slots.announcement}>
        {slots.card}
      </PhaseStage>
      <WindowFooter hint={slots.hint} actions={slots.actions} />
    </GlassWindow>
  );
}
