import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@sdlc/ui/ui'
import { Card, CardContent, CardHeader, CardTitle } from '@sdlc/ui/ui'
import { Input } from '@sdlc/ui/ui'
import { Label } from '@sdlc/ui/ui'
import { login } from '@/api/auth'

export function LoginPage() {
  const { t } = useTranslation()
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  async function onSubmit(event: React.FormEvent) {
    event.preventDefault()
    setError(null)
    setBusy(true)
    try {
      await login(username.trim(), password)
      window.location.href = '/'
    } catch {
      setError(t('login.invalidCredentials'))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-background p-4">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle>{t('login.title')}</CardTitle>
          <p className="text-sm text-muted-foreground">{t('login.description')}</p>
        </CardHeader>
        <CardContent>
          <form onSubmit={onSubmit} className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="username">{t('login.username')}</Label>
              <Input
                id="username"
                autoComplete="username"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                required
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="password">{t('login.password')}</Label>
              <Input
                id="password"
                type="password"
                autoComplete="current-password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                required
              />
            </div>
            {error && (
              <p role="alert" className="text-sm text-destructive">
                {error}
              </p>
            )}
            <Button type="submit" className="w-full" disabled={busy}>
              {busy ? t('login.signingIn') : t('login.submit')}
            </Button>
          </form>
        </CardContent>
      </Card>
    </div>
  )
}
