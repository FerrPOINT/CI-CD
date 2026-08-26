import * as React from 'react'
import { cn } from '@/shared/lib/utils'

const Label = React.forwardRef<HTMLLabelElement, React.LabelHTMLAttributes<HTMLLabelElement>>(
  ({ className, ...props }, ref) => (
    <label
      ref={ref}
      className={cn('text-sm font-medium leading-none text-text-secondary', className)}
      {...props}
    />
  ),
)
Label.displayName = 'Label'

export { Label }
