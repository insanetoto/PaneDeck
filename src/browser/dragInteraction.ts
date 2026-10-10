import { DIRECTORY_HOVER_EXPAND_MS } from "../commands/timing";

export const INTERNAL_DRAG_TYPE = "application/x-panedeck-entries";
/** One hover and one scroll timer per list. Async validation cannot revive a cancelled hover. */
export class DragInteraction {
  private key: string | null = null;
  private generation = 0;
  private hover: ReturnType<typeof setTimeout> | null = null;
  private scroll: ReturnType<typeof setInterval> | null = null;
  private allowed = false;
  private scrollDirection = 0;
  private scrollViewport: HTMLElement | null = null;
  target(
    key: string,
    validate: () => Promise<boolean>,
    update: (allowed: boolean) => void,
    expand?: () => void,
  ) {
    if (this.key === key) return this.allowed;
    this.clearHover();
    this.key = key;
    this.allowed = false;
    const generation = this.generation;
    const started = Date.now();
    void validate()
      .then((allowed) => {
        if (this.generation !== generation) return;
        this.allowed = allowed;
        update(allowed);
        if (allowed && expand)
          this.hover = setTimeout(
            () => {
              this.hover = null;
              expand();
            },
            Math.max(0, DIRECTORY_HOVER_EXPAND_MS - (Date.now() - started)),
          );
      })
      .catch(() => {
        if (this.generation === generation) update(false);
      });
    return false;
  }
  edge(viewport: HTMLElement, y: number) {
    const rect = viewport.getBoundingClientRect();
    const direction = y < rect.top + 32 ? -1 : y > rect.bottom - 32 ? 1 : 0;
    if (direction === this.scrollDirection && viewport === this.scrollViewport) return;
    this.stopScroll();
    if (!direction) return;
    this.scrollDirection = direction;
    this.scrollViewport = viewport;
    this.scroll = setInterval(() => {
      const previous = viewport.scrollTop;
      viewport.scrollTop = Math.max(
        0,
        Math.min(
          viewport.scrollHeight - viewport.clientHeight,
          viewport.scrollTop + direction * 16,
        ),
      );
      if (previous !== viewport.scrollTop) {
        this.clearHover();
        viewport.dispatchEvent(new Event("scroll"));
      }
    }, 40);
  }
  clearHover() {
    this.generation++;
    this.key = null;
    this.allowed = false;
    if (this.hover !== null) clearTimeout(this.hover);
    this.hover = null;
  }
  stopScroll() {
    if (this.scroll !== null) clearInterval(this.scroll);
    this.scroll = null;
    this.scrollDirection = 0;
    this.scrollViewport = null;
  }
  clear() {
    this.clearHover();
    this.stopScroll();
  }
}
