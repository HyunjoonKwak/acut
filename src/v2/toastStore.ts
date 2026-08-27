import { create } from "zustand";

/**
 * 토스트 — 작업 결과가 잠깐 뜨고 사라진다.
 *
 * 상태바 글자로 갈음하고 있었는데, 거긴 «지금 뭐 하는 중»과 «방금 뭐가
 * 됐다»가 한 자리를 다퉜다. 결과는 여기로, 진행은 상태바에.
 */
export type Tone = "plain" | "ok" | "drop";
export type Toast = { id: number; text: string; tone: Tone };

type Store = {
  toasts: Toast[];
  /** 띄운다. 같은 글이 이미 떠 있으면 다시 띄우지 않는다. */
  push: (text: string, tone?: Tone, ttlMs?: number) => number;
  dismiss: (id: number) => void;
};

/** 기본으로 떠 있는 시간 */
export const TTL_MS = 4000;

let seq = 0;

export const useToasts = create<Store>()((set, get) => ({
  toasts: [],
  push: (text, tone = "plain", ttlMs = TTL_MS) => {
    const dup = get().toasts.find((t) => t.text === text);
    if (dup) return dup.id;
    const id = ++seq;
    set((s) => ({ toasts: [...s.toasts, { id, text, tone }] }));
    if (ttlMs > 0) setTimeout(() => get().dismiss(id), ttlMs);
    return id;
  },
  dismiss: (id) =>
    set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));

/** 어디서든 한 줄로. `toast("3장 옮겼습니다")` */
export const toast = (text: string, tone: Tone = "plain") =>
  useToasts.getState().push(text, tone);
