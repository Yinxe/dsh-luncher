/** 体积按十进制 MB 显示（与 GitHub Release 页面的口径一致，不玩 MiB/MB 双标） */
export function formatBytes(n: number | null | undefined): string {
  if (!n || n <= 0) return "—";
  if (n < 1000) return `${n} B`;
  if (n < 1000 * 1000) return `${(n / 1000).toFixed(0)} KB`;
  if (n < 1000 * 1000 * 1000) return `${(n / 1000 / 1000).toFixed(1)} MB`;
  return `${(n / 1000 / 1000 / 1000).toFixed(2)} GB`;
}

/** ISO 时间 → `2026-09-21`（清单里的时间是 UTC，只取日期，避免时区错觉） */
export function formatDate(iso: string | null | undefined): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "—";
  return d.toISOString().slice(0, 10);
}

/** 距今多久：下载页最有用的一句话是「这个包有多新」 */
export function relativeTime(iso: string | null | undefined, now = Date.now()): string {
  if (!iso) return "";
  const t = new Date(iso).getTime();
  if (Number.isNaN(t)) return "";
  const diff = now - t;
  if (diff < 0) return "刚刚";
  const min = Math.floor(diff / 60000);
  if (min < 1) return "刚刚";
  if (min < 60) return `${min} 分钟前`;
  const hour = Math.floor(min / 60);
  if (hour < 24) return `${hour} 小时前`;
  const day = Math.floor(hour / 24);
  if (day < 30) return `${day} 天前`;
  const month = Math.floor(day / 30);
  if (month < 12) return `${month} 个月前`;
  return `${Math.floor(month / 12)} 年前`;
}

/** 直链太长，列表里只显示尾巴，完整值走 title/复制 */
export function shortenUrl(url: string, keep = 46): string {
  if (url.length <= keep) return url;
  const head = url.slice(0, 22);
  const tail = url.slice(-(keep - 22 - 1));
  return `${head}…${tail}`;
}
