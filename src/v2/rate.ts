/**
 * 진행 속도 — «초당 몇 장»과 «몇 분 남았나».
 *
 * 처음엔 «초당 약 20장이면»이라고 못 박아 적었다. 맥이 바쁘면 20장, 한가하면
 * 90장이라 그 문구는 늘 틀렸다. 최근 30초 창의 실제 처리량으로 센다 — 전체
 * 평균은 처음 느렸던 구간에 오래 끌려 다닌다.
 */
export type Sample = { t: number; n: number };

/** 속도를 재는 창(ms). 이보다 오래된 표본은 버린다. */
export const WINDOW_MS = 30_000;
/** 창이 이보다 짧으면 아직 못 잰다 — 첫 두 알림 사이로 추정하면 크게 튄다. */
const MIN_SPAN_MS = 2_000;

/** 표본을 더하고 창 밖의 것은 버린다. 원본은 두고 새 배열을 준다. */
export function pushSample(samples: readonly Sample[], s: Sample): Sample[] {
  const floor = s.t - WINDOW_MS;
  // 창 경계 바로 바깥의 하나는 남긴다 — 그래야 창 전체 길이로 잴 수 있다
  const idx = samples.findIndex((x) => x.t >= floor);
  const kept =
    idx === -1
      ? samples.slice(-1)
      : idx === 0
        ? samples
        : samples.slice(idx - 1);
  return [...kept, s];
}

/** 최근 창의 초당 처리량. 아직 못 재면 null */
export function rateOf(samples: readonly Sample[], now: number): number | null {
  if (samples.length < 2) return null;
  const last = samples[samples.length - 1];
  if (now - last.t > WINDOW_MS) return null; // 한동안 소식이 없다 — 멎은 것
  const first = samples.find((x) => x.t >= now - WINDOW_MS) ?? samples[0];
  const span = last.t - first.t;
  if (span < MIN_SPAN_MS) return null;
  return ((last.n - first.n) * 1000) / span;
}

/** 남은 시간(초). 속도를 모르거나 0이면 null */
export function etaSec(left: number, rate: number | null): number | null {
  if (rate === null || rate <= 0) return null;
  return left / rate;
}

/** «약 3분», «약 1시간 20분», «1분 안» */
export function fmtEta(sec: number | null): string {
  if (sec === null) return "";
  if (sec < 60) return "1분 안";
  const m = Math.round(sec / 60);
  if (m < 60) return `약 ${m}분`;
  const h = Math.floor(m / 60);
  const r = m % 60;
  return r === 0 ? `약 ${h}시간` : `약 ${h}시간 ${r}분`;
}
