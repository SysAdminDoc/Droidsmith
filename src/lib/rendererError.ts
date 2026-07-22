export function createRedactedRendererErrorSummary(
  error: unknown,
  componentStack = "",
): string {
  const name =
    error instanceof Error
      ? error.name.replace(/[^A-Za-z0-9_.-]/gu, "").slice(0, 40) || "Error"
      : "UnknownError";
  const rawMessage = error instanceof Error ? error.message : String(error);
  const components = [
    ...componentStack.matchAll(/\bat ([A-Z][A-Za-z0-9_.$]*)/gu),
  ]
    .slice(0, 12)
    .map((match) => match[1])
    .join(" > ");
  return [
    "Droidsmith renderer error",
    `Name: ${name}`,
    `Message: ${redactRendererText(rawMessage)}`,
    `Components: ${components || "unavailable"}`,
  ].join("\n");
}

function redactRendererText(value: string): string {
  const printable = [...value]
    .map((character) => {
      const code = character.charCodeAt(0);
      return code < 32 || code === 127 ? " " : character;
    })
    .join("");
  return printable
    .replace(/\b[A-Za-z][A-Za-z0-9+.-]*:\/\/\S+/gu, "<url>")
    .replace(/\b[A-Za-z]:[\\/][^\s"'<>]*/gu, "<path>")
    .replace(
      /(^|\s)\/(?:Users|home|private|tmp|var|etc|opt)\/[^\s"'<>]*/gu,
      "$1<path>",
    )
    .replace(/\b[A-Fa-f0-9]{8}-[A-Fa-f0-9-]{27,36}\b/gu, "<id>")
    .replace(/\b(?:\d{1,3}\.){3}\d{1,3}\b/gu, "<network>")
    .replace(/\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b/giu, "<email>")
    .replace(
      /\b(serial|token|password|secret|host|endpoint|device(?:[ _-]?id)?)(\s*[:=]\s*)[^\s,;]+/giu,
      "$1$2<redacted>",
    )
    .replace(/\b[A-Za-z0-9_-]{24,}\b/gu, "<redacted>")
    .trim()
    .slice(0, 500);
}
