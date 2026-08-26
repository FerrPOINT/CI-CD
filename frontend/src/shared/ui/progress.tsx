import * as React from 'react'
import { cn } from '@/shared/lib/utils'

interface ProgressProps extends React.HTMLAttributes<HTMLDivElement> {
  value: number
  max?: number
  variant?: 'default' | 'danger'
}

const Progress = React.forwardRef<HTMLDivElement, ProgressProps>(
  ({ className, value, max = 100, variant = 'default', ...props }, ref) => {
    const percent = Math.min(100, Math.max(0, (value / max) * 100))
    const barColor = variant === 'danger' ? 'bg-danger' : 'bg-accent'
    return (
      <div
        ref={ref}
        className={cn(
          'relative h-2 w-full overflow-hidden rounded-full bg-surface-raised',
          className,
        )}
        {...props}
      >
        <div className={cn('h-full transition-all', barColor)} style={{ width: `${percent}%` }} />
      </div>
    )
  },
)
Progress.displayName = 'Progress'

export { Progress }
