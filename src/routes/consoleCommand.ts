export type ConsoleParseError =
  | "unterminatedQuote"
  | "trailingEscape"
  | "emptyArgument";

export type ConsoleHistoryEntry = {
  command: string;
  output: string;
  error: boolean;
  timestamp: number;
  id: number;
};

export const MAX_CONSOLE_HISTORY_ENTRIES = 100;
export const MAX_CONSOLE_HISTORY_OUTPUT_CHARS = 64 * 1024;

export function parseConsoleCommand(
  source: string,
):
  | { argv: string[]; error?: never }
  | { argv?: never; error: ConsoleParseError } {
  const argv: string[] = [];
  let token = "";
  let tokenStarted = false;
  let quote: "single" | "double" | null = null;
  let escaping = false;

  const finishToken = (): ConsoleParseError | null => {
    if (!tokenStarted) return null;
    if (!token) return "emptyArgument";
    argv.push(token);
    token = "";
    tokenStarted = false;
    return null;
  };

  for (const character of source) {
    if (escaping) {
      token += character;
      tokenStarted = true;
      escaping = false;
      continue;
    }
    if (quote === "single") {
      if (character === "'") quote = null;
      else token += character;
      tokenStarted = true;
      continue;
    }
    if (character === "\\") {
      escaping = true;
      tokenStarted = true;
      continue;
    }
    if (quote === "double") {
      if (character === '"') quote = null;
      else token += character;
      tokenStarted = true;
      continue;
    }
    if (character === "'") {
      quote = "single";
      tokenStarted = true;
    } else if (character === '"') {
      quote = "double";
      tokenStarted = true;
    } else if (/\s/u.test(character)) {
      const error = finishToken();
      if (error) return { error };
    } else {
      token += character;
      tokenStarted = true;
    }
  }

  if (escaping) return { error: "trailingEscape" };
  if (quote) return { error: "unterminatedQuote" };
  const error = finishToken();
  return error ? { error } : { argv };
}

export function appendConsoleHistory(
  history: ConsoleHistoryEntry[],
  entry: ConsoleHistoryEntry,
  omissionLabel: string,
): ConsoleHistoryEntry[] {
  const bounded = {
    ...entry,
    output: boundConsoleOutput(entry.output, omissionLabel),
  };
  return [...history, bounded].slice(-MAX_CONSOLE_HISTORY_ENTRIES);
}

function boundConsoleOutput(output: string, omissionLabel: string): string {
  if (output.length <= MAX_CONSOLE_HISTORY_OUTPUT_CHARS) return output;
  const prefix = `${omissionLabel}\n`;
  const tailLength = Math.max(
    0,
    MAX_CONSOLE_HISTORY_OUTPUT_CHARS - prefix.length,
  );
  let start = output.length - tailLength;
  const first = output.charCodeAt(start);
  if (first >= 0xdc00 && first <= 0xdfff) start += 1;
  return `${prefix}${output.slice(start)}`;
}
