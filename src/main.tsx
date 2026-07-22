import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import {
  DevelopmentRendererFailureProbe,
  RendererErrorBoundary,
} from "./RendererErrorBoundary";
import i18n from "./lib/i18n";
import { initializeSettings } from "./lib/settings";
import "./index.css";

async function bootstrap() {
  try {
    const loaded = await initializeSettings();
    if (loaded.settings.language) {
      await i18n.changeLanguage(loaded.settings.language);
    }
  } catch {
    // Settings recovery is best-effort. A storage failure must not block launch.
  }
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <RendererErrorBoundary>
        <DevelopmentRendererFailureProbe />
        <App />
      </RendererErrorBoundary>
    </React.StrictMode>,
  );
}

void bootstrap();
