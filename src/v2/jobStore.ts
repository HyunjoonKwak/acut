import { create } from "zustand";

/**
 * 지금 도는 일 — 스캔·썸네일·가져오기의 진행 숫자.
 *
 * App의 useState였다. 진행 알림이 초당 20번 오는데 그때마다 App 전체가
 * 다시 그려져 그리드·사이드바·스크롤바까지 같이 돌았다. 여기에 두면
 * 숫자를 그리는 한 칸만 다시 그린다.
 *
 * 숫자는 **뒤로 가지 않는다.** 같은 일의 알림인데 지금보다 작은 값이 오면
 * 버린다 — 이벤트가 순서를 어긋나 도착해도 화면이 오르락내리락하지 않는다.
 */
export type Job = { label: string; done: number; total: number };

type Store = {
  job: Job | null;
  /** 진행 알림. 같은 일이면 앞으로만 간다. */
  progress: (j: Job) => void;
  clear: () => void;
};

export const useJob = create<Store>()((set) => ({
  job: null,
  progress: (j) =>
    set((s) => {
      const cur = s.job;
      const same = cur && cur.label === j.label && cur.total === j.total;
      if (same && j.done < cur.done) return s; // 늦게 온 옛 알림
      if (same && j.done === cur.done) return s; // 같은 값 — 다시 그리지 않는다
      return { job: j };
    }),
  clear: () => set({ job: null }),
}));
