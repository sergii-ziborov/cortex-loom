package langs;

public final class Retry {
    public static final int JAVA_RETRY_CAP = 8;

    private Retry() {}

    public static int scheduleJavaRetry(int attempt) {
        if (attempt >= JAVA_RETRY_CAP) {
            return 0;
        }
        return 1 << attempt;
    }
}
