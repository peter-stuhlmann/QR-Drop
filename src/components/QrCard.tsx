import { motion } from "motion/react";
import { EASE_OUT_EXPO } from "../lib/motion";
import { GlassCard } from "./GlassCard";

/** White plate of the QR code, also the skeleton's footprint. */
const PLATE = "relative grid size-[var(--size-qr-plate)] place-items-center overflow-hidden";

/**
 * QR code on pure white with quiet zone (scannability first, §7.2) and a
 * short shimmer when it appears. Shows the skeleton until the SVG exists.
 */
export function QrCard({ svg, label }: { svg: string | null; label: string }) {
  if (svg === null) return <QrSkeleton />;
  return (
    <GlassCard>
      <div
        className={PLATE}
        style={{ background: "var(--color-qr-bg)", borderRadius: "var(--radius-qr-plate)" }}
      >
        {/* An <img>, not innerHTML: the SVG can never run script or touch the DOM.
            SVG images stay vector, so the code is crisp at any scale. */}
        <img
          src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`}
          alt={label}
          draggable={false}
          className="block size-[var(--size-qr-code)]"
        />
        <motion.div
          aria-hidden
          className="pointer-events-none absolute inset-0"
          style={{
            background:
              "linear-gradient(105deg, transparent 30%, rgb(255 255 255 / 0.85) 50%, transparent 70%)",
          }}
          initial={{ x: "-100%" }}
          animate={{ x: "100%" }}
          transition={{ duration: 0.9, ease: EASE_OUT_EXPO, delay: 0.1 }}
        />
      </div>
    </GlassCard>
  );
}

/** Placeholder while the files are scanned / zipped. */
export function QrSkeleton() {
  return (
    <GlassCard>
      <div
        className={PLATE}
        style={{ background: "var(--surface-control)", borderRadius: "var(--radius-qr-plate)" }}
      >
        <div
          aria-hidden
          className="shimmer absolute inset-0"
          style={{
            background:
              "linear-gradient(105deg, transparent 20%, rgb(255 255 255 / 0.18) 50%, transparent 80%)",
          }}
        />
        <svg className="spin size-10" viewBox="0 0 40 40" aria-hidden>
          <circle
            cx="20"
            cy="20"
            r="17"
            fill="none"
            stroke="var(--progress-track)"
            strokeWidth="3.5"
          />
          <path
            d="M20 3a17 17 0 0 1 17 17"
            fill="none"
            stroke="var(--color-accent)"
            strokeWidth="3.5"
            strokeLinecap="round"
          />
        </svg>
      </div>
    </GlassCard>
  );
}
