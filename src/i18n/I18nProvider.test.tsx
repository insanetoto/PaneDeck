import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import App from "../App";
import { I18nProvider, LANGUAGE_STORAGE_KEY, resolvePreferredLanguage } from "./I18nProvider";

describe("language foundation", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.localStorage.setItem(LANGUAGE_STORAGE_KEY, "zh-CN");
  });

  afterEach(() => {
    cleanup();
    window.localStorage.clear();
  });

  it("switches visible text immediately and persists the choice", () => {
    render(
      <I18nProvider>
        <App />
      </I18nProvider>,
    );

    expect(screen.getByText("左窗格")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "EN" }));

    expect(screen.getByText("Left pane")).toBeInTheDocument();
    expect(document.documentElement).toHaveAttribute("lang", "en");
    expect(window.localStorage.getItem(LANGUAGE_STORAGE_KEY)).toBe("en");
  });

  it("selects Chinese for any Chinese system locale", () => {
    expect(resolvePreferredLanguage(["zh-Hant-HK", "en-US"])).toBe("zh-CN");
    expect(resolvePreferredLanguage(["en-US"])).toBe("en");
  });
});
