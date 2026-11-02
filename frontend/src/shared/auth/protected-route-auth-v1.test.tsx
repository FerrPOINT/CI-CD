import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { ProtectedRoute } from "./protected-route";
import { applicationPolicyRequired } from "./application-policy";

const state = vi.hoisted(() => ({
  retire: vi.fn(),
  token: "",
}));
vi.mock("./auth-provider", () => ({
  ssoConfig: { issuer: "https://auth.fixture.test" },
  useAuth: () => ({
    status: "authenticated",
    error: null,
    invalidate: state.retire,
    session: { access_token: state.token },
  }),
}));
vi.mock("./space-admission-boundary", () => ({
  SpaceAdmissionBoundary: ({ children }: { children: React.ReactNode }) => children,
}));
vi.mock("@/api/auth", () => ({ clearSession: vi.fn() }));
vi.mock("@pdlc/ui/platform", async (original) => ({
  ...await original<typeof import("@pdlc/ui/platform")>(),
  getPlatformProduct: () => undefined,
}));
vi.mock("react-router", () => ({
  Outlet: () => <p>admitted-content</p>,
  Navigate: () => <p>login-required</p>,
  useLocation: () => ({ pathname: "/repository-syncs", search: "", hash: "" }),
}));
beforeEach(() => {
  state.token = "header." + btoa(JSON.stringify({
    sub: "fixture-central-human",
    exp: Math.floor(Date.now() / 1000) + 300,
  })) + ".signature";
  state.retire.mockClear();
});
afterEach(() => {
  cleanup();
  vi.unstubAllEnvs();
  vi.unstubAllGlobals();
});

it("keeps Auth-v1 SSO admission without calling the absent application-access API", () => {
  vi.stubEnv("VITE_AUTH_APPLICATION_ACCESS_REQUIRED", undefined);
  const fetch = vi.fn();
  vi.stubGlobal("fetch", fetch);
  expect(applicationPolicyRequired()).toBe(false);
  render(<ProtectedRoute />);
  expect(screen.getByText("admitted-content")).toBeTruthy();
  expect(fetch).not.toHaveBeenCalled();
});

it("keeps an explicitly disabled deployment in the same legacy serving cohort", () => {
  vi.stubEnv("VITE_AUTH_APPLICATION_ACCESS_REQUIRED", "false");
  expect(applicationPolicyRequired()).toBe(false);
  render(<ProtectedRoute />);
  expect(screen.getByText("admitted-content")).toBeTruthy();
});

it("does not fall back to legacy admission after a required-policy endpoint failure", async () => {
  vi.stubEnv("VITE_AUTH_APPLICATION_ACCESS_REQUIRED", "true");
  const fetch = vi.fn().mockResolvedValue(new Response("", { status: 404 }));
  vi.stubGlobal("fetch", fetch);
  render(<ProtectedRoute />);
  await waitFor(() => expect(fetch).toHaveBeenCalled());
  await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
  expect(screen.queryByText("admitted-content")).toBeNull();
  expect(fetch.mock.calls[0]![0]).toBe("https://auth.fixture.test/auth/access");
});

it("rejects an invalid deployment policy instead of selecting a permissive default", () => {
  vi.stubEnv("VITE_AUTH_APPLICATION_ACCESS_REQUIRED", "not-a-policy");
  expect(() => applicationPolicyRequired()).toThrow("Invalid application access requirement");
});
