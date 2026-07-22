import { useTranslation } from "react-i18next";

import type { PackAssessment } from "../../lib/tauri";
import { Badge, Card } from "../common";
import { compatibilityTone } from "./tones";

export function CompatibilityChecks({
  assessment,
}: {
  assessment: PackAssessment;
}) {
  const { t } = useTranslation();
  return (
    <Card className="p-4">
      <h4 className="text-xs font-semibold text-anvil-200">
        {t("debloat.compatibilityChecks")}
      </h4>
      <ul className="mt-3 grid gap-2 sm:grid-cols-2">
        {assessment.checks.map((check) => (
          <li
            key={check.field}
            className="rounded-md border border-white/10 bg-white/[0.02] p-3"
          >
            <div className="flex items-center justify-between gap-2">
              <code className="font-mono text-xs text-anvil-200">
                {check.field}
              </code>
              <Badge tone={compatibilityTone(check.status)}>
                {t(`debloat.compatibility.${check.status}`)}
              </Badge>
            </div>
            <p className="mt-2 text-[11px] text-anvil-400">
              {t("debloat.expectedActual", {
                expected: check.expected.join(", "),
                actual: check.actual ?? t("debloat.unknownValue"),
              })}
            </p>
          </li>
        ))}
      </ul>
    </Card>
  );
}
