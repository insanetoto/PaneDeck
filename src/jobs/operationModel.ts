import type { OperationJob, OperationJobState } from "../browser/operationApi";
export const activeStates = new Set<OperationJobState>([
  "queued",
  "validating",
  "running",
  "awaitingDecision",
  "cancellationRequested",
]);
export const isActiveJob = (job: OperationJob) => activeStates.has(job.state);
export function operationGroups(jobs: OperationJob[]) {
  const groups = new Map<string, OperationJob[]>();
  for (const job of jobs) {
    const id = job.operationId ?? `job-${job.jobId}`;
    groups.set(id, [...(groups.get(id) ?? []), job]);
  }
  return [...groups].map(([id, children]) => {
    const active = children.filter(isActiveJob).sort((a, b) => a.jobId - b.jobId);
    const current = active[0] ?? children[0];
    const final = children.find((job) => job.operationState != null);
    const state =
      final?.operationState ?? (current.operationId && !active.length ? "running" : current.state);
    const outcomes = final?.operationOutcomes ?? children.flatMap((job) => job.itemOutcomes);
    return {
      id,
      children,
      current,
      state: state as OperationJobState,
      outcomes,
      active: activeStates.has(state as OperationJobState),
    };
  });
}
export function percent(job: OperationJob) {
  return job.totalUnits != null && job.totalUnits > 0 && job.completedUnits != null
    ? Math.min(100, Math.max(0, Math.floor((job.completedUnits / job.totalUnits) * 100)))
    : null;
}
export function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
}
