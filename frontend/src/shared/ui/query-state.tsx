import {
  QueryState as BaseQueryState,
  type QueryStateProps,
} from "@pdlc/ui/ui";
import { ApiError } from "@/api/client";
import { ForbiddenPage } from "@/pages/forbidden";

/** Forge retains its project-access explanation; all generic query UI belongs to Base. */
export function QueryState<T>(props: QueryStateProps<T>) {
  if (
    !props.isLoading &&
    props.error &&
    props.error instanceof ApiError &&
    props.error.status === 403
  ) {
    return <ForbiddenPage />;
  }
  return <BaseQueryState {...props} />;
}
