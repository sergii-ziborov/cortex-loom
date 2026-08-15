namespace Langs;

public static class Retry
{
    public const int CsRetryCap = 8;

    public static int ScheduleCsRetry(int attempt)
    {
        if (attempt >= CsRetryCap)
        {
            return 0;
        }
        return 1 << attempt;
    }
}
