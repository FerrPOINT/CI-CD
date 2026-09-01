import { Info } from 'lucide-react'
import { cn } from '@/shared/lib/utils'

type CapabilityTone = 'current' | 'mvp' | 'configuration' | 'target'

const toneClasses: Record<CapabilityTone, string> = {
  current: 'border-emerald-500/30 bg-emerald-500/5 text-emerald-600 dark:text-emerald-400',
  mvp: 'border-sky-500/30 bg-sky-500/5 text-sky-600 dark:text-sky-400',
  configuration: 'border-amber-500/30 bg-amber-500/5 text-amber-600 dark:text-amber-400',
  target: 'border-border bg-surface text-text-muted',
}

interface CapabilityCalloutProps {
  title: string
  description: string
  label: string
  tone?: CapabilityTone
  className?: string
}

export function CapabilityCallout({
  title,
  description,
  label,
  tone = 'current',
  className,
}: CapabilityCalloutProps) {
  return (
    <section className={cn('rounded-lg border p-3', toneClasses[tone], className)}>
      <div className="flex items-start gap-2">
        <Info className="mt-0.5 h-4 w-4 shrink-0" />
        <div className="min-w-0 space-y-1">
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="text-sm font-semibold text-text-primary">{title}</h2>
            <span className="rounded-full border border-border bg-surface-raised px-2 py-0.5 text-xs font-medium text-text-primary">
              {label}
            </span>
          </div>
          <p className="text-sm text-text-muted">{description}</p>
        </div>
      </div>
    </section>
  )
}
