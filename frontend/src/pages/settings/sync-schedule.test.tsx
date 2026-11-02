import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { repositorySync, type SyncSchedule } from "@/api/repository-sync";
import { ApiError } from "@/api/client";
import { SyncSchedulePanel } from "./sync-schedule";
import "@/shared/i18n/config";

vi.mock("@/shared/auth/auth-provider", () => ({ useAuth: () => ({ session: { subject: "schedule-operator" } }) }));
const initial: SyncSchedule = { version: 0, settings: { interval_seconds: 3600, on_change: false }, worker_enabled: true, concurrency: 1 };
const routers = new Set<ReturnType<typeof createMemoryRouter>>();
const clients = new Set<QueryClient>();
function mount() {
  const cache = new QueryClient({ defaultOptions: { queries: { retry: false } } }); clients.add(cache);
  const router = createMemoryRouter([{ path: "/", element: <SyncSchedulePanel /> }]); routers.add(router);
  render(<QueryClientProvider client={cache}><RouterProvider router={router} /></QueryClientProvider>);
}
afterEach(() => { cleanup(); routers.forEach((r) => r.dispose()); clients.forEach((c) => c.clear()); routers.clear(); clients.clear(); vi.restoreAllMocks(); });
const input = () => screen.getByLabelText("Период проверки, секунд");

describe("global Forge synchronization schedule", () => {
  it("loads the effective policy and saves only editable values with its revision", async () => {
    vi.spyOn(repositorySync,"schedule").mockResolvedValue(initial);
    const save=vi.spyOn(repositorySync,"saveSchedule").mockResolvedValue({ ...initial,version:1,settings:{interval_seconds:7200,on_change:true} });
    mount(); await screen.findByDisplayValue("3600");
    expect(screen.getByText(/Одновременно выполняется синхронизаций: 1/)).toBeInTheDocument();
    expect(screen.getByRole("button",{name:"Сохранить расписание"})).toBeDisabled();
    fireEvent.change(input(),{target:{value:"7200"}});
    fireEvent.click(screen.getByLabelText("Запускать также после изменений в Forge"));
    fireEvent.click(screen.getByRole("button",{name:"Сохранить расписание"}));
    await screen.findByText("Расписание сохранено и применяется без перезапуска.");
    expect(save.mock.calls[0][0]).toEqual({expected_version:0,settings:{interval_seconds:7200,on_change:true}});
    expect(save.mock.calls[0][1]).toMatch(/^[0-9a-f-]{36}$/);
    expect(screen.getByRole("button",{name:"Сохранить расписание"})).toBeDisabled();
  });
  it("replays an uncertain save using the identical body and operation, keeping fields locked", async () => {
    vi.spyOn(repositorySync,"schedule").mockResolvedValue(initial);
    const save=vi.spyOn(repositorySync,"saveSchedule")
      .mockRejectedValueOnce(new ApiError({kind:"network",message:"transport lost"}))
      .mockResolvedValueOnce({...initial,version:1,settings:{interval_seconds:1800,on_change:false}});
    mount();await screen.findByDisplayValue("3600");fireEvent.change(input(),{target:{value:"1800"}});
    fireEvent.click(screen.getByRole("button",{name:"Сохранить расписание"}));
    const retry=await screen.findByRole("button",{name:"Проверить сохранение"});
    expect(input()).toBeDisabled();expect(save).toHaveBeenCalledTimes(1);
    fireEvent.click(retry);await screen.findByText("Расписание сохранено и применяется без перезапуска.");
    expect(save.mock.calls[1]).toEqual(save.mock.calls[0]);expect(input()).toBeEnabled();
  });
  it("does not retry a revision conflict and keeps the draft until explicit refresh", async () => {
    const load=vi.spyOn(repositorySync,"schedule").mockResolvedValueOnce(initial)
      .mockResolvedValueOnce({...initial,version:2,settings:{interval_seconds:900,on_change:true}});
    const save=vi.spyOn(repositorySync,"saveSchedule").mockRejectedValue(new ApiError({kind:"api",status:409,message:"sync_schedule_version_conflict"}));
    mount();await screen.findByDisplayValue("3600");fireEvent.change(input(),{target:{value:"7200"}});
    fireEvent.click(screen.getByRole("button",{name:"Сохранить расписание"}));
    await screen.findByText("Расписание изменилось. Загрузите актуальные настройки перед сохранением.");
    expect(save).toHaveBeenCalledTimes(1);expect(input()).toHaveValue(7200);
    fireEvent.click(screen.getByRole("button",{name:"Отменить правки и обновить"}));
    await screen.findByDisplayValue("900");expect(load).toHaveBeenCalledTimes(2);
    expect(screen.getByRole("button",{name:"Сохранить расписание"})).toBeDisabled();
  });
  it("keeps a confirmed newer policy visible after replay of an older request", async () => {
    vi.spyOn(repositorySync,"schedule").mockResolvedValue(initial);
    vi.spyOn(repositorySync,"saveSchedule").mockResolvedValue({...initial,version:2,settings:{interval_seconds:900,on_change:false}});
    mount();await screen.findByDisplayValue("3600");fireEvent.change(input(),{target:{value:"7200"}});
    fireEvent.click(screen.getByRole("button",{name:"Сохранить расписание"}));
    await screen.findByText("Запрос подтверждён. Показана более новая версия расписания.");expect(input()).toHaveValue(900);
  });
  it("rejects invalid period values before submitting", async () => {
    vi.spyOn(repositorySync,"schedule").mockResolvedValue(initial);const save=vi.spyOn(repositorySync,"saveSchedule");
    mount();await screen.findByDisplayValue("3600");
    for(const value of ["","0","-1","1.5","86401"]){fireEvent.change(input(),{target:{value}});expect(screen.getByRole("button",{name:"Сохранить расписание"})).toBeDisabled();}
    expect(save).not.toHaveBeenCalled();
  });
  it("shows load/permission failure without editable controls", async () => {
    vi.spyOn(repositorySync,"schedule").mockRejectedValue(new ApiError({kind:"api",status:403,message:"forbidden"}));
    mount();await screen.findByText("Не удалось загрузить расписание. Нужен доступ администратора Forge.");
    expect(screen.queryByLabelText("Период проверки, секунд")).not.toBeInTheDocument();
  });
  it("shows pending state and disables editing while a save is in flight", async () => {
    vi.spyOn(repositorySync,"schedule").mockResolvedValue(initial);
    let complete!: (value:SyncSchedule)=>void;
    vi.spyOn(repositorySync,"saveSchedule").mockImplementation(()=>new Promise((resolve)=>{complete=resolve}));
    mount();await screen.findByDisplayValue("3600");fireEvent.change(input(),{target:{value:"600"}});
    fireEvent.click(screen.getByRole("button",{name:"Сохранить расписание"}));
    await waitFor(()=>expect(input()).toBeDisabled());
    await act(async()=>complete({...initial,version:1,settings:{interval_seconds:600,on_change:false}}));
    await screen.findByText("Расписание сохранено и применяется без перезапуска.");expect(input()).toBeEnabled();
  });
});
