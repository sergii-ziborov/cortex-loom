package langs

const GoRetryCap = 8

func ScheduleGoRetry(attempt int) int {
	if attempt >= GoRetryCap {
		return 0
	}
	return 1 << attempt
}
