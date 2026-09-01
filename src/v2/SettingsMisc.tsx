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
import { listen } from "@tauri-apps/api/event";

/** 백엔드가 알려 주는 새 판 정보 — src-tauri/src/api/update.rs 와 짝이다 */
type UpdateInfo = {
  current: string;
  latest: string;
  newer: boolean;
  notes: string;
  page_url: string;
  published_at: string | null;
  asset_name: string | null;
  asset_url: string | null;
  asset_size: number | null;
};

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
            ? `프로세스 시작 → DB 준비 ${startup.db_ms}ms · 첫 그리드 ${startup.first_grid_ms}ms (${fmtDateTime(startup.at)}).${
                startup.marks
                  ? ` 웹뷰 기준: ${Object.entries(startup.marks)
                      .map(([k, v]) => `${k} ${v}`)
                      .join(" · ")}.`
                  : ""
              } 사진이 5만 장을 넘으면 첫 그리드가 몇 초 걸립니다 — 사진 목록을 읽는 질의 자체는 1ms 안쪽이고, 시간은 웹뷰가 첫 화면을 그리는 데 듭니다.`
            : "아직 잰 적 없습니다."
        }
      >
        <span />
      </Row>
      <Row
        label="Photo Desk"
        hint="가져와 고르고, 제자리에 놓습니다. 폰·카메라·NAS 어디서 온 사진이든 여기서 정리해 제 구역으로 보냅니다. 사진은 원래 자리에 그대로 둡니다."
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
      <UpdateRow />
      <Row
        label="지명 자료"
        hint="GeoNames 도시·행정구역 (CC BY 4.0) · Natural Earth 시도 경계 (퍼블릭 도메인) · OpenStreetMap 국가 경계 (ODbL 1.0) · Unicode CLDR 국가 이름. 자세한 고지는 앱 꾸러미 안 Contents/Resources/NOTICE 에 있습니다."
      >
        <span />
      </Row>
    </Section>
  );
}

/**
 * 새 판 확인과 내려받기.
 *
 * 브라우저 대신 앱 안에서 받는 이유: 브라우저로 받으면 격리 표시가 붙어
 * 자체 서명한 앱이 «손상되었다»며 열리지 않는다.
 */
function UpdateRow() {
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [busy, setBusy] = useState<"check" | "download" | null>(null);
  const [percent, setPercent] = useState(0);
  const [auto, setAuto] = useState(true);

  useEffect(() => {
    invoke<string | null>("settings_get", { key: "update.auto" })
      .then((v) => setAuto(v !== "off"))
      .catch(() => {});
    const un = listen<{ percent: number }>("update-progress", (e) =>
      setPercent(e.payload.percent),
    );
    return () => {
      void un.then((f) => f());
    };
  }, []);

  const check = () => {
    setBusy("check");
    invoke<UpdateInfo>("update_check")
      .then((v) => {
        setInfo(v);
        if (!v.newer) toast(`최신판입니다 (${v.current})`);
      })
      .catch((e) => toast(String(e), "drop"))
      .finally(() => setBusy(null));
  };

  const download = () => {
    if (!info?.asset_url || !info.asset_name) return;
    setBusy("download");
    setPercent(0);
    invoke<string>("update_download", {
      assetUrl: info.asset_url,
      assetName: info.asset_name,
    })
      .then(() => toast("받았습니다 — 열린 창에서 앱을 «응용 프로그램»으로 끌어다 놓으세요"))
      .catch((e) => toast(String(e), "drop"))
      .finally(() => setBusy(null));
  };

  const size = info?.asset_size ? ` · ${(info.asset_size / 1048576).toFixed(0)}MB` : "";
  const hint = busy === "download"
    ? `받는 중 ${percent}%${size}`
    : info === null
      ? "GitHub 릴리스에서 새 판이 있는지 살펴봅니다."
      : info.newer
        ? `${info.latest} 이 나왔습니다 (지금 ${info.current})${size}. 받으면 열리는 창에서 앱을 «응용 프로그램»으로 끌어다 놓으세요.`
        : `최신판입니다 (${info.current}).`;

  return (
    <>
      <Row label="업데이트" hint={hint}>
        <>
          {info?.newer && info.asset_url && (
            <Btn
              disabled={busy !== null}
              onClick={download}
              title="앱 안에서 받습니다 — 브라우저로 받으면 «손상됨»으로 열리지 않습니다"
            >
              {busy === "download" ? `${percent}%` : `${info.latest} 받기`}
            </Btn>
          )}
          {info?.newer && (
            <Btn
              disabled={busy !== null}
              onClick={() =>
                invoke("update_open_page", { url: info.page_url }).catch((e) =>
                  toast(String(e), "drop"),
                )
              }
            >
              무엇이 바뀌었나
            </Btn>
          )}
          <Btn disabled={busy !== null} onClick={check}>
            {busy === "check" ? "살피는 중…" : "확인"}
          </Btn>
        </>
      </Row>
      <Row
        label="열 때 자동 확인"
        hint="앱을 열 때 하루 한 번만 살핍니다. 새 판이 있을 때만 알리고, 인터넷이 없으면 조용히 넘어갑니다."
      >
        <Btn
          active={auto}
          onClick={() => {
            const next = !auto;
            setAuto(next);
            invoke("settings_set", { key: "update.auto", value: next ? "on" : "off" }).catch(
              (e) => toast(String(e), "drop"),
            );
          }}
        >
          {auto ? "켬" : "끔"}
        </Btn>
      </Row>
    </>
  );
}
