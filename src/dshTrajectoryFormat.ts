/**
 * Wall-clock formatting for the trajectory panel.
 *
 * Two resolutions, because the two surfaces answer different questions: a ledger
 * row is scanned against its neighbours, so it shows the clock alone, while the
 * detail column's `Started` is quoted against logs outside the app and therefore
 * carries the date and the milliseconds the events are stamped at.
 */

function pad(value: number, width = 2): string {
  return String(value).padStart(width, "0");
}

/** The clock a ledger row shows; `--:--:--` for an event with no timestamp. */
export function dshDisplayTime(time: number): string {
  if (!time) return "--:--:--";
  const at = new Date(time);
  return `${pad(at.getHours())}:${pad(at.getMinutes())}:${pad(at.getSeconds())}`;
}

/** The detail column's `Started`: local date, clock, and milliseconds. */
export function dshStartedAt(time: number): string {
  if (!time) return "--";
  const at = new Date(time);
  const date = `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}`;
  const clock = `${pad(at.getHours())}:${pad(at.getMinutes())}:${pad(at.getSeconds())}`;
  return `${date} ${clock}.${pad(at.getMilliseconds(), 3)}`;
}
