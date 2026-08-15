PY_RETRY_CAP = 8


def schedule_py_retry(attempt: int) -> int:
    if attempt >= PY_RETRY_CAP:
        return 0
    return 2**attempt
