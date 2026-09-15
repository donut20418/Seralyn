# MyAI — Unified Multi-Provider AI Desktop

A Windows-first Tauri desktop application providing one unified conversation interface over multiple AI coding CLIs.

## Supported Providers

| Provider | Protocol | CLI Command |
|----------|----------|-------------|
| **Claude Code** | `stream-json` (NDJSON) | `claude --print --output-format stream-json` |
| **OpenAI Codex** | JSON-RPC 2.0 via app-server | `codex app-server --listen stdio://` |
| **Google Gemini** | Agent Client Protocol (ACP) | `gemini --acp` |

## Key Principles

- **SQLite is the source of truth** — native CLI sessions are optional accelerators
- **Context belongs to this app** — not to CLAUDE.md, AGENTS.md, or GEMINI.md
- **Skills belong to this app** — one canonical copy, mirrored to provider locations
- **Provider-specific details stop at the adapter boundary**
- **Prefer structured protocols over terminal scraping**

## Technology Stack

- **Frontend**: React 19 + TypeScript
- **Desktop**: Tauri v2 (WebView2 on Windows)
- **Backend**: Rust + Tokio
- **Storage**: SQLite (rusqlite, WAL mode)
- **No Electron**

## Prerequisites

- [Rust](https://rustup.rs/) (1.77+)
- [Node.js](https://nodejs.org/) (18+)
- At least one of:
  - [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code)
  - [Codex CLI](https://github.com/openai/codex)
  - [Gemini CLI](https://github.com/google-gemini/gemini-cli)

## Getting Started

```bash
# Install dependencies
npm install

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

## Data Storage

Persistent data is stored at `%LOCALAPPDATA%/MyAI/`:

```
%LOCALAPPDATA%/MyAI/
├── data/myai.db          # SQLite database
├── attachments/          # Chat attachments (Phase 2)
├── skills/               # Canonical skills (Phase 4)
├── prompts/              # System prompts (Phase 4)
├── cache/                # Token estimates, summaries
├── logs/                 # Application logs
└── temp/                 # Disposable temporary files
```

## Architecture

```
Desktop UI (React)
       │
  Tauri IPC
       │
Conversation Core
       │
  ┌────┴────┐
  │         │
Context  Provider
Engine   Manager
           │
    ┌──────┼──────┐
    │      │      │
  Claude Codex  Gemini
  Adapter Adapter Adapter
    │      │      │
  claude  codex  gemini
   CLI    CLI    CLI
```

## Project Structure

```
src-tauri/src/
├── main.rs                    # Tauri entry + IPC commands
├── lib.rs                     # Library root
└── app/
    ├── mod.rs
    ├── error.rs               # Error types
    ├── db/                    # SQLite layer
    ├── conversation/          # Conversation manager + context
    ├── providers/             # Provider trait + adapters
    │   ├── claude/
    │   ├── codex/
    │   └── gemini/
    ├── process/               # Subprocess management
    ├── events/                # Normalized event system
    └── tokens/                # Token estimation

src/                           # React frontend
├── components/
│   ├── chat/
│   ├── sidebar/
│   ├── providers/
│   └── common/
├── hooks/
├── lib/
└── styles/
```

## Development Phases

- **Phase 1** ✅ Core — Conversations, providers, event streaming, basic UI
- **Phase 2** — Attachments, token tracking, Context Inspector, session resume
- **Phase 3** — Context Engine, compaction, handoff
- **Phase 4** — Skill Registry, SkillBridge, project instructions, MCP
- **Phase 5** — Branching, compare-model, multi-agent, advanced search

## License

Private — All rights reserved.
