import { beforeEach, describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import Toasts from "./Toasts";
import { useToasts } from "./toastStore";

describe("토스트", () => {
  beforeEach(() => useToasts.setState({ toasts: [] }));

  it("없으면 아무것도 안 그린다", () => {
    const { container } = render(<Toasts />);
    expect(container).toBeEmptyDOMElement();
  });

  it("띄운 말이 보이고 누르면 사라진다", async () => {
    useToasts.getState().push("3장 옮겼습니다", "ok", 0);
    render(<Toasts />);
    const t = screen.getByRole("status");
    expect(t).toHaveTextContent("3장 옮겼습니다");
    expect(t.className).toContain("text-keep");
    await userEvent.click(t);
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });
});
