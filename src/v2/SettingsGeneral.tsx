import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { useData } from "./dataStore";
import { STYLES } from "./gridStyle";
import { useJob } from "./jobStore";
import { usePref } from "./prefs";
import { toast } from "./toastStore";
import { Btn } from "./ui";
import { Section, Row, Select, Toggle } from "./settingsUi";
import type { GeoStats } from "./types";

export function General() {
  return (
    <Section id="general" title="일반">
      <Row
        label="글꼴"
        hint="프리텐다드는 앱에 들어 있어 어디서나 같게 보입니다. 시스템은 SF Pro + Apple SD Gothic Neo."
      >
        <Select
          k="font"
          options={[
            { v: "pretendard", label: "프리텐다드" },
            { v: "system", label: "시스템 글꼴" },
          ]}
        />
      </Row>
      <Row
        label="테마"
        hint="어두운 테마 하나입니다. 밝은 테마는 아직 없습니다."
      >
        <span className="text-[13.5px] text-fg-mute">어두움</span>
      </Row>
      <Row label="이름표" hint="레일·툴바 버튼에 커서를 올리면 뜨는 설명">
        <Toggle k="tooltips" />
      </Row>
      <Row
        label="상태바"
        hint="아래쪽 한 줄 — 지금 보고 있는 사진의 정보와 진행"
      >
        <Toggle k="statusBar" />
      </Row>
    </Section>
  );
}

export function Library({ onRescanAll }: { onRescanAll: () => void }) {
  const scanMsg = useData((s) => s.scanMsg);
  const scanning = useJob((s) => s.job !== null);
  return (
    <Section id="library" title="라이브러리">
      <Row
        label="폴더 감시"
        hint="파인더로 넣거나 지운 사진이 저절로 반영됩니다. 복사가 멎고 1.5초 뒤에 그 폴더만 다시 봅니다."
      >
        <Toggle k="watch" />
      </Row>
      <Row
        label="다시 스캔"
        hint={
          scanMsg ||
          "새로 들어온 사진과 썸네일을 채웁니다. 이미 아는 것은 건너뜁니다."
        }
      >
        <Btn disabled={scanning} onClick={onRescanAll}>
          {scanning ? "스캔 중…" : "전부 다시 스캔"}
        </Btn>
      </Row>
      <Row
        label="영상 촬영일"
        hint="영상은 EXIF가 없어 Spotlight가 준 시각을 썼는데, 색인이 없으면 파일을 복사한 날이 잡힙니다. 파일 안에 박힌 촬영 시각을 다시 읽어 고칩니다."
      >
        <Btn
          disabled={scanning}
          onClick={() =>
            invoke("video_dates_refresh", { libraryId: null }).catch((e) =>
              toast(String(e), "drop"),
            )
          }
        >
          다시 읽기
        </Btn>
      </Row>
    </Section>
  );
}

export function Browse() {
  const [thumbSize, setThumbSize] = usePref("thumbSize");
  const [style] = usePref("gridStyle");
  return (
    <Section id="browse" title="탐색">
      <GeoEndpointRow />
      <GeoRow />
      <Row label="보기 방식" hint="툴바 버튼과 같은 것입니다">
        <Select
          k="gridStyle"
          options={STYLES.map((s) => ({ v: s.v, label: s.label }))}
        />
      </Row>
      <Row label="썸네일 크기">
        <input
          type="range"
          min={100}
          max={320}
          value={thumbSize}
          onChange={(e) => setThumbSize(+e.target.value)}
          aria-label="썸네일 크기"
          className="w-40 accent-accent"
        />
        <span className="w-10 text-right text-[13px] text-fg-mute tabular-nums">
          {thumbSize}
        </span>
      </Row>
      <Row
        label="이름줄 표시"
        hint={
          style === "card"
            ? "사진 아래 이름과 날짜·크기"
            : "카드 보기에서만 뜹니다"
        }
      >
        <Toggle k="caption" />
      </Row>
      <Row
        label="이름줄 첫째 줄"
        hint={style === "card" ? undefined : "카드 보기에서만 뜹니다"}
      >
        <Select
          k="caption1"
          options={[
            { v: "name", label: "파일 이름" },
            { v: "date", label: "촬영일" },
          ]}
        />
      </Row>
      <Row label="이름줄 둘째 줄">
        <Select
          k="caption2"
          options={[
            { v: "dateSize", label: "촬영일 · 크기" },
            { v: "date", label: "촬영일" },
            { v: "size", label: "크기" },
            { v: "camera", label: "카메라" },
            { v: "none", label: "없음" },
          ]}
        />
      </Row>
      <Row label="타일 배지" hint="타일 왼쪽 아래에 붙는 작은 표시">
        <Select
          k="badge"
          options={[
            { v: "none", label: "없음" },
            { v: "format", label: "형식 (RAW·영상)" },
            { v: "iso", label: "ISO" },
            { v: "shutter", label: "셔터 속도" },
            { v: "aperture", label: "조리개" },
            { v: "focal", label: "초점 거리" },
          ]}
        />
      </Row>
      <Row label="두 번 누르면">
        <Select
          k="dblClick"
          options={[
            { v: "viewer", label: "크게 보기" },
            { v: "app", label: "기본 앱으로 열기" },
          ]}
        />
      </Row>
      <Row label="필름스트립 띠 자리">
        <Select
          k="stripPos"
          options={[
            { v: "top", label: "위" },
            { v: "bottom", label: "아래" },
          ]}
        />
      </Row>
    </Section>
  );
}

export function ViewerSection() {
  return (
    <Section id="viewer" title="뷰어">
      <Row label="마우스 휠" hint="확대는 커서 자리를 기준으로 합니다">
        <Select
          k="wheel"
          options={[
            { v: "zoom", label: "확대·축소" },
            { v: "next", label: "앞뒤 사진" },
          ]}
        />
      </Row>
      <Row label="배경">
        <Select
          k="viewerBg"
          options={[
            { v: "canvas", label: "앱 바탕" },
            { v: "black", label: "검정" },
            { v: "gray", label: "회색" },
          ]}
        />
      </Row>
      <Row label="슬라이드쇼 간격">
        <Select
          k="slideshowSec"
          options={[2, 3, 5, 10].map((n) => ({ v: n, label: `${n}초` }))}
        />
      </Row>
      <Row label="영상 자동 재생" hint="크게 보기와 필름스트립에서">
        <Toggle k="autoplay" />
      </Row>
      <Row label="영상 반복">
        <Toggle k="loopVideo" />
      </Row>
    </Section>
  );
}


function geoEndpointProblem(value: string): string | null {
  if (!value) return null;
  try {
    const u = new URL(value);
    if (u.protocol !== "http:" && u.protocol !== "https:")
      return "http 또는 https 주소를 입력해 주세요";
    if (u.hostname.replace(/\.$/, "").toLowerCase() === "nominatim.openstreetmap.org")
      return "OSM 공개 Nominatim은 배치 조회에 사용할 수 없습니다";
    return null;
  } catch {
    return "올바른 지명 서버 URL을 입력해 주세요";
  }
}

/** 지명 서버 — 공개 Nominatim은 배포 앱 전체가 초당 한 건이라 배치에는 쓰지 않는다 */
function GeoEndpointRow() {
  const [url, setUrl] = useState("");
  useEffect(() => {
    invoke<string | null>("settings_get", { key: "geo.endpoint" })
      .then((v) => setUrl(v ?? ""))
      .catch(() => {});
  }, []);
  return (
    <Row
      label="지명 서버"
      hint="자체 Nominatim 또는 배치 조회가 허용된 호환 서비스의 reverse 주소입니다. 사진의 대표 GPS 좌표가 이 서버로 전송됩니다. OSM 공개 Nominatim은 배포 앱의 대량 조회에 사용할 수 없습니다."
    >
      <input
        value={url}
        onChange={(e) => setUrl(e.target.value)}
        onBlur={() => {
          const value = url.trim();
          const problem = geoEndpointProblem(value);
          const save = value
            ? invoke("settings_set", { key: "geo.endpoint", value })
            : invoke("settings_remove", { key: "geo.endpoint" });
          save
            .then(() => {
              useData.getState().bumpGeo();
              toast(
                problem
                  ? `저장했지만 사용할 수 없습니다 — ${problem}`
                  : value
                    ? "지명 서버 주소를 저장했습니다"
                    : "지명 서버 설정을 비웠습니다",
                problem ? "drop" : "ok",
              );
            })
            .catch((e) => toast(String(e), "drop"));
        }}
        placeholder="https://내-서버.example/reverse"
        spellCheck={false}
        className="h-control w-[320px] px-2 rounded-md bg-raised text-[13px] ring-1 ring-line focus:ring-accent outline-none"
      />
    </Row>
  );
}

/** 지명 채우기 — 좌표를 국가·시도·시군구 이름으로. 격자마다 한 번만 묻는다 */
function GeoRow() {
  const [st, setSt] = useState<GeoStats | null>(null);
  const hasJob = useJob((s) => s.job !== null);
  const geoRev = useData((s) => s.geoRev);
  // 처음 열 때, 일이 끝날 때, 서버 설정이 바뀔 때 숫자를 다시 읽는다
  useEffect(() => {
    if (!hasJob)
      void invoke<GeoStats>("geo_stats")
        .then(setSt)
        .catch(() => {});
  }, [hasJob, geoRev]);

  const left = st?.cells_left ?? 0;
  const networkLeft = st?.network_cells_left ?? 0;
  const offlineLeft = st?.offline_cells_left ?? 0;
  const mins = Math.ceil((networkLeft * 1.1) / 60);
  const ready = st?.endpoint_ready ?? false;
  const needsServer = networkLeft > 0;
  const canRun = ready || !needsServer;
  const hint = st
    ? [
        `유효한 좌표가 있는 사진 ${st.with_gps.toLocaleString()}장 중 ${st.named.toLocaleString()}장에 지명이 붙어 있습니다.`,
        st.pending_files > 0
          ? `처리할 사진 ${st.pending_files.toLocaleString()}장 · ${left.toLocaleString()}곳.${
              networkLeft > 0
                ? ` 서버에 물어야 하는 곳 ${networkLeft.toLocaleString()}곳 — 약 ${mins}분입니다.`
                : offlineLeft > 0
                  ? ` ${offlineLeft.toLocaleString()}곳은 서버 없이 처리할 수 있습니다.`
                  : " 모두 저장된 캐시로 바로 채울 수 있습니다."
            }`
          : "조회할 곳은 없습니다.",
        st.unavailable_files > 0
          ? `서버에서 지명을 찾지 못한 사진 ${st.unavailable_files.toLocaleString()}장은 좌표로만 표시됩니다.`
          : "",
        !ready && needsServer ? "새로 조회할 곳이 있어 먼저 위에 지명 서버를 설정해야 합니다." : "",
      ]
        .filter(Boolean)
        .join(" ")
    : "좌표를 국가·시도·시군구 이름으로 바꿔 위치 갈래에서 이름으로 찾습니다.";
  return (
    <Row
      label="지명 채우기"
      hint={hint}
    >
      <>
        <Btn
          disabled={hasJob || left === 0 || !canRun}
          onClick={() => {
            invoke("geo_fill_start", { limit: 100 })
              .then(() => toast("최대 100곳을 채웁니다 — 진행은 위 작업 표시에서"))
              .catch((e) => toast(String(e), "drop"));
          }}
        >
          {!canRun && left > 0
            ? "서버 설정 필요"
            : left > 0
              ? `${Math.min(100, left)}곳 채우기`
              : "처리 완료"}
        </Btn>
        <Btn
          disabled={hasJob || left === 0 || !canRun}
          onClick={() => {
            invoke("geo_fill_start", { limit: null })
              .then(() => toast("남은 곳을 모두 채웁니다 — 멈추면 채운 것은 남습니다"))
              .catch((e) => toast(String(e), "drop"));
          }}
        >
          전부
        </Btn>
      </>
    </Row>
  );
}
