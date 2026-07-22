import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import {
  DevelopmentRendererFailureProbe,
  RendererErrorBoundary,
} from "./RendererErrorBoundary";
import { changeDroidsmithLanguage, INITIAL_LANGUAGE } from "./lib/i18n";
import { initializeSettings } from "./lib/settings";
import "./index.css";

async function bootstrap() {
  let language = INITIAL_LANGUAGE;
  try {
    const loaded = await initializeSettings();
    if (loaded.settings.language) {
      language = loaded.settings.language;
    }
  } catch {
    // Settings recovery is best-effort. A storage failure must not block launch.
  }
  try {
    await changeDroidsmithLanguage(language);
  } catch {
    // A missing/corrupt locale chunk must not strand the renderer before the
    // recovery boundary mounts. English is embedded in the entry bundle.
    await changeDroidsmithLanguage("en");
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
