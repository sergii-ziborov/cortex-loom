export const JS_RETRY_CAP = 8;

export function scheduleJsRetry(attempt) {
  if (attempt >= JS_RETRY_CAP) {
    return 0;
  }
  return 2 ** attempt;
}
