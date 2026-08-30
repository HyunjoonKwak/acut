import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { fmtBytes } from "./format";
import { useJob } from "./jobStore";
import { etaSec, fmtEta, pushSample, rateOf, type Sample } from "./rate";
import { toast } from "./toastStore";
import { Btn } from "./ui";
import { Section, Row } from "./settingsUi";

type AiStatus = {
  model_present: boolean;
  model_bytes: number;
  embedded: number;
  total: number;
  running: boolean;
  text_present: boolean;
  text_bytes: number;
  face_present: boolean;
  face_bytes: number;
  faces_done: number;
  faces_total: number;
  faces: number;
  persons: number;
};

export function Ai() {
  const [st, setSt] = useState<AiStatus | null>(null);
  // 개수 표본 — DB가 진실이다. 이벤트는 화면이 새로 뜨면 놓치지만 개수는 안 놓친다.
  const [samples, setSamples] = useState<Sample[]>([]);
  const job = useJob((s) => s.job);
  const busy = job !== null;
  // 누른 직후 — 개수가 움직이기 전 몇 초 동안도 «만드는 중»으로
  const [kicked, setKicked] = useState(0);
  // 마지막으로 센 시각 — 그리는 동안 시계를 읽지 않는다
  const [now, setNow] = useState(0);
  const reload = useCallback(() => {
    invoke<AiStatus>("ai_status")
      .then((s) => {
        const t = Date.now();
        setSt(s);
        setNow(t);
        setSamples((prev) => pushSample(prev, { t, n: s.embedded }));
      })
      .catch(() => setSt(null));
  }, []);
  // 3초마다 다시 센다 — 개수가 오르면 도는 것이고, 안 오르면 멎은 것이다
  useEffect(() => {
    reload();
    const t = setInterval(reload, 3_000);
    return () => clearInterval(t);
  }, [reload]);

  const download = async (which: "vision" | "text" | "face") => {
    try {
      await invoke("ai_model_download", { which });
    } catch (e) {
      toast(String(e), "drop");
    }
  };
  const embed = async () => {
    try {
      await invoke("ai_embed_start");
      setKicked(Date.now());
    } catch (e) {
      toast(String(e), "drop");
    }
  };
  const stop = () => invoke("scan_cancel").catch(() => {});

  const rate = rateOf(samples, now);
  const growing = rate !== null && rate > 0;
  // 뒷단이 «도는 중»이라 하면 그게 진실. 개수가 오르는 것과 방금 누른 것은 거들 뿐.
  const embedding =
    (st?.running ?? false) ||
    growing ||
    job?.label === "AI 벡터" ||
    now - kicked < 15_000;
  const done = st?.embedded ?? 0;
  const total = st?.total ?? 0;
  const left = Math.max(0, total - done);
  const hint = !st
    ? "…"
    : embedding
      ? `${done.toLocaleString()} / ${total.toLocaleString()}장 — 만드는 중. 남은 ${left.toLocaleString()}장${growing ? `, 초당 ${Math.round(rate)}장이면 ${fmtEta(etaSec(left, rate))}` : ", 속도 재는 중"}. 멈춰도 한 것은 남습니다.`
      : left > 0
        ? `${done.toLocaleString()} / ${total.toLocaleString()}장 — 남은 ${left.toLocaleString()}장. 하다 말아도 한 것은 남습니다.`
        : `${done.toLocaleString()}장 전부 있습니다. 새로 들어온 사진만 더 만들면 됩니다.`;

  return (
    <Section id="ai" title="AI">
      <Row
        label="모델"
        hint={
          st?.model_present
            ? "CLIP ViT-B/32 — 사진을 512개 숫자로 요약합니다. 전부 이 맥 안에서 돕니다."
            : `CLIP ViT-B/32 (${st ? fmtBytes(st.model_bytes) : "…"}) — 한 번만 받습니다. 그 뒤로는 네트워크가 필요 없습니다.`
        }
      >
        {st?.model_present ? (
          <span className="text-[12.5px] text-keep">받아 둠</span>
        ) : (
          <Btn tone="accent" disabled={busy} onClick={() => download("vision")}>
            받기
          </Btn>
        )}
      </Row>
      <Row
        label="글로 찾기 모델"
        hint={
          st?.text_present
            ? "다국어 텍스트 모델 — «바닷가 강아지»처럼 글로 찾습니다. 찾기 갈래의 «AI로 찾기»에 씁니다."
            : `다국어 텍스트 모델 (${st ? fmtBytes(st.text_bytes) : "…"}) — 한국어·영어로 사진을 찾습니다. 사진 벡터가 있어야 씁니다.`
        }
      >
        {st?.text_present ? (
          <span className="text-[12.5px] text-keep">받아 둠</span>
        ) : (
          <Btn tone="accent" disabled={busy} onClick={() => download("text")}>
            받기
          </Btn>
        )}
      </Row>
      <Row label="사진 벡터" hint={hint}>
        {embedding ? (
          <Btn tone="drop" onClick={stop}>
            멈추기
          </Btn>
        ) : (
          <Btn
            tone="accent"
            disabled={busy || !st?.model_present || left === 0}
            onClick={embed}
          >
            벡터 만들기
          </Btn>
        )}
      </Row>
      {embedding && total > 0 && (
        <div className="px-4 pb-3">
          <div className="h-1 rounded-full bg-raised overflow-hidden">
            <div
              className="h-full bg-accent transition-[width] duration-300"
              style={{ width: `${Math.min(100, (done / total) * 100)}%` }}
            />
          </div>
        </div>
      )}
      <Row
        label="얼굴 모델"
        hint={
          st?.face_present
            ? "YuNet(찾기)·SFace(알아보기) — OpenCV zoo, Apache-2.0. 전부 이 맥 안에서 돕니다."
            : `YuNet·SFace (${st ? fmtBytes(st.face_bytes) : "…"}) — 얼굴을 찾아 사람으로 묶습니다.`
        }
      >
        {st?.face_present ? (
          <span className="text-[12.5px] text-keep">받아 둠</span>
        ) : (
          <Btn tone="accent" disabled={busy} onClick={() => download("face")}>
            받기
          </Btn>
        )}
      </Row>
      <Row
        label="얼굴 찾기"
        hint={
          !st
            ? "…"
            : st.faces_total === 0
              ? "썸네일이 있어야 찾습니다."
              : `${st.faces_done.toLocaleString()} / ${st.faces_total.toLocaleString()}장에서 얼굴 ${st.faces.toLocaleString()}개, ${st.persons.toLocaleString()}명. 왼쪽 「사람」 갈래에서 이름을 붙이고 합칩니다.`
        }
      >
        <Btn
          tone="accent"
          disabled={
            busy ||
            !st?.face_present ||
            (st?.faces_total ?? 0) - (st?.faces_done ?? 0) === 0
          }
          onClick={() =>
            invoke("ai_faces_start").catch((e) => toast(String(e), "drop"))
          }
        >
          얼굴 찾기
        </Btn>
      </Row>
      <Row
        label="비슷한 사진 찾기"
        hint="사진을 우클릭해 「비슷한 사진 찾기」. 벡터가 있는 사진끼리 비교합니다."
      >
        <span />
      </Row>
    </Section>
  );
}

