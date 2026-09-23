import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router'
import { completeSso } from '@sdlc/ui/sso'
import { Button } from '@sdlc/ui/ui'
import { ssoConfig, useAuth } from '@/shared/auth/auth-provider'

let pending: ReturnType<typeof completeSso> | null = null
function completion() {
  if (!pending) {
    pending = completeSso(ssoConfig)
    void pending.finally(() => { pending = null }).catch(() => undefined)
  }
  return pending
}

export function SsoCallbackPage() {
  const { acceptSso } = useAuth()
  const navigate = useNavigate()
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    let active = true
    void completion().then((session) => {
      if (!active) return
      acceptSso(session)
      navigate(session.returnTo, { replace: true })
    }).catch((caught) => {
      if (active) setError(caught instanceof Error ? caught.message : 'Не удалось завершить вход')
    })
    return () => { active = false }
  }, [acceptSso, navigate])
  return <main className="grid min-h-screen place-items-center bg-background p-4">
    {error ? <div className="space-y-4 text-center"><p role="alert">{error}</p><Button className="min-h-10 sm:min-h-10" onClick={() => navigate('/login', { replace: true })}>Повторить вход</Button></div>
      : <p role="status">Завершаем вход...</p>}
  </main>
}
