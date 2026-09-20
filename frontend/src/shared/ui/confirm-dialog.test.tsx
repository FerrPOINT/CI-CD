import { describe, expect, it, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

import { ConfirmDialog } from "./confirm-dialog";

describe("ConfirmDialog", () => {
  it("is hidden until open", () => {
    render(
      <ConfirmDialog
        open={false}
        onConfirm={() => {}}
        onCancel={() => {}}
        title="t"
        description="d"
      />,
    );
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("shows title and triggers confirm", () => {
    const onConfirm = vi.fn();
    render(
      <ConfirmDialog
        open
        onConfirm={onConfirm}
        onCancel={() => {}}
        title="Delete runner"
        confirmLabel="Delete"
        description="Are you sure?"
      />,
    );

    expect(screen.getByRole("alertdialog")).toBeDefined();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(onConfirm).toHaveBeenCalledOnce();
  });

  it("cancel does not confirm", () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(
      <ConfirmDialog
        open
        onConfirm={onConfirm}
        onCancel={onCancel}
        title="t"
        description="d"
        confirmLabel="Delete"
        cancelLabel="Cancel"
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onConfirm).not.toHaveBeenCalled();
    expect(onCancel).toHaveBeenCalled();
  });

  it("can remain open until an asynchronous confirmation succeeds", () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(
      <ConfirmDialog
        open
        onConfirm={onConfirm}
        onCancel={onCancel}
        title="Delete project"
        closeOnConfirm={false}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "common.delete" }));
    expect(onConfirm).toHaveBeenCalledOnce();
    expect(onCancel).not.toHaveBeenCalled();
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();
  });

  it("shows an actionable error inside an open confirmation", () => {
    const onConfirm = vi.fn();
    render(
      <ConfirmDialog
        open
        onConfirm={onConfirm}
        onCancel={() => {}}
        title="Delete secret"
        description="This cannot be undone"
        error="Deletion failed"
        closeOnConfirm={false}
      />,
    );
    expect(screen.getByRole("alertdialog")).toHaveTextContent("This cannot be undone");
    expect(screen.getByRole("alert")).toHaveTextContent("Deletion failed");
    fireEvent.click(screen.getByRole("button", { name: "common.delete" }));
    expect(onConfirm).toHaveBeenCalledOnce();
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();
  });

  it("keeps dialog actions touch-sized", () => {
    render(<ConfirmDialog open onConfirm={() => {}} onCancel={() => {}} title="Delete" />);
    expect(screen.getByRole("button", { name: "common.delete" })).toHaveClass("min-h-10", "text-danger-foreground");
    expect(screen.getByRole("button", { name: "common.cancel" })).toHaveClass("min-h-10");
  });
});
