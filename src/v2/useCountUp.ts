import { useEffect, useRef, useState } from "react";

/**
 * 목표 숫자까지 **한 칸씩** 올라가는 표시값.
 *
 * 진행 알림은 초당 20번 오는데 그 사이에 열 장쯤 처리된다. 그대로 그리면
 * 숫자가 뭉텅뭉텅 뛴다. 그래서 화면에는 프레임마다 조금씩 올린다.
 *
 * 한 프레임에 1씩만 올리면 초당 60까지밖에 못 세서 실제(초당 177장)를
 * 영영 못 따라잡는다. 그래서 **멀어질수록 성큼** 간다 — 가까우면 1씩,
 * 벌어지면 간격의 1/8씩. 눈에는 계속 세는 것처럼 보이고 끝날 때는 정확히 맞다.
 */
export function useCountUp(target: number): number {
  const [shown, setShown] = useState(target);
  const raf = useRef(0);
  const cur = useRef(target);

  useEffect(() => {
    // 뒤로 가거나(새 작업 시작) 크게 벌어지면 따라가지 않고 그냥 맞춘다
    if (target < cur.current || target - cur.current > 100_000) {
      cur.current = target;
      setShown(target);
      return;
    }

    const step = () => {
      const gap = target - cur.current;
      if (gap <= 0) {
        raf.current = 0;
        return;
      }
      cur.current += Math.max(1, Math.ceil(gap / 8));
      if (cur.current > target) cur.current = target;
      setShown(cur.current);
      raf.current = requestAnimationFrame(step);
    };

    if (raf.current === 0) raf.current = requestAnimationFrame(step);
    return () => {
      cancelAnimationFrame(raf.current);
      raf.current = 0;
    };
  }, [target]);

  return shown;
}
