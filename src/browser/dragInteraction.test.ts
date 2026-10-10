import { afterEach, describe, expect, it, vi } from "vitest";
import { DragInteraction } from "./dragInteraction";

afterEach(() => vi.useRealTimers());
describe("drag hover and edge scrolling", () => {
  it("expands once at 700 ms without resetting on repeated dragover", async () => {
    vi.useFakeTimers();
    const interaction = new DragInteraction();
    const expand = vi.fn(),
      update = vi.fn(),
      validate = vi.fn(async () => true);
    interaction.target("folder", validate, update, expand);
    await Promise.resolve();
    vi.advanceTimersByTime(400);
    expect(interaction.target("folder", validate, update, expand)).toBe(true);
    vi.advanceTimersByTime(299);
    expect(expand).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(expand).toHaveBeenCalledTimes(1);
    expect(validate).toHaveBeenCalledTimes(1);
    interaction.clear();
    expect(vi.getTimerCount()).toBe(0);
  });
  it("does not expand invalid targets or allow late validation to revive cancelled drag", async () => {
    vi.useFakeTimers();
    const interaction = new DragInteraction(),
      expand = vi.fn(),
      update = vi.fn();
    let finish!: (allowed: boolean) => void;
    interaction.target(
      "old",
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
      update,
      expand,
    );
    interaction.clear();
    finish(true);
    await Promise.resolve();
    interaction.target("invalid", async () => false, update, expand);
    await Promise.resolve();
    vi.advanceTimersByTime(1000);
    expect(update).toHaveBeenCalledExactlyOnceWith(false);
    expect(expand).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  });
  it("cancels the previous directory when the target or modifier changes", async () => {
    vi.useFakeTimers();
    const interaction = new DragInteraction(),
      old = vi.fn(),
      next = vi.fn();
    interaction.target("folder:move", async () => true, vi.fn(), old);
    await Promise.resolve();
    vi.advanceTimersByTime(500);
    interaction.target("folder:copy", async () => true, vi.fn(), next);
    await Promise.resolve();
    vi.advanceTimersByTime(700);
    expect(old).not.toHaveBeenCalled();
    expect(next).toHaveBeenCalledTimes(1);
    interaction.clear();
  });
  it("scrolls continuously at edges and cleans up on exit", () => {
    vi.useFakeTimers();
    const viewport = document.createElement("div");
    Object.defineProperties(viewport, {
      scrollHeight: { value: 1000 },
      clientHeight: { value: 200 },
    });
    viewport.getBoundingClientRect = () => ({ top: 0, bottom: 200 }) as DOMRect;
    const interaction = new DragInteraction();
    interaction.edge(viewport, 195);
    for (let i = 0; i < 10; i++) {
      interaction.edge(viewport, 195);
      vi.advanceTimersByTime(20);
    }
    expect(viewport.scrollTop).toBe(80);
    interaction.edge(viewport, 100);
    vi.advanceTimersByTime(200);
    expect(viewport.scrollTop).toBe(80);
    interaction.edge(viewport, 5);
    vi.advanceTimersByTime(80);
    expect(viewport.scrollTop).toBe(48);
    interaction.clear();
    vi.advanceTimersByTime(400);
    expect(viewport.scrollTop).toBe(48);
    expect(vi.getTimerCount()).toBe(0);
  });
});
