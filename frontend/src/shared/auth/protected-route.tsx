import { SpaceAdmissionBoundary } from "./space-admission-boundary";
import { LoadingState, Button } from "@pdlc/ui/ui";
// K2: layout route guard driven by AuthProvider.

import { Navigate, Outlet, useLocation } from "react-router";
import { useAuth, ssoConfig } from "./auth-provider";
import { clearSession } from "@/api/auth";
import {
  ApplicationAccessProvider,
  RequireApplicationAccess,
} from "@pdlc/ui/auth";
import { getPlatformProduct } from "@pdlc/ui/platform";

export function ProtectedRoute() {
  const { status, error, invalidate, session } = useAuth();
  const location = useLocation();

  if (status === "checking") {
    return <LoadingState />;
  }
  if (status === "authenticated") {
    return (
      <ApplicationAccessProvider
        issuer={ssoConfig.issuer}
        token={session?.access_token ?? null}
        onSessionExpired={() => {
          clearSession();
          invalidate();
          getPlatformProduct()?.recheckAuthorization();
        }}
      >
        <RequireApplicationAccess application="ci-cd">
          <SpaceAdmissionBoundary>
            <Outlet />
          </SpaceAdmissionBoundary>
        </RequireApplicationAccess>
      </ApplicationAccessProvider>
    );
  }
  if (status === "unavailable")
    return (
      <section role="alert">
        <p>{error}</p>
        <Button variant="ghost" onClick={invalidate}>
          Повторить
        </Button>
      </section>
    );
  if (getPlatformProduct()) {
    getPlatformProduct()?.requireLogin(
      location.pathname + location.search + location.hash,
    );
    return null;
  }
  return (
    <Navigate
      to="/login"
      replace
      state={{ from: location.pathname + location.search }}
    />
  );
}
