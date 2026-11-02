import type { ReactNode } from "react";
import { RequireAdministrator, useApplicationAccess } from "@pdlc/ui/auth";

/** Explicit deployment cohort; a required-policy failure never selects Auth-v1. */
export function applicationPolicyRequired(): boolean {
  const value = import.meta.env.VITE_AUTH_APPLICATION_ACCESS_REQUIRED;
  if (value === undefined || value === "false") return false;
  if (value === "true") return true;
  throw new Error("Invalid application access requirement");
}

/** Legacy Forge exposed administration to admitted central humans. UI visibility
 * does not authorize an API operation; the server checks every request. */
export function useForgePermissions() {
  const access = useApplicationAccess();
  const required = applicationPolicyRequired();
  return {
    permits(applications: readonly string[], administrative = false): boolean {
      return required ? access?.permits(applications, administrative) === true : true;
    },
  };
}

export function RequireForgeAdministrator({ children }: { children: ReactNode }) {
  return applicationPolicyRequired()
    ? <RequireAdministrator>{children}</RequireAdministrator>
    : <>{children}</>;
}
