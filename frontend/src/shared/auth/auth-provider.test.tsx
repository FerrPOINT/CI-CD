import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { MemoryRouter, Routes, Route } from "react-router";
import { AuthProvider, useAuth, ssoConfig } from "./auth-provider";
import { ProtectedRoute } from "./protected-route";

const authMod = vi.hoisted(() => ({
  currentSession: vi.fn<() => unknown>(() => null),
  acceptSso:
    vi.fn<(token: string, expiresAt: number, username: string) => unknown>(),
  refresh: vi.fn(),
  authRequired: vi.fn(),
  login: vi.fn(),
  logout: vi.fn(),
  clearSession: vi.fn(),
  rotateSessionAccess: vi.fn(),
}));

vi.mock("@/api/auth", () => authMod);
// Auth restoration is tested independently of the owner-window wire.
vi.mock("./space-admission-boundary", () => ({
  SpaceAdmissionBoundary: ({ children }: { children: React.ReactNode }) =>
    children,
}));

const sso = vi.hoisted(() => ({ endSso: vi.fn(), restoreSso: vi.fn() }));
vi.mock("@pdlc/ui/sso", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pdlc/ui/sso")>()),
  ...sso,
}));

function Probe() {
  const { status, logout } = useAuth();
  return (
    <>
      <div data-testid="probe">{status}</div>
      <button onClick={() => void logout()}>Logout</button>
    </>
  );
}

describe("AuthProvider", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    authMod.currentSession.mockReturnValue(null);
    authMod.clearSession.mockImplementation(() =>
      authMod.currentSession.mockReturnValue(null),
    );
    authMod.rotateSessionAccess.mockImplementation(() => {
      const current = authMod.currentSession();
      const next = current ? { ...(current as Record<string, unknown>) } : null;
      authMod.currentSession.mockReturnValue(next);
      return next;
    });
    authMod.acceptSso.mockImplementation((token, expiresAt, username) => {
      const next = {
        access_token: token,
        expires_at: expiresAt / 1000,
        username,
      };
      authMod.currentSession.mockReturnValue(next);
      return next;
    });
    sso.restoreSso.mockRejectedValue(
      Object.assign(new Error("Login required"), {
        name: "SsoLoginRequiredError",
      }),
    );
  });

  it("keeps an in-memory central session", async () => {
    authMod.currentSession.mockReturnValue({
      access_token: "x",
      expires_at: 1,
    });
    render(
      <AuthProvider>
        <Probe />
      </AuthProvider>,
    );
    await waitFor(() =>
      expect(screen.getByTestId("probe").textContent).toBe("authenticated"),
    );
  });

  it("requires central login after a hard reload", async () => {
    render(
      <AuthProvider>
        <Probe />
      </AuthProvider>,
    );
    await waitFor(() =>
      expect(screen.getByTestId("probe").textContent).toBe("anonymous"),
    );
  });

  it("keeps checking while silent SSO is unresolved and accepts its verified result", async () => {
    let complete!: (value: unknown) => void;
    sso.restoreSso.mockReturnValueOnce(
      new Promise((resolve) => {
        complete = resolve;
      }),
    );
    render(
      <AuthProvider>
        <Probe />
      </AuthProvider>,
    );
    expect(screen.getByTestId("probe")).toHaveTextContent("checking");
    const session = {
      accessToken: "fresh-fixture",
      expiresAt: Date.now() + 60000,
      subject: "fixture",
      email: "fixture@example.test",
      name: "Fixture",
      returnTo: "/",
    };
    complete(session);
    await waitFor(() =>
      expect(screen.getByTestId("probe")).toHaveTextContent("authenticated"),
    );
    expect(authMod.acceptSso).toHaveBeenCalledWith(
      session.accessToken,
      session.expiresAt,
      session.name,
      session.subject,
    );
  });

  it("redirects to central logout without rendering the local login route", async () => {
    const session = { access_token: "x", expires_at: 1 };
    authMod.currentSession.mockReturnValue(session);
    authMod.logout.mockImplementation(async () => {
      authMod.currentSession.mockReturnValue(null);
    });
    let sessionAtNavigation: unknown;
    sso.endSso.mockImplementationOnce(() => {
      sessionAtNavigation = authMod.currentSession();
    });
    render(
      <AuthProvider>
        <Probe />
      </AuthProvider>,
    );
    await waitFor(() =>
      expect(screen.getByTestId("probe").textContent).toBe("authenticated"),
    );

    const beforeNavigation = authMod.currentSession();
    expect(beforeNavigation).toEqual(session);
    fireEvent.click(screen.getByRole("button", { name: "Logout" }));

    await waitFor(() => expect(sso.endSso).toHaveBeenCalledOnce());
    expect(authMod.logout).not.toHaveBeenCalled();
    expect(sessionAtNavigation).toBe(beforeNavigation);
    expect(authMod.currentSession()).toBe(beforeNavigation);
    expect(screen.getByTestId("probe").textContent).toBe("authenticated");
  });

  it("does not enter open mode when central auth is unavailable", async () => {
    sso.restoreSso.mockRejectedValueOnce(new Error("Auth unavailable"));
    render(
      <AuthProvider>
        <Probe />
      </AuthProvider>,
    );
    await waitFor(() =>
      expect(screen.getByTestId("probe").textContent).toBe("unavailable"),
    );
  });
});

afterEach(() => vi.unstubAllGlobals());
const subject = "123e4567-e89b-12d3-a456-426614174000";
function liveSession() {
  const payload = btoa(
    JSON.stringify({ sub: subject, exp: Math.floor(Date.now() / 1000) + 300 }),
  )
    .replace(/=/g, "")
    .replace(/\+/g, "-")
    .replace(/\//g, "_");
  return {
    access_token: "header." + payload + ".signature",
    expires_at: Math.floor(Date.now() / 1000) + 300,
  };
}
function proof(applications = ["ci-cd"], revision = 1) {
  const now = Date.now();
  return {
    schema_version: 1,
    issuer: ssoConfig.issuer,
    subject,
    role: "user",
    applications,
    revision,
    enforced: true,
    observed_at: new Date(now).toISOString(),
    expires_at: new Date(now + 30000).toISOString(),
  };
}
function protectedView() {
  return render(
    <MemoryRouter initialEntries={["/secret"]}>
      <AuthProvider>
        <Routes>
          <Route element={<ProtectedRoute />}>
            <Route path="/secret" element={<div>secret-content</div>} />
          </Route>
          <Route path="/login" element={<div>login-page</div>} />
        </Routes>
      </AuthProvider>
    </MemoryRouter>,
  );
}
describe("ProtectedRoute", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    authMod.currentSession.mockReturnValue(null);
  });

  it("renders children with a central session and current assigned application", async () => {
    authMod.currentSession.mockReturnValue(liveSession());
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => Response.json(proof())),
    );
    render(
      <MemoryRouter initialEntries={["/secret"]}>
        <AuthProvider>
          <Routes>
            <Route element={<ProtectedRoute />}>
              <Route path="/secret" element={<div>secret-content</div>} />
            </Route>
          </Routes>
        </AuthProvider>
      </MemoryRouter>,
    );
    await waitFor(() =>
      expect(screen.getByText("secret-content")).toBeInTheDocument(),
    );
  });

  it("does not loop through SSO when the application is absent", async () => {
    authMod.currentSession.mockReturnValue(liveSession());
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => Response.json(proof([]))),
    );
    protectedView();
    expect(await screen.findByText("Нет доступа")).toBeInTheDocument();
    expect(screen.queryByText("secret-content")).not.toBeInTheDocument();
    expect(screen.queryByText("login-page")).not.toBeInTheDocument();
  });
  it("retires the same-token work screen after owner revocation", async () => {
    authMod.currentSession.mockReturnValue(liveSession());
    let current = proof();
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => Response.json(current)),
    );
    protectedView();
    await screen.findByText("secret-content");
    current = proof([], 2);
    act(() => window.dispatchEvent(new Event("focus")));
    await waitFor(() =>
      expect(screen.queryByText("secret-content")).not.toBeInTheDocument(),
    );
    expect(screen.getByText("Нет доступа")).toBeInTheDocument();
  });
  it("redirects to /login when anonymous", async () => {
    render(
      <MemoryRouter initialEntries={["/secret"]}>
        <AuthProvider>
          <Routes>
            <Route element={<ProtectedRoute />}>
              <Route path="/secret" element={<div>secret-content</div>} />
            </Route>
            <Route path="/login" element={<div>login-page</div>} />
          </Routes>
        </AuthProvider>
      </MemoryRouter>,
    );
    await waitFor(() =>
      expect(screen.getByText("login-page")).toBeInTheDocument(),
    );
    expect(screen.queryByText("secret-content")).not.toBeInTheDocument();
  });
});
