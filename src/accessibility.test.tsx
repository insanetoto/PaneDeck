import tokens from "./styles/tokens.css?raw";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import App from "./App";
import { I18nProvider, LANGUAGE_STORAGE_KEY } from "./i18n/I18nProvider";
import { enMessages, zhCNMessages } from "./i18n/messages";

function luminance(hex: string) {
  const channels = hex
    .replace("#", "")
    .match(/.{2}/g)!
    .map((channel) => Number.parseInt(channel, 16) / 255)
    .map((channel) => (channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4));
  return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2];
}

function contrast(foreground: string, background: string) {
  const [lighter, darker] = [luminance(foreground), luminance(background)].sort((a, b) => b - a);
  return (lighter + 0.05) / (darker + 0.05);
}

describe("accessibility and bilingual audit", () => {
  beforeEach(() => window.localStorage.clear());
  afterEach(cleanup);

  it("keeps both language catalogs complete and non-empty", () => {
    expect(Object.keys(zhCNMessages).sort()).toEqual(Object.keys(enMessages).sort());
    expect(Object.values(enMessages).every(Boolean)).toBe(true);
    expect(Object.values(zhCNMessages).every(Boolean)).toBe(true);
  });

  it("meets WCAG AA text contrast for light and dark surfaces", () => {
    const themes = tokens.split("@media (prefers-color-scheme: dark)");
    expect(themes).toHaveLength(2);
    const light = Object.fromEntries(
      [...themes[0].matchAll(/--(color-[\w-]+): (#[a-f0-9]{6})/g)].map((m) => [m[1], m[2]]),
    );
    const dark = {
      ...light,
      ...Object.fromEntries(
        [...themes[1].matchAll(/--(color-[\w-]+): (#[a-f0-9]{6})/g)].map((m) => [m[1], m[2]]),
      ),
    };
    for (const theme of [light, dark]) {
      for (const text of ["primary", "secondary", "tertiary"]) {
        for (const surface of ["surface", "surface-raised", "canvas", "control-selected"]) {
          expect(
            contrast(theme[`color-text-${text}`], theme[`color-${surface}`]),
          ).toBeGreaterThanOrEqual(4.5);
        }
      }
      expect(contrast(theme["color-danger"], theme["color-surface-raised"])).toBeGreaterThanOrEqual(
        4.5,
      );
    }
  });

  it("exposes named panes and core actions in Simplified Chinese without mouse-only controls", () => {
    window.localStorage.setItem(LANGUAGE_STORAGE_KEY, "zh-CN");
    render(
      <I18nProvider>
        <App />
      </I18nProvider>,
    );

    expect(document.documentElement).toHaveAttribute("lang", "zh-CN");
    const left = screen.getByRole("region", { name: "左窗格" });
    expect(within(left).queryByText("活动")).toBeNull();
    expect(within(left).getByRole("textbox", { name: "文件夹路径" })).toBeInTheDocument();
    fireEvent.contextMenu(left.querySelector(".file-area")!);
    expect(screen.getByRole("menuitem", { name: /粘贴/ })).toBeDisabled();
    expect(screen.getByRole("menuitem", { name: /新建文件夹/ })).toBeDisabled();
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });

    const main = screen.getByRole("main", { name: "PaneDeck" });
    fireEvent.keyDown(main, { key: "Tab", ctrlKey: true });
    expect(screen.getByRole("region", { name: "右窗格" })).toHaveFocus();
    expect(screen.queryByText("活动")).toBeNull();
  });
});
