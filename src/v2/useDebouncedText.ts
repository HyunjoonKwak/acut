import { useEffect, useRef, useState, type Dispatch, type SetStateAction } from "react";

/**
 * 타이핑이 멎은 뒤에야 값을 흘려보낸다.
 *
 * 한 글자마다 14만 행을 훑으면 입력이 밀린다. 그렇다고 값이 바뀔 때마다
 * 타이머를 되감으면 필터가 자주 바뀌는 화면에서는 영영 발화하지 않는다 —
 * 최신 콜백은 ref로 보고, 타이머는 **입력한 글자에만** 반응하게 한다.
 *
 * @returns 지금 상자에 보일 글자와 그것을 바꾸는 함수
 */
export function useDebouncedText(
  initial: string,
  ms: number,
  onSettled: (text: string) => void,
): [string, Dispatch<SetStateAction<string>>] {
  const [text, setText] = useState(initial);
  const fire = useRef(onSettled);
  useEffect(() => {
    fire.current = onSettled;
  });

  useEffect(() => {
    const t = setTimeout(() => fire.current(text), ms);
    return () => clearTimeout(t);
  }, [text, ms]);

  return [text, setText];
}
