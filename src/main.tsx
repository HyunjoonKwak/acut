import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./index.css";
// v2 — 재설계된 화면. 기존 App.tsx는 참조용으로 남겨둔다.
import App from "./v2/App.tsx";
import { ConfirmProvider } from "./v2/confirm.tsx";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    {/* 물음 상자는 App 위에 있어야 App 안에서도 부를 수 있다 */}
    <ConfirmProvider>
      <App />
    </ConfirmProvider>
  </StrictMode>,
);
