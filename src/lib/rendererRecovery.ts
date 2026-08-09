import i18n from "./i18n";

export function getRendererRecoveryFallbackCopy(
  translate: (key: string, fallback: string) => string = (key, fallback) =>
    i18n.t(key, { defaultValue: fallback }),
) {
  const copy = (key: string, fallback: string) =>
    translate(key, fallback) || fallback;
  return {
    title: copy(
      "rendererError.fallbackTitle",
      "Droidsmith could not render its recovery controls.",
    ),
    body: copy(
      "rendererError.fallbackBody",
      "Close and reopen Droidsmith to continue.",
    ),
  };
}
