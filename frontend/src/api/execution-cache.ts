import type { QueryClient } from '@tanstack/react-query'

/** Legacy configuration actions and workspace views observe the same execution. */
export function invalidateExecutionViews(client: QueryClient) {
  void client.invalidateQueries({ queryKey: ['executions'] })
  void client.invalidateQueries({ queryKey: ['workspace-summary'] })
  void client.invalidateQueries({ queryKey: ['workspace-projects'] })
  void client.invalidateQueries({ queryKey: ['workspace-catalog'] })
}
