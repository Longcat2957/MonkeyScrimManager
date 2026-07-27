const expectedStartInputPattern = /^(\d{4})-(\d{2})-(\d{2}) ([01]\d|2[0-3]):([0-5]\d)$/;

export const expectedStartInputExample = '2026-07-26 20:00';

export function expectedStartAtFromKstInput(value: string | null): string | null {
	const match = expectedStartInputPattern.exec(value?.trim() ?? '');
	if (!match) return null;

	const [, yearText, monthText, dayText, hourText, minuteText] = match;
	const year = Number(yearText);
	const month = Number(monthText);
	const day = Number(dayText);
	const hour = Number(hourText);
	const minute = Number(minuteText);
	const parsed = new Date(Date.UTC(year, month - 1, day, hour, minute));
	if (
		parsed.getUTCFullYear() !== year ||
		parsed.getUTCMonth() !== month - 1 ||
		parsed.getUTCDate() !== day ||
		parsed.getUTCHours() !== hour ||
		parsed.getUTCMinutes() !== minute
	) {
		return null;
	}

	return `${yearText}-${monthText}-${dayText}T${hourText}:${minuteText}:00+09:00`;
}
