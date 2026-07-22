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
    <Card className="px-0 py-3">
      <h4 className="text-sm font-semibold text-anvil-100">
        {t("debloat.compatibilityChecks")}
      </h4>
      <ul className="mt-2 grid divide-y divide-white/[0.08] sm:grid-cols-3 sm:divide-x sm:divide-y-0 sm:divide-white/[0.08]">
        {assessment.checks.map((check) => (
          <li key={check.field} className="px-3 py-2 first:ps-0 last:pe-0">
            <div className="flex items-center justify-between gap-2">
              <code className="font-mono text-xs text-anvil-200">
                {check.field}
              </code>
              <Badge tone={compatibilityTone(check.status)}>
                {t(`debloat.compatibility.${check.status}`)}
              </Badge>
            </div>
            <p className="mt-1 text-xs text-anvil-400">
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
