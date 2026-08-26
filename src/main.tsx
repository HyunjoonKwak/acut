import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
// v2 — 재설계된 화면. 기존 App.tsx는 참조용으로 남겨둔다.
import App from './v2/App.tsx'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
