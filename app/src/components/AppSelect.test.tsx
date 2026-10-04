import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AppSelect } from "./AppSelect";

const options = [
  { value: "fabric", label: "Fabric" },
  { value: "quilt", label: "Quilt" },
  { value: "forge", label: "Forge" },
  { value: "vanilla", label: "Vanilla" },
];

afterEach(cleanup);

describe("AppSelect", () => {
  it("shows the themed loader list and chooses Forge", () => {
    const onChange = vi.fn();
    render(<AppSelect ariaLabel="Загрузчик" onChange={onChange} options={options} value="fabric" />);

    fireEvent.click(screen.getByRole("button", { name: "Загрузчик" }));
    expect(screen.getByRole("listbox", { name: "Загрузчик" })).toBeTruthy();
    fireEvent.click(screen.getByRole("option", { name: "Forge" }));

    expect(onChange).toHaveBeenCalledWith("forge");
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("closes on Escape and outside click", () => {
    render(<AppSelect ariaLabel="Загрузчик" onChange={vi.fn()} options={options} value="fabric" />);
    fireEvent.click(screen.getByRole("button", { name: "Загрузчик" }));
    fireEvent.keyDown(screen.getByRole("listbox"), { key: "Escape" });
    expect(screen.queryByRole("listbox")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Загрузчик" }));
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("listbox")).toBeNull();
  });
});
