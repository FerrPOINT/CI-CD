import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { PullComments } from "./comments";
import { ApiError } from "@/api/client";

const { request, downloadPatch, savePatch } = vi.hoisted(() => ({ request: vi.fn(), downloadPatch: vi.fn(), savePatch: vi.fn() }));
vi.mock("@/api/client", async (original) => ({ ...await original<typeof import("@/api/client")>(), api: request, downloadPullPatch: downloadPatch, saveDownloadedArtifact: savePatch }));
const base = "a".repeat(40), head = "b".repeat(40);
function show() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(<QueryClientProvider client={client}><PullComments repositoryId="repo-id" number={3} repo="group/repo" /></QueryClientProvider>);
}
beforeEach(() => {
  request.mockReset();
  downloadPatch.mockReset(); savePatch.mockReset();
  request.mockImplementation(async (url: string, init?: RequestInit) => {
    if (init?.method === "POST") return { id: "new-comment" };
    if (url.endsWith("/review-context")) return { base_sha: base, head_sha: head, merge_base: base };
    return [];
  });
});

describe("PR comments", () => {
  it("saves a general finding against the exact reviewed source", async () => {
    show();
    fireEvent.change(screen.getByRole("textbox", { name: /Замечание/ }), { target: { value: "Missing permission check" } });
    await waitFor(() => expect(screen.getByRole("button", { name: "Добавить замечание" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Добавить замечание" }));
    await waitFor(() => expect(request.mock.calls.some(([, init]) => init?.method === "POST")).toBe(true));
    const input = JSON.parse(request.mock.calls.find(([, init]) => init?.method === "POST")![1].body);
    expect(input).toMatchObject({ base_sha: base, head_sha: head, body: "Missing permission check", anchor: null });
    expect(input.operation_id).toMatch(/^[0-9a-f-]{36}$/);
  });

  it("reuses the original operation and body after an unknown response", async () => {
    let posted = 0;
    const normal = request.getMockImplementation()!;
    request.mockImplementation(async (url: string, init?: RequestInit) => {
      if (init?.method === "POST" && posted++ === 0)
        throw new ApiError({ kind: "network", status: 0, message: "Connection lost" });
      return normal(url, init);
    });
    show();
    fireEvent.change(screen.getByRole("textbox", { name: /Замечание/ }), { target: { value: "Finding" } });
    await waitFor(() => expect(screen.getByRole("button", { name: "Добавить замечание" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Добавить замечание" }));
    fireEvent.click(await screen.findByRole("button", { name: "Повторить сохранение" }));
    await waitFor(() => expect(posted).toBe(2));
    const posts = request.mock.calls.filter(([, init]) => init?.method === "POST");
    expect(posts[0][1].body).toBe(posts[1][1].body);
  });

  it("retains a deleted-line anchor and marks a comment on an older revision", async () => {
    const normal = request.getMockImplementation()!;
    request.mockImplementation(async (url: string, init?: RequestInit) => {
      if (url.includes("/comments?")) return [{ id: "old", operation_id: "op", body: "Deleted check",
        severity: "P1", author: "Reviewer", created_at: "2026-11-08T10:00:00Z",
        base_sha: base, head_sha: "c".repeat(40), anchor: { path: "src/auth.rs", side: "old", line: 17 } }];
      return normal(url, init);
    });
    show();
    expect(await screen.findByText("Предыдущая версия")).toBeVisible();
    expect(screen.getByRole("link", { name: "src/auth.rs:17 · до изменения" })).toHaveAttribute("href", expect.stringContaining("c".repeat(40)));
  });
  it("downloads the complete patch for the pinned source through the protected client", async () => {
    const patch = { blob: new Blob(["full diff"]), filename: "review.patch" };
    downloadPatch.mockResolvedValue(patch);
    show();
    await waitFor(() => expect(screen.getByRole("button", { name: "Скачать полный diff" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Скачать полный diff" }));
    await waitFor(() => expect(savePatch).toHaveBeenCalledWith(patch));
    expect(downloadPatch).toHaveBeenCalledWith("repo-id", 3, base, head);
  });

  it("reads every page of CI for the pinned SHA without executing a check", async () => {
    const normal = request.getMockImplementation()!;
    request.mockImplementation(async (url: string, init?: RequestInit) => {
      if (url.includes("/checks?")) return url.includes("offset=100")
        ? [{ pipeline_id: "last-check", commit_sha: head, status: "failed" }]
        : Array.from({ length: 100 }, (_, i) => ({ pipeline_id: `check-${i}`, commit_sha: head, status: "success" }));
      return normal(url, init);
    });
    show();
    expect(await screen.findByRole("link", { name: "Проверка last-che" })).toHaveAttribute("href", "/pipelines/last-check");
    expect(request.mock.calls.filter(([url]) => url.includes("/checks?")).every(([url]) => url.includes(`head_sha=${head}`))).toBe(true);
    expect(request.mock.calls.every(([, init]) => !init?.method || init.method === "GET")).toBe(true);
  });

});
