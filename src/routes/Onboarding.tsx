import { useState } from "react";
import { Badge, Button, Card } from "./common";

type Step = {
  title: string;
  content: string[];
  tip?: string;
};

const STEPS: Step[] = [
  {
    title: "Enable Developer Options",
    content: [
      "Open Settings on your Android device.",
      "Scroll to About phone (or About tablet).",
      'Tap Build number 7 times. You\'ll see a toast saying "You are now a developer!"',
      "Go back to Settings — Developer options is now visible.",
    ],
    tip: "On Samsung: Settings → About phone → Software information → Build number.",
  },
  {
    title: "Enable USB Debugging",
    content: [
      "Open Settings → Developer options.",
      "Scroll down to USB debugging and toggle it on.",
      "Confirm the warning dialog.",
    ],
    tip: 'On Xiaomi/MIUI: also enable "USB debugging (Security settings)" to allow app installs.',
  },
  {
    title: "Install USB Drivers (Windows)",
    content: [
      "Most devices work with the Google USB Driver: download it from the Android SDK Manager.",
      "Samsung: install Samsung USB Driver from samsung.com.",
      "Other OEMs: check your manufacturer's support page for ADB/USB drivers.",
      "macOS and Linux usually don't need extra drivers.",
    ],
    tip: "On Linux, you may need a udev rule — Droidsmith shows the fix automatically when this happens.",
  },
  {
    title: "Connect and Authorize",
    content: [
      "Connect your device via USB cable.",
      'Your device will show an "Allow USB debugging?" dialog.',
      'Check "Always allow from this computer" if this is your personal machine.',
      "Tap Allow. The device should appear in Droidsmith's Devices tab.",
    ],
    tip: "If the dialog doesn't appear, try a different USB cable — charge-only cables don't support data.",
  },
  {
    title: "Wireless Debugging (Android 11+)",
    content: [
      "Open Settings → Developer options → Wireless debugging.",
      "Toggle it on and confirm.",
      "In Droidsmith, go to the Wireless tab.",
      "Use QR pairing: scan the QR code shown in Droidsmith with your device.",
      "Or use manual pairing: enter the host, port, and 6-digit code from your device.",
    ],
    tip: "Both devices must be on the same Wi-Fi network. The pairing code changes each time.",
  },
];

export default function OnboardingTour({
  onDismiss,
}: {
  onDismiss: () => void;
}) {
  const [step, setStep] = useState(0);
  const current = STEPS[step]!;
  const isLast = step === STEPS.length - 1;

  return (
    <Card className="mx-auto max-w-2xl p-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <h2 className="text-lg font-semibold text-anvil-50">
            Getting started
          </h2>
          <Badge tone="info">
            {step + 1} / {STEPS.length}
          </Badge>
        </div>
        <Button type="button" size="sm" variant="ghost" onClick={onDismiss}>
          Skip tour
        </Button>
      </div>

      <div className="mt-2 flex gap-1">
        {STEPS.map((_, i) => (
          <div
            key={i}
            className={[
              "h-1 flex-1 rounded-sm transition-colors",
              i <= step ? "bg-circuit-300" : "bg-white/10",
            ].join(" ")}
          />
        ))}
      </div>

      <div className="mt-6">
        <h3 className="text-base font-semibold text-anvil-50">
          {current.title}
        </h3>
        <ol className="mt-4 space-y-3">
          {current.content.map((line, i) => (
            <li key={i} className="flex gap-3 text-sm leading-6 text-anvil-200">
              <span className="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-md border border-white/10 bg-white/[0.04] font-mono text-[11px] text-anvil-300">
                {i + 1}
              </span>
              <span>{line}</span>
            </li>
          ))}
        </ol>
        {current.tip && (
          <div className="mt-4 rounded-md border border-circuit-300/20 bg-circuit-950/30 p-3">
            <p className="text-xs leading-5 text-circuit-100">
              <span className="font-semibold">Tip:</span> {current.tip}
            </p>
          </div>
        )}
      </div>

      <div className="mt-6 flex justify-between">
        <Button
          type="button"
          size="sm"
          disabled={step === 0}
          onClick={() => setStep((s) => s - 1)}
        >
          Previous
        </Button>
        {isLast ? (
          <Button type="button" size="sm" variant="primary" onClick={onDismiss}>
            Done
          </Button>
        ) : (
          <Button
            type="button"
            size="sm"
            variant="primary"
            onClick={() => setStep((s) => s + 1)}
          >
            Next
          </Button>
        )}
      </div>
    </Card>
  );
}
