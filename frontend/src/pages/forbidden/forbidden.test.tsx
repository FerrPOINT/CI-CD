import { render, screen, fireEvent } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { MemoryRouter, Route, Routes, useLocation } from "react-router";
import { ForbiddenPage } from "./index";

function Destination() {
  const location = useLocation();
  return (
    <output aria-label="Return URL">
      {location.pathname + location.search}
    </output>
  );
}

describe("Forge forbidden resource", () => {
  it("uses the common terminal surface while preserving its explanation, h1 and Namespace return", () => {
    const query =
      "?registry_instance_id=506a8476-3868-4581-af70-6d22c93ced2f&namespace_id=0ee31fca-2533-44bf-b54e-a7759fe80b14";
    render(
      <MemoryRouter initialEntries={["/forbidden" + query]}>
        <Routes>
          <Route path="/forbidden" element={<ForbiddenPage />} />
          <Route path="/" element={<Destination />} />
        </Routes>
      </MemoryRouter>,
    );
    expect(
      screen
        .getByRole("heading", { level: 1, name: "Доступ запрещён" })
        .closest('[data-page-state="permission-denied"]'),
    ).not.toBeNull();
    expect(
      screen.getByText(
        "У вашей роли нет прав на это действие или раздел. Обратитесь к администратору проекта.",
      ),
    ).toHaveClass("base-state-description");
    const back = screen.getByRole("link", { name: "На главную" });
    expect(back).toHaveClass("base-button");
    expect(back).toHaveAttribute("href", "/" + query);
    fireEvent.click(back);
    expect(
      screen.getByRole("status", { name: "Return URL" }),
    ).toHaveTextContent("/" + query);
  });
});
