import { useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge, Button, Card } from "./common";
import HostDoctor from "./HostDoctor";

type Step = {
  titleKey: string;
  contentKeys: string[];
  tipKey?: string;
};

const STEPS: Step[] = [
  {
    titleKey: "onboarding.steps.developerOptions.title",
    contentKeys: [
      "onboarding.steps.developerOptions.content.0",
      "onboarding.steps.developerOptions.content.1",
      "onboarding.steps.developerOptions.content.2",
      "onboarding.steps.developerOptions.content.3",
    ],
    tipKey: "onboarding.steps.developerOptions.tip",
  },
  {
    titleKey: "onboarding.steps.usbDebugging.title",
    contentKeys: [
      "onboarding.steps.usbDebugging.content.0",
      "onboarding.steps.usbDebugging.content.1",
      "onboarding.steps.usbDebugging.content.2",
    ],
    tipKey: "onboarding.steps.usbDebugging.tip",
  },
  {
    titleKey: "onboarding.steps.drivers.title",
    contentKeys: [
      "onboarding.steps.drivers.content.0",
      "onboarding.steps.drivers.content.1",
      "onboarding.steps.drivers.content.2",
      "onboarding.steps.drivers.content.3",
    ],
    tipKey: "onboarding.steps.drivers.tip",
  },
  {
    titleKey: "onboarding.steps.authorize.title",
    contentKeys: [
      "onboarding.steps.authorize.content.0",
      "onboarding.steps.authorize.content.1",
      "onboarding.steps.authorize.content.2",
      "onboarding.steps.authorize.content.3",
    ],
    tipKey: "onboarding.steps.authorize.tip",
  },
  {
    titleKey: "onboarding.steps.wireless.title",
    contentKeys: [
      "onboarding.steps.wireless.content.0",
      "onboarding.steps.wireless.content.1",
      "onboarding.steps.wireless.content.2",
      "onboarding.steps.wireless.content.3",
      "onboarding.steps.wireless.content.4",
    ],
    tipKey: "onboarding.steps.wireless.tip",
  },
];

export default function OnboardingTour({
  onDismiss,
}: {
  onDismiss: () => void;
}) {
  const { t } = useTranslation();
  const [step, setStep] = useState(0);
  const current = STEPS[step]!;
  const isLast = step === STEPS.length - 1;

  return (
    <Card
      surface="dialog"
      className="mx-auto max-h-[94vh] max-w-2xl overflow-y-auto p-6"
    >
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <h2
            id="onboarding-title"
            className="text-lg font-semibold text-anvil-50"
          >
            {t("onboarding.title")}
          </h2>
          <Badge tone="info">
            {step + 1} / {STEPS.length}
          </Badge>
        </div>
        <Button type="button" size="sm" variant="ghost" onClick={onDismiss}>
          {t("onboarding.skipTour")}
        </Button>
      </div>

      <div className="mt-2 flex gap-1">
        {STEPS.map((item, index) => (
          <div
            key={item.titleKey}
            className={[
              "h-1 flex-1 rounded-sm transition-colors",
              index <= step ? "bg-circuit-300" : "bg-white/10",
            ].join(" ")}
          />
        ))}
      </div>

      <div className="mt-6">
        <h3 className="text-base font-semibold text-anvil-50">
          {t(current.titleKey)}
        </h3>
        <ol className="mt-4 space-y-3">
          {current.contentKeys.map((lineKey, index) => (
            <li
              key={lineKey}
              className="flex gap-3 text-sm leading-6 text-anvil-200"
            >
              <span className="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-md border border-white/10 bg-white/[0.04] font-mono text-[11px] text-anvil-300">
                {index + 1}
              </span>
              <span>{t(lineKey)}</span>
            </li>
          ))}
        </ol>
        {current.tipKey && (
          <div className="mt-4 rounded-md border border-circuit-300/20 bg-circuit-950/30 p-3">
            <p className="text-xs leading-5 text-circuit-100">
              <span className="font-semibold">{t("onboarding.tip")}:</span>{" "}
              {t(current.tipKey)}
            </p>
          </div>
        )}
      </div>

      <div className="mt-6 flex justify-between">
        <Button
          type="button"
          size="sm"
          disabled={step === 0}
          onClick={() => setStep((value) => value - 1)}
        >
          {t("onboarding.previous")}
        </Button>
        {isLast ? (
          <Button type="button" size="sm" variant="primary" onClick={onDismiss}>
            {t("onboarding.done")}
          </Button>
        ) : (
          <Button
            type="button"
            size="sm"
            variant="primary"
            onClick={() => setStep((value) => value + 1)}
          >
            {t("onboarding.next")}
          </Button>
        )}
      </div>

      <div className="mt-6 border-t border-white/10 pt-6">
        <HostDoctor />
      </div>
    </Card>
  );
}
