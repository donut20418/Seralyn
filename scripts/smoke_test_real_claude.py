import subprocess
import json
import sqlite3
import os
import sys
import uuid
from datetime import datetime, timezone

DB_PATH = os.path.join(os.environ.get("TEMP", "."), "seralyn_live_smoke_test.db")
if os.path.exists(DB_PATH):
    try:
        os.remove(DB_PATH)
    except Exception:
        pass

print(f"=== SERALYN REAL SMOKE TEST ===")
print(f"Target DB: {DB_PATH}")

# 1. Initialize SQLite Database using Seralyn Migrations
conn = sqlite3.connect(DB_PATH)
cursor = conn.cursor()

MIGRATION_001 = """
CREATE TABLE IF NOT EXISTS conversations (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    archived INTEGER NOT NULL DEFAULT 0,
    metadata_json TEXT
);

CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY NOT NULL,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    parent_id TEXT REFERENCES messages(id),
    role TEXT NOT NULL CHECK(role IN ('user', 'assistant', 'system', 'tool')),
    content TEXT NOT NULL,
    provider TEXT CHECK(provider IN ('claude', 'codex', 'gemini')),
    model TEXT,
    created_at TEXT NOT NULL,
    provider_session_id TEXT REFERENCES provider_sessions(id),
    token_estimate INTEGER,
    metadata_json TEXT
);

CREATE TABLE IF NOT EXISTS provider_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    provider TEXT NOT NULL CHECK(provider IN ('claude', 'codex', 'gemini')),
    provider_session_id TEXT,
    model TEXT,
    created_at TEXT NOT NULL,
    last_used_at TEXT,
    status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active', 'paused', 'closed', 'error')),
    metadata_json TEXT
);
"""

MIGRATION_002 = """
ALTER TABLE messages ADD COLUMN seq INTEGER NOT NULL DEFAULT 0;
ALTER TABLE provider_sessions ADD COLUMN synced_through_seq INTEGER NOT NULL DEFAULT 0;
"""

cursor.executescript(MIGRATION_001)
cursor.executescript(MIGRATION_002)
conn.commit()
print("SQLite Migrations 001 & 002 applied successfully.")

# 2. Create Seralyn Conversation & Session Record
conv_id = str(uuid.uuid4())
now = datetime.now(timezone.utc).isoformat()
cursor.execute(
    "INSERT INTO conversations (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)",
    (conv_id, "Real Claude E2E Smoke Test", now, now)
)

ps_id = str(uuid.uuid4())
cursor.execute(
    "INSERT INTO provider_sessions (id, conversation_id, provider, created_at, status, synced_through_seq) VALUES (?, ?, ?, ?, ?, ?)",
    (ps_id, conv_id, "claude", now, "active", 0)
)
conn.commit()
print(f"Created Seralyn conversation {conv_id} with provider_session record {ps_id}")

# 3. Helper to run a Seralyn turn via Claude CLI
def run_seralyn_turn(turn_num, prompt, resume_sid=None):
    print(f"\n--- [TURN {turn_num}] User Prompt: {prompt} ---")
    
    # Save User message to SQLite
    user_msg_id = str(uuid.uuid4())
    cursor.execute(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE conversation_id = ?",
        (conv_id,)
    )
    user_seq = cursor.fetchone()[0]
    cursor.execute(
        "INSERT INTO messages (id, conversation_id, role, content, provider, created_at, provider_session_id, seq) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        (user_msg_id, conv_id, "user", prompt, "claude", datetime.now(timezone.utc).isoformat(), ps_id, user_seq)
    )
    conn.commit()
    print(f"Saved user message (seq={user_seq})")

    # Build Claude CLI arguments exactly matching src/app/providers/claude/mod.rs
    args = [
        "claude",
        "-p", prompt,
        "--output-format", "stream-json",
        "--verbose",
        "--include-partial-messages"
    ]
    if resume_sid:
        args.extend(["--resume", resume_sid])

    print(f"Spawning: {' '.join(args)}")
    proc = subprocess.Popen(
        args,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1
    )

    # Immediately flush empty stdin to avoid 3s wait warning
    try:
        proc.stdin.write("\n")
        proc.stdin.flush()
        proc.stdin.close()
    except Exception as e:
        print(f"Stdin flush note: {e}")

    detected_sid = None
    streamed_deltas = []
    assistant_full_text = ""

    for line in proc.stdout:
        line = line.strip()
        if not line:
            continue
        try:
            val = json.loads(line)
        except Exception:
            continue

        # 1. Parse system/init (ChatGPT review point 2 & 5)
        if val.get("type") == "system" and val.get("subtype") == "init":
            sid = val.get("session_id")
            if sid:
                detected_sid = sid
                print(f"[EVENT: SessionStarted] Native session ID: {detected_sid}")
                # Persist to SQLite provider_sessions immediately
                cursor.execute(
                    "UPDATE provider_sessions SET provider_session_id = ? WHERE id = ?",
                    (detected_sid, ps_id)
                )
                conn.commit()

        # 2. Parse token stream deltas (ChatGPT review point 1)
        elif val.get("type") == "stream_event":
            ev = val.get("event", {})
            if ev.get("type") == "content_block_delta":
                delta = ev.get("delta", {})
                if delta.get("type") == "text_delta":
                    txt = delta.get("text", "")
                    streamed_deltas.append(txt)
                    print(txt, end="", flush=True)

        # 3. Fallback / assistant message (ChatGPT review point 3)
        elif val.get("type") == "assistant":
            msg = val.get("message", {})
            for content_item in msg.get("content", []):
                if content_item.get("type") == "text":
                    assistant_full_text = content_item.get("text", "")

        # 4. Result event
        elif val.get("type") == "result":
            res_sid = val.get("session_id")
            if res_sid and not detected_sid:
                detected_sid = res_sid
                cursor.execute(
                    "UPDATE provider_sessions SET provider_session_id = ? WHERE id = ?",
                    (detected_sid, ps_id)
                )
                conn.commit()

    proc.wait()
    print("\n[Stream Complete]")

    # Resolve final response text
    final_text = "".join(streamed_deltas) if streamed_deltas else assistant_full_text
    print(f"Final Assistant Text: {final_text.strip()}")
    print(f"Token Deltas Emitted Count: {len(streamed_deltas)}")

    # Save Assistant message to SQLite
    asst_msg_id = str(uuid.uuid4())
    cursor.execute(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE conversation_id = ?",
        (conv_id,)
    )
    asst_seq = cursor.fetchone()[0]
    cursor.execute(
        "INSERT INTO messages (id, conversation_id, role, content, provider, created_at, provider_session_id, seq) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        (asst_msg_id, conv_id, "assistant", final_text.strip(), "claude", datetime.now(timezone.utc).isoformat(), ps_id, asst_seq)
    )
    cursor.execute(
        "UPDATE provider_sessions SET synced_through_seq = ? WHERE id = ?",
        (asst_seq, ps_id)
    )
    conn.commit()
    print(f"Saved assistant message (seq={asst_seq}, synced_through_seq={asst_seq})")

    return detected_sid, final_text.strip(), len(streamed_deltas)

# === RUN TURN 1 ===
sid_turn1, text1, deltas1 = run_seralyn_turn(1, "What is 40 + 2? Reply with ONLY the number.")
assert sid_turn1 is not None, "Turn 1 failed to capture Claude native session ID!"
assert deltas1 > 0, "Turn 1 failed to receive live token deltas! (--include-partial-messages missing?)"
assert "42" in text1, f"Expected 42 in response, got '{text1}'"

# Check DB state after Turn 1
cursor.execute("SELECT provider_session_id, synced_through_seq FROM provider_sessions WHERE id = ?", (ps_id,))
row = cursor.fetchone()
print(f"\n[DB State after Turn 1] provider_session_id: {row[0]}, synced_through_seq: {row[1]}")
assert row[0] == sid_turn1, "Database provider_session_id does not match native session ID!"

# === RUN TURN 2 (SAME SESSION WITHOUT RESTART) ===
sid_turn2, text2, deltas2 = run_seralyn_turn(2, "Multiply that by 2. Reply with ONLY the number.", resume_sid=sid_turn1)
assert deltas2 > 0, "Turn 2 failed to receive live token deltas!"
assert "84" in text2, f"Expected 84 in Turn 2 response, got '{text2}'"
assert sid_turn2 == sid_turn1, f"Session ID changed unexpectedly! {sid_turn2} vs {sid_turn1}"

# === SIMULATE APP CLOSE & RESTART ===
print("\n>>> SIMULATING APP CLOSE & RESTART <<<")
conn.close()

# Re-open fresh database connection (simulating new application launch)
conn = sqlite3.connect(DB_PATH)
cursor = conn.cursor()

# Query DB for active session to resume
cursor.execute("SELECT provider_session_id FROM provider_sessions WHERE conversation_id = ? AND provider = 'claude' AND status = 'active'", (conv_id,))
saved_native_sid = cursor.fetchone()[0]
print(f"Retrieved from persistent DB after restart: {saved_native_sid}")
assert saved_native_sid == sid_turn1, "Persisted native session ID lost after restart!"

# === RUN TURN 3 (NATIVE RESUME TEST AFTER RESTART) ===
sid_turn3, text3, deltas3 = run_seralyn_turn(3, "Multiply that by 10. Reply with ONLY the number.", resume_sid=saved_native_sid)
assert deltas3 > 0, "Turn 3 failed to receive live token deltas!"
assert "840" in text3, f"Expected 840 in Turn 3 resume response, got '{text3}'"
assert sid_turn3 == saved_native_sid, f"Session ID changed unexpectedly across restart! {sid_turn3} vs {saved_native_sid}"

print("\n" + "="*50)
print("SUCCESS: ALL SERALYN PIPELINE CHECKS PASSED 100%!")
print(f"- Live Token Streaming: Confirmed ({deltas1} deltas in T1, {deltas2} deltas in T2, {deltas3} deltas in T3)")
print(f"- System/Init Schema: Confirmed (Captured session ID {sid_turn1})")
print(f"- SQLite Persistence: Confirmed (Messages seq 1-6 stored)")
print(f"- Same-Session Turn 1 -> Turn 2: Confirmed (Claude correctly recalled 42 * 2 = 84)")
print(f"- App Restart & Native Resume (Turn 3): Confirmed (Claude correctly recalled 84 * 10 = 840)")
print("="*50)
conn.close()
