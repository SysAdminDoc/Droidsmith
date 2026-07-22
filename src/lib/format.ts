import { formatNumber } from "./i18n";

/**
 * Locale-aware byte formatter shared by every route that reports artifact or
 * bundle sizes (APK Analyzer, Bugreport, Diagnostics). Fraction digits and
 * grouping follow the active language.
 */
export function formatBytes(bytes: number, language: string): string {
  if (bytes < 1024) return `${formatNumber(bytes, language)} B`;
  if (bytes < 1024 * 1024) {
    return `${formatNumber(bytes / 1024, language, {
      minimumFractionDigits: 1,
      maximumFractionDigits: 1,
    })} KB`;
  }
  return `${formatNumber(bytes / (1024 * 1024), language, {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  })} MB`;
}
