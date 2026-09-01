// Deterministic user avatars: initials + stable color derived from the name.
// No external service / upload needed; same name always renders the same look
// so it is safe across re-renders and instances.

import { cn } from '@/shared/lib/utils'

const PALETTE = [
  'bg-violet-700',
  'bg-blue-700',
  'bg-emerald-700',
  'bg-amber-700',
  'bg-rose-700',
  'bg-cyan-700',
  'bg-indigo-700',
  'bg-teal-700',
  'bg-orange-700',
  'bg-pink-700',
  'bg-lime-800',
  'bg-sky-700',
]

// Tailwind must see full class names statically; map size -> explicit classes.
const SIZES: Record<string, string> = {
  xs: 'h-6 w-6 text-[10px]',
  sm: 'h-7 w-7 text-xs',
  md: 'h-8 w-8 text-xs',
  lg: 'h-10 w-10 text-sm',
  xl: 'h-12 w-12 text-base',
}

function hashName(name: string): number {
  let h = 0
  for (let i = 0; i < name.length; i++) {
    h = (h * 31 + name.charCodeAt(i)) >>> 0
  }
  return h
}

function initialsOf(name: string): string {
  const parts = name.trim().split(/[\s._-]+/).filter(Boolean)
  if (parts.length === 0) return '?'
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase()
  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase()
}

interface UserAvatarProps {
  name: string | null | undefined
  size?: keyof typeof SIZES
  /** Show the name next to the avatar. */
  withName?: boolean
  className?: string
}

export function UserAvatar({ name, size = 'md', withName = false, className }: UserAvatarProps) {
  const label = (name ?? '').trim() || 'unknown'
  const color = PALETTE[hashName(label) % PALETTE.length]
  return (
    <span className={cn('inline-flex items-center gap-2', className)}>
      <span
        role="img"
        aria-label={label}
        className={cn(
          'inline-flex select-none items-center justify-center rounded-full font-semibold text-white',
          SIZES[size],
          color,
        )}
      >
        {initialsOf(label)}
      </span>
      {withName && <span className="text-sm font-medium text-text-primary">{label}</span>}
    </span>
  )
}
