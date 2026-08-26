import { useState } from 'react'
import { useNavigate } from 'react-router'
import { GitBranch } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/shared/ui/button'
import { Input } from '@/shared/ui/input'
import { Label } from '@/shared/ui/label'

export function LoginPage() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')

  return (
    <div className="flex min-h-screen items-center justify-center bg-background px-4">
      <div className="w-full max-w-sm space-y-6">
        <div className="flex flex-col items-center gap-2">
          <GitBranch className="h-10 w-10 text-accent" />
          <h1 className="text-xl font-bold text-text-primary">{t('app.name')}</h1>
          <p className="text-sm text-text-muted">Self-hosted CI/CD control plane</p>
        </div>
        <form
          className="space-y-4"
          onSubmit={(e) => {
            e.preventDefault()
            navigate('/')
          }}
        >
          <div className="space-y-1.5">
            <Label htmlFor="email">Email</Label>
            <Input id="email" type="email" value={email} onChange={(e) => setEmail(e.target.value)} placeholder="admin@example.com" />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="password">Password</Label>
            <Input id="password" type="password" value={password} onChange={(e) => setPassword(e.target.value)} placeholder="••••••••" />
          </div>
          <Button type="submit" className="w-full">Sign in</Button>
        </form>
      </div>
    </div>
  )
}
