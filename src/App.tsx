import { MotionConfig } from "motion/react";
import { lazy, Suspense } from "react";
import type { PlatformInfo } from "./lib/platform";
import { api, closeWindow } from "./lib/tauri";
import { DropzoneWindow } from "./views/DropzoneWindow";
import { ReceiveWindow } from "./views/ReceiveWindow";
import { ShareWindow } from "./views/ShareWindow";
import { WelcomeView } from "./views/WelcomeView";

/**
 * Dev-only gallery of every state (`?demo=1`, CLAUDE.md §10). Behind
 * `import.meta.env.DEV` the import is dead code in release builds, so the
 * gallery and its sample data never ship.
 */
const DemoView = import.meta.env.DEV
  ? lazy(() => import("./views/DemoView").then((m) => ({ default: m.DemoView })))
  : null;

/** Router: one bundle serves share, receive, welcome and drop-zone windows. */
export default function App({ platform }: { platform: PlatformInfo }) {
  const params = new URLSearchParams(window.location.search);
  const id = params.get("id") ?? "";

  let content;
  if (DemoView && params.get("demo")) {
    content = (
      <Suspense>
        <DemoView os={platform.os} />
      </Suspense>
    );
  } else {
    switch (params.get("view")) {
      case "share":
        content = <ShareWindow id={id} platform={platform} />;
        break;
      case "receive":
        content = <ReceiveWindow id={id} platform={platform} />;
        break;
      case "welcome":
        content = (
          <WelcomeView
            os={platform.os}
            onClose={() => void closeWindow()}
            onDone={() => void api.finishWelcome()}
          />
        );
        break;
      default:
        content = <DropzoneWindow platform={platform} />;
    }
  }
  return <MotionConfig reducedMotion="user">{content}</MotionConfig>;
}
