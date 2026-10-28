import type { ReactNode } from "react";
import { ApplicationAccessProvider } from "@pdlc/ui/auth";
import { ssoConfig, useAuth } from "./auth-provider";
import { clearSession } from "@/api/auth";

export function CurrentApplicationAccess({
  children,
}: {
  children: ReactNode;
}) {
  const { session, invalidate } = useAuth();
  return (
    <ApplicationAccessProvider
      issuer={ssoConfig.issuer}
      token={session?.access_token ?? null}
      onSessionExpired={() => {
        clearSession();
        invalidate();
      }}
    >
      {children}
    </ApplicationAccessProvider>
  );
}
