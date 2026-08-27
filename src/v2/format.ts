/** 사람이 읽는 값으로 바꾸는 것들. 화면 여러 곳이 같은 규칙을 써야 한다. */

export function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const u = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(1)} ${u[i]}`;
}

export const fmtDate = (ts: number) =>
  new Date(ts * 1000).toLocaleDateString("ko-KR", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  });

export const fmtDateTime = (ts: number) =>
  new Date(ts * 1000).toLocaleString("ko-KR", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });

/** 12345678 → `2:03`, 한 시간을 넘으면 `1:02:03` */
export function fmtDuration(ms: number): string {
  const t = Math.round(ms / 1000);
  const h = Math.floor(t / 3600);
  const m = Math.floor((t % 3600) / 60);
  const s = t % 60;
  return h > 0
    ? `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`
    : `${m}:${String(s).padStart(2, "0")}`;
}

/** 백만 화소. 1MP 미만이면 빈 문자열 */
export function megapixels(w: number, h: number): string {
  const n = (w * h) / 1_000_000;
  return n >= 1 ? `${n.toFixed(1)}MP` : "";
}
