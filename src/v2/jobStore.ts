import { create } from "zustand";
import { pushSample, type Sample } from "./rate.ts";

/**
 * 지금 도는 일 — 스캔·썸네일·가져오기·AI 벡터의 진행 숫자.
 *
 * App의 useState였다. 진행 알림이 초당 20번 오는데 그때마다 App 전체가
 * 다시 그려져 그리드·사이드바·스크롤바까지 같이 돌았다. 여기에 두면
 * 숫자를 그리는 한 칸만 다시 그린다.
 *
 * 숫자는 **뒤로 가지 않는다.** 같은 일의 알림인데 지금보다 작은 값이 오면
 * 버린다 — 이벤트가 순서를 어긋나 도착해도 화면이 오르락내리락하지 않는다.
 *
 * 알림이 올 때마다 (시각, 개수) 표본을 남긴다 — 그걸로 «초당 몇 장»과
 * «몇 분 남았나»를 센다 (rate.ts). 새 일이 시작되면 표본도 새로.
 */
export type Job = { label: string; done: number; total: number };

type Store = {
  job: Job | null;
  /** 진행 표본 — 최근 창만 남는다 */
  samples: Sample[];
  /** 진행 알림. 같은 일이면 앞으로만 간다. now는 시험에서만 준다. */
  progress: (j: Job, now?: number) => void;
  clear: () => void;
};

export const useJob = create<Store>()((set) => ({
  job: null,
  samples: [],
  progress: (j, now = Date.now()) =>
    set((s) => {
      const cur = s.job;
      const same = cur && cur.label === j.label && cur.total === j.total;
      if (same && j.done < cur.done) return s; // 늦게 온 옛 알림
      if (same && j.done === cur.done) return s; // 같은 값 — 다시 그리지 않는다
      const sample = { t: now, n: j.done };
      return {
        job: j,
        samples: same ? pushSample(s.samples, sample) : [sample],
      };
    }),
  clear: () => set({ job: null, samples: [] }),
}));
