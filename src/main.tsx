import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { MobileApp } from "./mobile/MobileApp";
import "./index.css";

/**
 * One bundle, two shells (ADR 0010).
 *
 * The mobile companion is approval/monitoring only, so it is chosen by the
 * platform rather than the window size — a narrow desktop window should still
 * get the full app. `?mobile=1` forces it for testing on a desktop browser.
 */
function isMobileShell(): boolean {
  const params = new URLSearchParams(window.location.search);
  if (params.has("mobile")) return params.get("mobile") !== "0";

  const tauriPlatform = (window as unknown as { __TAURI_OS_PLATFORM__?: string }).__TAURI_OS_PLATFORM__;
  if (tauriPlatform === "android" || tauriPlatform === "ios") return true;

  const touchOnly = window.matchMedia?.("(pointer: coarse)").matches ?? false;
  return touchOnly && window.innerWidth < 820;
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>{isMobileShell() ? <MobileApp /> : <App />}</React.StrictMode>
);
