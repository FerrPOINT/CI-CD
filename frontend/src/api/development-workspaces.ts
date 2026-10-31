import { api } from "./client";
import type { components } from "./schema";
import type { NamespaceLocation } from "@pdlc/ui/lib";

export type DevelopmentWorkspace =
  components["schemas"]["DevelopmentWorkspace"];
export type WorkspaceDetail =
  components["schemas"]["DevelopmentWorkspaceDetail"];
export type WorkspaceOperation =
  components["schemas"]["DevelopmentWorkspaceOperation"];
export type WorkspaceCommand = components["schemas"]["WorkspaceAction"];
export type OperationRequest = components["schemas"]["OperationRequest"];
export type RegisterWorkspace = components["schemas"]["RegisterWorkspace"];
const path = (id: string) =>
  `/development-workspaces/${encodeURIComponent(id)}`;
export const developmentWorkspaceKeys = {
  configurations: (
    ref: NamespaceLocation | null,
    search: string,
    offset: number,
  ) =>
    [
      "development-workspace-configurations",
      ref?.registry_instance_id,
      ref?.namespace_id,
      search,
      offset,
    ] as const,
  configuration: (id: string) =>
    ["development-workspace-configuration", id] as const,
  repository: (id: string) => ["development-workspace-repository", id] as const,
  list: (project: string, offset: number) =>
    ["development-workspaces", project, offset] as const,
  detail: (id: string) => ["development-workspace", id] as const,
  targets: (project: string) =>
    ["development-workspace-targets", project] as const,
  operations: (id: string) => ["development-workspace-operations", id] as const,
};
export const developmentWorkspaces = {
  configurations: (
    ref: NamespaceLocation,
    search: string,
    offset: number,
    signal?: AbortSignal,
  ) =>
    api<components["schemas"]["ConfigPage"]>(
      `/delivery-configurations?managed=true&registry_instance_id=${encodeURIComponent(ref.registry_instance_id)}&namespace_id=${encodeURIComponent(ref.namespace_id)}&limit=20&offset=${offset}&search=${encodeURIComponent(search)}`,
      { signal },
    ),
  configuration: (id: string, signal?: AbortSignal) =>
    api<components["schemas"]["DeliveryConfig"]>(
      `/delivery-configurations/${encodeURIComponent(id)}`,
      { signal },
    ),
  repository: (id: string, signal?: AbortSignal) =>
    api<components["schemas"]["CatalogRepository"]>(
      `/catalog/repositories/${encodeURIComponent(id)}`,
      { signal },
    ),
  list: (project: string, offset: number, signal?: AbortSignal) =>
    api<components["schemas"]["DevelopmentWorkspacePage"]>(
      `/projects/${encodeURIComponent(project)}/development-workspaces?limit=20&offset=${offset}`,
      { signal },
    ),
  targets: (project: string, signal?: AbortSignal) =>
    api<components["schemas"]["DevelopmentWorkspaceTargets"]>(
      `/projects/${encodeURIComponent(project)}/development-workspace-profiles`,
      { signal },
    ),
  detail: (id: string, signal?: AbortSignal) =>
    api<WorkspaceDetail>(path(id), { signal }),
  register: (project: string, payload: RegisterWorkspace) =>
    api<WorkspaceOperation>(
      `/projects/${encodeURIComponent(project)}/development-workspaces`,
      { method: "POST", body: JSON.stringify(payload) },
    ),
  submit: (id: string, payload: OperationRequest) =>
    api<WorkspaceOperation>(`${path(id)}/operations`, {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  operation: (id: string, operation: string, signal?: AbortSignal) =>
    api<WorkspaceOperation>(
      `${path(id)}/operations/${encodeURIComponent(operation)}`,
      { signal },
    ),
  operations: (id: string, signal?: AbortSignal) =>
    api<WorkspaceOperation[]>(`${path(id)}/operations?limit=20`, { signal }),
  reconcile: (id: string, operation: string) =>
    api<WorkspaceOperation>(
      `${path(id)}/operations/${encodeURIComponent(operation)}/reconcile`,
      { method: "POST" },
    ),
};
