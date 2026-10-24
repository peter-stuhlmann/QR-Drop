import { ScanLine } from "lucide-react";
import type { ReactNode } from "react";
import { t } from "../i18n";
import { HintBanner } from "./HintBanner";

/** One centred line below the main card, optionally with a leading icon. */
export function HintLine({ icon, children }: { icon?: ReactNode; children: ReactNode }) {
  return (
    <p className="tabular flex min-h-[20px] items-center justify-center gap-2 text-center text-body font-medium text-[var(--text-primary)]">
      {icon}
      {children}
    </p>
  );
}

/** Hint line with the scan icon: "Mit der Handy-Kamera scannen". */
export function ScanHint({ children }: { children: ReactNode }) {
  return <HintLine icon={<ScanLine size={18} color="var(--text-link)" />}>{children}</HintLine>;
}

/** Below the QR code: how to scan, or the firewall banner after 60 s without a request. */
export function ReadyHint({
  showFirewallHint,
  children,
}: {
  showFirewallHint: boolean;
  children: ReactNode;
}) {
  return showFirewallHint ? (
    <HintBanner>{t("firewall.hint")}</HintBanner>
  ) : (
    <ScanHint>{children}</ScanHint>
  );
}
