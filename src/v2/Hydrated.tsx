import { useEffect, useState } from "react";
import { mark } from "./startupMarks";
import { usePrefs } from "./prefs";

/**
 * 저장된 설정을 읽어 올 때까지 기다린다.
 *
 * 안 기다리면 기본값으로 한 번 그렸다가 저장값으로 다시 그린다 — 썸네일
 * 크기가 튀고 사이드바가 접혔다 펴진다. SQLite 한 줄이라 몇 ms면 온다.
 */
export default function Hydrated({ children }: { children: React.ReactNode }) {
  const [ready, setReady] = useState(usePrefs.persist.hasHydrated());
  useEffect(() => {
    // 구독하기 전에 이미 끝났을 수 있다. 그 경우도 콜백으로 알린다 —
    // 효과 안에서 바로 setState하지 않는다.
    const un = usePrefs.persist.onFinishHydration(() => setReady(true));
    if (usePrefs.persist.hasHydrated()) queueMicrotask(() => setReady(true));
    return un;
  }, []);
  if (!ready) return <div className="h-screen bg-canvas" />;
  mark("hydrated");
  return <>{children}</>;
}
