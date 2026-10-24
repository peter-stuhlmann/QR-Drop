/** Dev-only gallery of every state (`?demo=1`, CLAUDE.md §10); App.tsx loads it lazily in dev only. */
import type { ReactNode } from "react";
import type { Os } from "../lib/platform";
import type { ShareInfo } from "../lib/tauri";
import type { ReceivedItem, ReceiveState } from "../state/receiveMachine";
import type { SharePhase } from "../state/shareMachine";
import type { SessionViewProps } from "../state/useShareSession";
import { DropzoneView } from "./DropzoneView";
import { ReceiveView } from "./ReceiveView";
import { ShareView } from "./ShareView";
import { WelcomeView } from "./WelcomeView";

const DEMO_QR =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 21 21" shape-rendering="crispEdges"><rect width="21" height="21" fill="#fff"/><path d="M0 0h7v7H0zM14 0h7v7h-7zM0 14h7v7H0z" fill="#000"/><path d="M1 1h5v5H1zM15 1h5v5h-5zM1 15h5v5H1z" fill="#fff"/><path d="M2 2h3v3H2zM16 2h3v3h-3zM2 16h3v3H2zM8 1h2v2H8zM11 3h2v3h-2zM8 8h5v2H8zM14 9h3v3h-3zM9 12h2v4H9zM12 14h4v2h-4zM17 15h3v4h-3zM12 18h3v2h-3zM8 18h2v2H8z" fill="#000"/></svg>';

const info: ShareInfo = {
  id: "demo",
  status: "ready",
  error: null,
  name: "Urlaubsfotos_Mallorca_Strand_Sonnenuntergang_2026.jpg",
  size: 4_200_000,
  kind: "image",
  isZip: false,
  fileCount: 1,
  content: "file",
  secret: false,
  charCount: 0,
  preview: null,
  url: "http://192.168.178.24:52817/s/demo",
  qrSvg: DEMO_QR,
  expiresInMs: 564_000,
  ttlMs: 600_000,
  networks: [
    { ip: "192.168.178.24", name: "WLAN", kind: "wifi" },
    { ip: "10.0.0.12", name: "Ethernet", kind: "ethernet" },
  ],
  selectedIp: "192.168.178.24",
};

const noop = () => {};

/** Session props without a backend: fixed data, buttons do nothing. */
function demoSession(
  shareInfo: ShareInfo,
  patch: Partial<SessionViewProps> = {},
): SessionViewProps {
  return {
    info: shareInfo,
    remainingMs: shareInfo.ttlMs ? shareInfo.ttlMs - 36_000 : 0,
    showFirewallHint: false,
    onClose: noop,
    onCancel: noop,
    onDismiss: noop,
    onRenew: noop,
    onRetry: noop,
    onSelectNetwork: noop,
    ...patch,
  };
}

interface ShareDemo {
  title: string;
  state: SharePhase;
  info?: ShareInfo;
  firewall?: boolean;
}

const secret: ShareInfo = {
  ...info,
  content: "text",
  kind: "secret",
  secret: true,
  name: "Text",
  ttlMs: 120_000,
};

const SHARE_DEMOS: ShareDemo[] = [
  { title: "preparing", state: { phase: "preparing" }, info: { ...info, status: "preparing" } },
  { title: "ready", state: { phase: "ready" } },
  { title: "ready + firewall", state: { phase: "ready" }, firewall: true },
  { title: "connected", state: { phase: "connected", interrupted: false } },
  {
    title: "transferring",
    state: { phase: "transferring", sent: 2_700_000, total: 4_200_000, bytesPerSec: 12_400_000 },
  },
  { title: "done", state: { phase: "done" } },
  { title: "expired", state: { phase: "expired" } },
  { title: "error · no_network", state: { phase: "error", reason: "no_network" } },
  { title: "error · not_found", state: { phase: "error", reason: "not_found" } },
  // Text, password and link shares (clipboard to phone).
  {
    title: "text · ready",
    state: { phase: "ready" },
    info: {
      ...info,
      content: "text",
      kind: "text",
      name: "Text",
      charCount: 128,
      preview: "Einkaufsliste: Milch, Brot, Kaffee, Tomaten …",
    },
  },
  { title: "passwort · ready", state: { phase: "ready" }, info: secret },
  { title: "passwort · done", state: { phase: "done" }, info: secret },
  {
    title: "link · ready",
    state: { phase: "ready" },
    info: {
      ...info,
      content: "url",
      kind: "link",
      name: "https://www.example.com/rezepte/lasagne",
      preview: "https://www.example.com/rezepte/lasagne",
      ttlMs: 0,
    },
  },
  {
    title: "error · clipboard_empty",
    state: { phase: "error", reason: "clipboard_empty" },
    // Same as the backend: failed shares carry no name, so no header.
    info: { ...info, name: "" },
  },
];

/** Phone → computer. */
const receiveInfo: ShareInfo = {
  ...info,
  name: "Downloads",
  size: 0,
  kind: "receive",
  content: "receive",
  fileCount: 0,
};

const RECEIVED: ReceivedItem[] = [
  { key: "1", kind: "image", name: "IMG_2041.HEIC", size: 3_100_000 },
  { key: "2", kind: "image", name: "IMG_2042.HEIC", size: 2_800_000 },
  { key: "3", kind: "video", name: "IMG_2043.MOV", size: 148_000_000 },
  { key: "4", kind: "text", name: "Hallo vom Handy 👋", size: 18 },
  { key: "5", kind: "document", name: "Mietvertrag_unterschrieben.pdf", size: 1_400_000 },
];

const RECEIVE_DEMOS: { title: string; state: ReceiveState }[] = [
  { title: "receive · ready", state: { view: { phase: "ready" }, items: [] } },
  {
    title: "receive · connected",
    state: { view: { phase: "connected", interrupted: false }, items: [] },
  },
  {
    title: "receive · uploading",
    state: {
      view: {
        phase: "uploading",
        name: "IMG_2043.MOV",
        index: 3,
        count: 5,
        sent: 96_000_000,
        total: 156_000_000,
        bytesPerSec: 18_500_000,
      },
      items: RECEIVED.slice(0, 2),
    },
  },
  {
    title: "receive · received",
    state: { view: { phase: "received", interrupted: false }, items: RECEIVED },
  },
  {
    title: "receive · received (unterbrochen)",
    state: { view: { phase: "received", interrupted: true }, items: RECEIVED.slice(0, 1) },
  },
  {
    title: "receive · expired",
    state: { view: { phase: "expired" }, items: RECEIVED.slice(0, 3) },
  },
];

const DROPZONE_HANDLERS = { onClose: noop, onPick: noop, onClipboard: noop, onReceive: noop };

function Frame({ title, children }: { title: string; children: ReactNode }) {
  return (
    <figure className="flex flex-col gap-2">
      <figcaption className="text-[12px] font-semibold text-white/80">{title}</figcaption>
      <div className="h-[540px] w-[380px] overflow-hidden rounded-[20px] shadow-2xl">
        {children}
      </div>
    </figure>
  );
}

export function DemoView({ os }: { os: Os }) {
  return (
    // The windows never scroll (html/body clip), so the gallery scrolls itself.
    <div
      className="h-full overflow-auto p-10"
      style={{
        background:
          "radial-gradient(50% 50% at 10% 10%, #6b5cf2aa, transparent), radial-gradient(50% 50% at 90% 60%, #268cf2aa, transparent), radial-gradient(40% 40% at 40% 100%, #cc4ce6aa, transparent), #0b0c18",
      }}
    >
      <div className="flex flex-wrap gap-8">
        {SHARE_DEMOS.map((demo) => (
          <Frame key={demo.title} title={demo.title}>
            <ShareView
              os={os}
              state={demo.state}
              autoCloseIn={3}
              {...demoSession(demo.info ?? info, { showFirewallHint: !!demo.firewall })}
            />
          </Frame>
        ))}
        {RECEIVE_DEMOS.map((demo) => (
          <Frame key={demo.title} title={demo.title}>
            <ReceiveView
              os={os}
              state={demo.state}
              onOpenFolder={noop}
              {...demoSession(receiveInfo)}
            />
          </Frame>
        ))}
        <Frame title="welcome">
          <WelcomeView os={os} onClose={noop} onDone={noop} />
        </Frame>
        <Frame title="dropzone">
          <DropzoneView os={os} dragging={false} {...DROPZONE_HANDLERS} />
        </Frame>
        <Frame title="dropzone · update">
          <DropzoneView
            os={os}
            dragging={false}
            update={{ phase: "available", version: "0.2.0" }}
            {...DROPZONE_HANDLERS}
          />
        </Frame>
        <Frame title="dropzone · dragging">
          <DropzoneView os={os} dragging {...DROPZONE_HANDLERS} />
        </Frame>
      </div>
    </div>
  );
}
