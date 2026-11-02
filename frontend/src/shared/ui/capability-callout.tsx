import { Badge, Notice, SectionHeader } from "@pdlc/ui/ui";

type CapabilityTone = "current" | "mvp" | "configuration" | "target";

// Capability maturity remains a Forge concept; its presentation uses Base semantic tones.
const tones = {
  current: "success",
  mvp: "info",
  configuration: "warning",
  target: "neutral",
} as const;

interface CapabilityCalloutProps {
  title: string;
  description: string;
  label: string;
  tone?: CapabilityTone;
  className?: string;
}

export function CapabilityCallout({
  title,
  description,
  label,
  tone = "current",
  className,
}: CapabilityCalloutProps) {
  return (
    <Notice
      tone={tones[tone]}
      className={className}
      title={
        <SectionHeader
          title={
            <span className="flex flex-wrap items-center gap-2">
              {title}
              <Badge>{label}</Badge>
            </span>
          }
        />
      }
    >
      {description}
    </Notice>
  );
}
