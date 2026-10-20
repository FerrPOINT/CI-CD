import { useEffect, useState } from 'react'
import {useNamespaceNavigate as useNavigate, SsoCallbackView} from '@pdlc/ui/ui'

import { completeSso } from '@pdlc/ui/sso'

import { ssoConfig, useAuth } from '@/shared/auth/auth-provider'
import { finishPlatformLogin } from '@pdlc/ui/platform'

let pending: ReturnType<typeof completeSso> | null = null
function completion() {
  if (!pending) {
    pending = completeSso(ssoConfig)
    void pending
      .finally(() => {
        pending = null
      })
      .catch(() => undefined)
  }
  return pending
}

export function SsoCallbackPage() {
  const { acceptSso } = useAuth()
  const navigate = useNavigate()
  const [failed, setFailed] = useState(false)
  useEffect(() => {
    let active = true
    void completion().then((session) => {
      if (!active) return
      acceptSso(session)
      if (!finishPlatformLogin(import.meta.env.VITE_PLATFORM_ORIGIN)) navigate(session.returnTo, { replace: true })
    }).catch(() => {
      if (active) setFailed(true)
    })
    return () => { active = false }
  }, [acceptSso, navigate])
  return <SsoCallbackView failed={failed} onRetry={() => navigate('/login', { replace: true })} />
}
