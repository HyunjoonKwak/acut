import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { useConfirm } from "./confirmContext";
import { fmtDateTime } from "./format";
import { DEFAULT_PREFS, usePrefs, type Prefs } from "./prefs";
import { toast } from "./toastStore";
import { Btn } from "./ui";
import { useUi } from "./uiStore";
import { Section, Row } from "./settingsUi";

export function Advanced() {
  const ask = useConfirm();
  const setUi = useUi((s) => s.set);
  const reset = async () => {
    const ok = await ask({
      title: "설정을 처음 상태로 되돌립니다",
      lines: ["· 보기·크기·글꼴 같은 설정만 — 사진·판정·태그는 그대로입니다"],
      confirmLabel: "되돌리기",
    });
    if (!ok) return;
    const set = usePrefs.getState().set;
    (Object.keys(DEFAULT_PREFS) as (keyof Prefs)[]).forEach((k) =>
      (set as (k: keyof Prefs, v: unknown) => void)(k, DEFAULT_PREFS[k]),
    );
    toast("설정을 되돌렸습니다");
  };
  return (
    <Section id="advanced" title="고급">
      <Row label="단축키" hint="목록·크게 보기·나란히 보기의 키를 한 장에">
        <Btn hint="?" onClick={() => setUi({ helping: true })}>
          보기
        </Btn>
      </Row>
      <Row label="설정 초기화">
        <Btn onClick={reset}>처음 상태로</Btn>
      </Row>
    </Section>
  );
}

export function About() {
  const [startup, setStartup] = useState<{
    db_ms: number;
    first_grid_ms: number;
    at: number;
    marks?: Record<string, number>;
  } | null>(null);
  const [ver, setVer] = useState("");
  useEffect(() => {
    invoke<string | null>("settings_get", { key: "startup.last" })
      .then((v) => setStartup(v ? JSON.parse(v) : null))
      .catch(() => {});
    getVersion()
      .then(setVer)
      .catch(() => setVer(""));
  }, []);
  return (
    <Section id="about" title="정보">
      <Row
        label="마지막 시작"
        hint={
          startup
            ? `프로세스 시작 → DB 준비 ${startup.db_ms}ms · 첫 그리드 ${startup.first_grid_ms}ms (${fmtDateTime(startup.at)}). 목표는 1초.${
                startup.marks
                  ? ` 웹뷰 기준: ${Object.entries(startup.marks)
                      .map(([k, v]) => `${k} ${v}`)
                      .join(" · ")}`
                  : ""
              }`
            : "아직 잰 적 없습니다."
        }
      >
        <span />
      </Row>
      <Row
        label="에이컷"
        hint="대규모 로컬 라이브러리를 위한 오프라인 우선 사진 관리자. 사진은 원래 자리에 그대로 둡니다."
      >
        <span className="text-[13.5px] text-fg-mute tabular-nums">
          {ver ? `v${ver}` : "—"}
        </span>
      </Row>
      <Row
        label="글꼴"
        hint="Pretendard — Kil Hyung-jin, SIL Open Font License 1.1"
      >
        <span />
      </Row>
    </Section>
  );
}

