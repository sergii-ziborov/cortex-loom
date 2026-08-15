export const TS_RETRY_CAP = 8;

export function scheduleTsRetry(attempt: number): number {
  if (attempt >= TS_RETRY_CAP) {
    return 0;
  }
  return 2 ** attempt;
}
