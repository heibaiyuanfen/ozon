/** Display an API timestamp without hiding the UTC/local-time distinction. */
export function displayTime(value: string | null | undefined, businessTimeZone = "Europe/Moscow"): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  const format = (timeZone: string) => {
    try {
      return new Intl.DateTimeFormat("zh-CN", {
        timeZone, year: "numeric", month: "2-digit", day: "2-digit",
        hour: "2-digit", minute: "2-digit", hourCycle: "h23",
      }).format(date).replaceAll("/", "-");
    } catch { return value; }
  };
  const zone = businessTimeZone || "Europe/Moscow";
  return `${format(zone)}（${zone}） · 中国 ${format("Asia/Shanghai")}`;
}

export function displayDate(value: string | null | undefined, timeZone = "Asia/Shanghai"): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  try {
    return new Intl.DateTimeFormat("zh-CN", { timeZone, year: "numeric", month: "2-digit", day: "2-digit" }).format(date).replaceAll("/", "-");
  } catch { return value; }
}
