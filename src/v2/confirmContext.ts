import { createContext, useContext } from "react";

/**
 * 물음 상자에 넘길 것. 상자 자체는 `confirm.tsx`가 그린다.
 *
 * 훅과 문맥을 따로 두는 이유: 컴포넌트 파일에서 함수까지 내보내면
 * Fast Refresh가 파일 전체를 다시 얹으면서 화면 상태가 날아간다.
 */
export type Ask = {
  title: string;
  /** 무엇이 어떻게 되는지. 줄 단위로 준다. */
  lines?: string[];
  /** 확인 버튼 글자. 기본 「확인」 */
  confirmLabel?: string;
  /** 되돌릴 수 없는 일이면 true — 버튼이 붉어진다 */
  danger?: boolean;
};

export type AskFn = (a: Ask) => Promise<boolean>;

export const ConfirmCtx = createContext<AskFn>(async () => false);

/** 물어보는 함수. 답할 때까지 기다린다. */
export const useConfirm = (): AskFn => useContext(ConfirmCtx);
