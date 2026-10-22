import { confirmApplicationAccess, permitsApplication } from "@pdlc/ui/auth";

/** Component fixtures use the public SDK rule; transport/expiry has dedicated integration tests. */
export function applicationAccessFixture(actor: {
  role: "user" | "admin";
  applications: string[];
}) {
  const issuer = "https://auth.example.test";
  const subject = "123e4567-e89b-12d3-a456-426614174000";
  const wall = Date.now();
  const monotonic = performance.now();
  const access = confirmApplicationAccess(
    {
      schema_version: 1,
      issuer,
      subject,
      role: actor.role,
      applications: actor.applications,
      revision: 1,
      enforced: true,
      observed_at: new Date(wall).toISOString(),
      expires_at: new Date(wall + 30000).toISOString(),
    },
    {
      issuer,
      subject,
      startedAt: monotonic,
      receivedAt: monotonic,
      wallAtReceipt: wall,
      tokenExpiresAt: wall + 300000,
    },
  );
  return {
    permits: (applications: readonly string[], administrative = false) =>
      permitsApplication(access, applications, administrative),
  };
}
