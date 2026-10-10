import type { OperationJob } from "../browser/operationApi";
import { operationGroups } from "./operationModel";
import { TRANSFER_PROGRESS_DELAY_MS, TRANSFER_SUCCESS_DISMISS_MS } from "../commands/timing";
export type FeedbackGroup = ReturnType<typeof operationGroups>[number];
interface Tracked {
  group: FeedbackGroup;
  since: number;
  shown: boolean;
  finishedAt: number | null;
  dismissed: boolean;
}
/** Deadline-driven store: short successes never become visible, failures never auto-dismiss. */
export class FeedbackTracker {
  private tracked = new Map<string, Tracked>();
  private listeners = new Set<() => void>();
  private timer: ReturnType<typeof setTimeout> | null = null;
  private visible: FeedbackGroup[] = [];
  subscribe = (callback: () => void) => {
    this.listeners.add(callback);
    return () => {
      this.listeners.delete(callback);
    };
  };
  snapshot = () => this.visible;
  update(jobs: OperationJob[], now = Date.now()) {
    for (const group of operationGroups(jobs)) {
      let item = this.tracked.get(group.id);
      if (!item) {
        item = {
          group,
          since: now - group.current.elapsedMillis,
          shown: false,
          finishedAt: null,
          dismissed: false,
        };
        this.tracked.set(group.id, item);
      }
      item.group = group;
      if (!group.active && item.finishedAt === null) item.finishedAt = now;
    }
    this.refresh(now);
  }
  dismiss(id: string) {
    const item = this.tracked.get(id);
    if (item) item.dismissed = true;
    this.refresh(Date.now());
  }
  dispose() {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }
  private refresh(now: number) {
    this.dispose();
    let next = Infinity;
    const visible: FeedbackGroup[] = [];
    for (const item of this.tracked.values()) {
      if (item.dismissed) continue;
      const group = item.group,
        transfer = ["copy", "move"].includes(group.current.kind);
      const exceptional = ["failed", "partiallyFailed", "cancelled", "awaitingDecision"].includes(
        group.state,
      );
      if (exceptional) item.shown = true;
      else if (transfer && group.active) {
        if (now - item.since >= TRANSFER_PROGRESS_DELAY_MS) item.shown = true;
        else next = Math.min(next, item.since + TRANSFER_PROGRESS_DELAY_MS);
      }
      if (!group.active && !exceptional) {
        if (!item.shown) {
          item.dismissed = true;
          continue;
        }
        const expires = item.finishedAt! + TRANSFER_SUCCESS_DISMISS_MS;
        if (now >= expires) {
          item.dismissed = true;
          continue;
        }
        next = Math.min(next, expires);
      }
      if (item.shown) visible.push(group);
    }
    this.visible = visible;
    for (const listener of this.listeners) listener();
    if (Number.isFinite(next))
      this.timer = setTimeout(() => this.refresh(Date.now()), Math.max(0, next - now));
  }
}
