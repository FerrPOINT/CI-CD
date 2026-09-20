import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { GitMerge, RotateCcw, XCircle } from 'lucide-react'
import { toast } from 'sonner'
import { usePullRequestAction } from '@/api/hooks'
import type { PullRequest } from '@/api/types'
import { ConfirmDialog } from '@/shared/ui/confirm-dialog'
import { Button } from '@sdlc/ui/ui'

type ConfirmedAction = 'merge' | 'close'

export function PullRequestActions({ repo, pullRequest, compact = false }: {
  repo: string
  pullRequest: PullRequest
  compact?: boolean
}) {
  const { t } = useTranslation()
  const action = usePullRequestAction(repo)
  const [pendingAction, setPendingAction] = useState<ConfirmedAction | null>(null)

  function runAction(nextAction: ConfirmedAction | 'reopen') {
    action.mutate({ number: pullRequest.number, action: nextAction }, {
      onSuccess: () => {
        setPendingAction(null)
        toast.success(t(`pulls.action_${nextAction}`))
      },
      onError: (error) => toast.error(error.message),
    })
  }

  const buttonClass = compact ? 'min-h-10 min-w-10 sm:min-h-10 sm:min-w-10' : 'min-h-10 justify-start'

  return (
    <>
      <div className={compact ? 'flex shrink-0 items-center gap-1' : 'flex flex-col gap-2'}>
        {pullRequest.status === 'open' && (
          <>
            <Button type="button" size={compact ? 'icon' : 'default'} className={buttonClass}
              disabled={action.isPending} aria-label={compact ? t('pulls.merge') : undefined}
              title={compact ? t('pulls.merge') : undefined} onClick={() => setPendingAction('merge')}>
              <GitMerge className="h-4 w-4" aria-hidden />{!compact && t('pulls.merge')}
            </Button>
            <Button type="button" size={compact ? 'icon' : 'default'} variant="outline" className={buttonClass}
              disabled={action.isPending} aria-label={compact ? t('pulls.close') : undefined}
              title={compact ? t('pulls.close') : undefined} onClick={() => setPendingAction('close')}>
              <XCircle className="h-4 w-4" aria-hidden />{!compact && t('pulls.close')}
            </Button>
          </>
        )}
        {pullRequest.status === 'closed' && (
          <Button type="button" size={compact ? 'icon' : 'default'} variant="outline" className={buttonClass}
            disabled={action.isPending} aria-label={compact ? t('pulls.reopen') : undefined}
            title={compact ? t('pulls.reopen') : undefined} onClick={() => runAction('reopen')}>
            <RotateCcw className="h-4 w-4" aria-hidden />{!compact && t('pulls.reopen')}
          </Button>
        )}
      </div>
      <ConfirmDialog
        open={pendingAction !== null}
        title={pendingAction ? t(`pulls.confirm_${pendingAction}`) : ''}
        description={t('pulls.confirmDescription', {
          number: pullRequest.number,
          source: pullRequest.source_branch,
          target: pullRequest.target_branch,
        })}
        confirmLabel={pendingAction ? t(`pulls.${pendingAction}`) : ''}
        confirmVariant={pendingAction === 'merge' ? 'default' : 'destructive'}
        pending={action.isPending}
        closeOnConfirm={false}
        onCancel={() => setPendingAction(null)}
        onConfirm={() => { if (pendingAction) runAction(pendingAction) }}
      />
    </>
  )
}
