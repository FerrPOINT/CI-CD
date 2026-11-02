import { MemoryRouter } from "react-router";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SettingsPage } from "./index";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

afterEach(cleanup);

describe("SettingsPage", () => {
  it("keeps description groups directly under dl and retains the documented defaults", () => {
    render(
      <MemoryRouter>
        <SettingsPage />
      </MemoryRouter>,
    );
    for (const term of screen.getAllByRole("term")) {
      const group = term.parentElement!;
      if (group.tagName === "DIV") {
        expect(group.parentElement?.tagName).toBe("DL");
        expect(
          [...group.children].every((child) =>
            ["DT", "DD"].includes(child.tagName),
          ),
        ).toBe(true);
      } else {
        expect(group.tagName).toBe("DL");
      }
    }
    expect(screen.getByText(":22801")).toBeInTheDocument();
    expect(screen.getByText(":22802")).toBeInTheDocument();
    expect(screen.getByText("PostgreSQL 17")).toBeInTheDocument();
  });

  it("filters variables by key and description and can clear the search", () => {
    render(
      <MemoryRouter>
        <SettingsPage />
      </MemoryRouter>,
    );
    const search = screen.getByRole("searchbox", { name: "settings.search" });

    fireEvent.change(search, { target: { value: "CICD_RUNNER_TAGS" } });
    expect(screen.getByText("CICD_RUNNER_TAGS")).toBeInTheDocument();
    expect(screen.queryByText("CICD_DATABASE_URL")).not.toBeInTheDocument();

    fireEvent.change(search, { target: { value: "settings.env.databaseUrl" } });
    expect(screen.getByText("CICD_DATABASE_URL")).toBeInTheDocument();
    expect(screen.queryByText("CICD_RUNNER_TAGS")).not.toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "settings.clearSearch" }),
    );
    expect(screen.getByText("CICD_RUNNER_TAGS")).toBeInTheDocument();
  });

  it("shows an empty search result", () => {
    render(
      <MemoryRouter>
        <SettingsPage />
      </MemoryRouter>,
    );
    fireEvent.change(
      screen.getByRole("searchbox", { name: "settings.search" }),
      { target: { value: "no-such-variable" } },
    );
    expect(screen.getByRole("status")).toHaveTextContent("settings.noMatches");
  });
});
