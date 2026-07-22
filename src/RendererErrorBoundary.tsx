import {
  Component,
  type ErrorInfo,
  type ReactNode,
  useMemo,
  useState,
} from "react";
import { useTranslation } from "react-i18next";

import { createRedactedRendererErrorSummary } from "./lib/rendererError";
import { callRevealDiagnosticsDirectory } from "./lib/tauri";

declare global {
  interface Window {
    __DROIDSMITH_SMOKE_RENDER_FAILURE__?: boolean;
    __DROIDSMITH_SMOKE_RECOVERY_FAILURE__?: boolean;
  }
}

type BoundaryProps = { children: ReactNode };
type BoundaryState = { error: Error | null; componentStack: string };

export class RendererErrorBoundary extends Component<
  BoundaryProps,
  BoundaryState
> {
  state: BoundaryState = { error: null, componentStack: "" };

  static getDerivedStateFromError(error: Error): Partial<BoundaryState> {
    return { error };
  }

  componentDidCatch(_error: Error, info: ErrorInfo): void {
    this.setState({ componentStack: info.componentStack ?? "" });
  }

  render(): ReactNode {
    if (!this.state.error) return this.props.children;
    return (
      <RecoveryFallbackBoundary>
        <RendererRecoverySurface
          error={this.state.error}
          componentStack={this.state.componentStack}
        />
      </RecoveryFallbackBoundary>
    );
  }
}

class RecoveryFallbackBoundary extends Component<
  BoundaryProps,
  { failed: boolean }
> {
  state = { failed: false };

  static getDerivedStateFromError(): { failed: boolean } {
    return { failed: true };
  }

  render(): ReactNode {
    if (this.state.failed) {
      return (
        <main role="alert">
          <h1>Droidsmith could not render its recovery controls.</h1>
          <p>Close and reopen Droidsmith to continue.</p>
        </main>
      );
    }
    return this.props.children;
  }
}

function RendererRecoverySurface({
  error,
  componentStack,
}: {
  error: Error;
  componentStack: string;
}) {
  if (import.meta.env.DEV && window.__DROIDSMITH_SMOKE_RECOVERY_FAILURE__) {
    throw new Error("Synthetic recovery-surface failure");
  }

  const { t } = useTranslation();
  const [status, setStatus] = useState<string | null>(null);
  const summary = useMemo(
    () => createRedactedRendererErrorSummary(error, componentStack),
    [componentStack, error],
  );

  const copySummary = async () => {
    try {
      await navigator.clipboard.writeText(summary);
      setStatus(t("rendererError.copied"));
    } catch {
      setStatus(t("rendererError.copyFailed"));
    }
  };

  const openDiagnostics = async () => {
    try {
      await callRevealDiagnosticsDirectory();
      setStatus(t("rendererError.logsOpened"));
    } catch {
      setStatus(t("rendererError.openFailed"));
    }
  };

  return (
    <main className="flex min-h-screen items-center justify-center bg-[#08090d] p-5 text-anvil-100">
      <section
        role="alert"
        aria-labelledby="renderer-error-title"
        className="w-full max-w-2xl rounded-xl border border-red-300/20 bg-[#151a21] p-6 shadow-2xl"
      >
        <p className="text-xs font-semibold uppercase tracking-[0.16em] text-red-200">
          {t("rendererError.eyebrow")}
        </p>
        <h1
          id="renderer-error-title"
          className="mt-2 text-2xl font-semibold text-anvil-50"
        >
          {t("rendererError.title")}
        </h1>
        <p className="mt-3 text-sm leading-6 text-anvil-300">
          {t("rendererError.body")}
        </p>
        <label className="mt-5 block">
          <span className="text-xs font-medium text-anvil-300">
            {t("rendererError.summaryLabel")}
          </span>
          <textarea
            readOnly
            value={summary}
            rows={6}
            className="mt-2 w-full resize-y rounded-md border border-white/10 bg-black/20 px-3 py-2 font-mono text-xs leading-5 text-anvil-200 outline-none"
          />
        </label>
        <div className="mt-5 flex flex-wrap gap-2">
          <button
            type="button"
            onClick={() => window.location.reload()}
            className="rounded-md bg-circuit-300 px-4 py-2 text-sm font-semibold text-black hover:bg-circuit-200 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-circuit-200"
          >
            {t("rendererError.reload")}
          </button>
          <button
            type="button"
            onClick={() => void openDiagnostics()}
            className="rounded-md border border-white/15 bg-white/[0.06] px-4 py-2 text-sm font-medium text-anvil-100 hover:bg-white/10 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-circuit-200"
          >
            {t("rendererError.openLogs")}
          </button>
          <button
            type="button"
            onClick={() => void copySummary()}
            className="rounded-md border border-white/15 bg-white/[0.06] px-4 py-2 text-sm font-medium text-anvil-100 hover:bg-white/10 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-circuit-200"
          >
            {t("rendererError.copySummary")}
          </button>
        </div>
        {status && (
          <p role="status" className="mt-4 text-sm text-circuit-100">
            {status}
          </p>
        )}
      </section>
    </main>
  );
}

export function DevelopmentRendererFailureProbe() {
  if (import.meta.env.DEV && window.__DROIDSMITH_SMOKE_RENDER_FAILURE__) {
    throw new Error(
      "Synthetic route render failure: serial=QA123 path=C:\\Users\\QA\\private.txt",
    );
  }
  return null;
}
