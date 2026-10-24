import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { lang } from "./i18n";
import { applyPlatformClasses, detectPlatform } from "./lib/platform";
import "./styles/index.css";

const platform = detectPlatform();
applyPlatformClasses(platform);
document.documentElement.lang = lang;

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <App platform={platform} />
    </StrictMode>,
  );
}
