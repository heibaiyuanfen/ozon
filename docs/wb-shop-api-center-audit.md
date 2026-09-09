# WBerp Module 01 — Repository Audit

## Current Stack

React 19 + TypeScript 5.9 + Vite 7 frontend, Tauri 2 desktop shell, Rust 2021 backend, `rusqlite` bundled SQLite persistence, synchronous `ureq` HTTP client, Windows DPAPI secret protection.

## Existing Shop Model

The shared Ozon registry in `shops.json` is platform-specific and points at per-shop Ozon databases. WB currently has one global `data-next/wb/wb_analytics.db` and only a `store_name` setting. Reusing the Ozon rows would mix platform identities, so Module 01 adds an independent WB shop aggregate while retaining the shared desktop shell.

## Existing Credential Model

WB stores one DPAPI-encrypted token in its settings table, masks it in the UI, but the legacy export command can export plaintext. The new model stores credential versions as rows, never returns the encrypted value, supports rotation/disablement, and records only a masked hint.

## Existing Job/Queue System

There is no durable WB queue. `sync_wb` executes a whole synchronization directly. Module 01 adds persistent `sync_jobs`, `sync_job_runs`, `sync_errors`, active-job deduplication, cancellation of pending jobs, and retry records.

## Existing Audit System

Several feature modules have local action logs, but WB has no unified audit table. Module 01 adds shop-scoped audit events with JSON metadata and explicit token redaction.

## Existing Permission System

The desktop app has no authenticated user/RBAC subsystem. Module 01 introduces a backend role guard with a local `admin` actor as the compatibility default. This creates a real enforcement point without inventing a login flow.

## Existing Marketplace Integration

`wb.rs` calls Content, Statistics, Promotion, Marketplace and Analytics hosts directly. UI calls Tauri commands rather than HTTP, but API concerns and business synchronization remain coupled in one file.

## Recommended Integration Point

Add `wb_shop_center.rs` as the Module 01 application/infrastructure boundary and expose one Tauri RPC dispatcher. Add a dedicated React page in the WB workspace. Later modules consume its `shop_id`, active credential lookup, capabilities and durable sync runs.

## Required Database Changes

Create `data-next/wb-v2/shop_api_center.db` with `wb_organizations`, `wb_shops`, `wb_api_credentials`, `wb_api_capabilities`, `wb_api_health`, `wb_sync_jobs`, `wb_sync_job_runs`, `wb_sync_errors`, and `wb_audit_logs`. Existing WB tables are unchanged for rollback.

## Required API Changes

Add the `wb_shop_center` Tauri command for shop CRUD, credential lifecycle, validation/capability checks, dashboard reads, manual sync creation, retry, cancellation and error resolution.

## Required UI Changes

Add “店铺与 API 中心” to the WB navigation with shop cards, creation form, capability health, resource freshness, sync history, retry/cancel actions, empty/loading/error states and audit history.
