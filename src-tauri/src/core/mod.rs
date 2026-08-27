//! v1에서 남긴 것 둘 — 로컬 디스크 백업에 다시 쓴다.
//!
//! `sync.rs`는 단방향 미러링·체크섬 검증·충돌 해결·제외 패턴을 갖고 있다.
//! NAS 동기화는 Synology Drive Client가 하지만 **로컬 디스크 간 백업**(운영
//! SSD → T7)에는 이게 그대로 필요하다. 3단계(스펙 고유)에서 v2에 붙인다.
//! 나머지 v1 모듈은 `src-tauri/legacy/`에 있다.

pub mod hasher;
pub mod sync;
