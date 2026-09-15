# MyAI — Unified Multi-Provider AI Desktop

## Overview
MyAI is a Windows-first Tauri desktop app providing one unified conversation 
interface over Claude Code CLI, Codex CLI, and Gemini CLI.

## Principles
- SQLite is the source of truth for all conversation data
- Provider CLIs are execution backends only — not sources of truth
- Context, skills, and attachments belong to this application
- Provider-specific details stop at the Provider Adapter boundary
- Prefer structured protocols over terminal scraping
- Never silently lose conversation data

## Architecture
- Frontend: React + TypeScript
- Desktop: Tauri v2
- Backend: Rust + Tokio
- Storage: SQLite (rusqlite)
- Location: %LOCALAPPDATA%/MyAI/

## Provider Protocols
- Claude: `claude --print --output-format stream-json`
- Codex: `codex app-server --listen stdio://` (JSON-RPC 2.0)
- Gemini: `gemini --acp` (Agent Client Protocol, JSON-RPC 2.0)

## Coding Standards
- Rust: use `thiserror`, `serde`, `tokio`, `tracing` — no println
- TypeScript: strict mode, React 19, Tauri v2 API
- All IDs are UUIDs
- All timestamps are RFC 3339
