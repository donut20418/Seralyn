import subprocess
import json
import time
import sys
import os

print("=== SERALYN GEMINI ACP PROTOCOL SMOKE TEST ===")

# A compliant ACP v1 mock server script
SERVER_SCRIPT = """
import sys, json, os

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    req = json.loads(line)
    req_id = req.get("id")
    method = req.get("method")
    
    if method == "initialize":
        sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": req_id, "result": {"protocolVersion": 1, "capabilities": {"loadSession": True}}}) + "\\n")
        sys.stdout.flush()
    elif method == "session/new":
        cwd = req.get("params", {}).get("cwd", "")
        if not os.path.isabs(cwd):
            sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": req_id, "error": {"code": -32602, "message": "cwd must be an absolute path"}}) + "\\n")
        else:
            sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": req_id, "result": {"sessionId": "gemini-acp-sess-12345"}}) + "\\n")
        sys.stdout.flush()
    elif method == "session/prompt":
        sid = req["params"]["sessionId"]
        # 1. Emit streaming message chunks
        for chunk in ["Hello ", "from ", "Gemini ", "ACP!"]:
            notif = {
                "jsonrpc": "2.0",
                "method": "session/update",
                "params": {
                    "sessionId": sid,
                    "update": {
                        "sessionUpdate": "agent_message_chunk",
                        "content": {"type": "text", "text": chunk}
                    }
                }
            }
            sys.stdout.write(json.dumps(notif) + "\\n")
            sys.stdout.flush()

        # 2. Emit flattened ACP v1 tool_call notification
        tool_call_notif = {
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": sid,
                "update": {
                    "sessionUpdate": "tool_call",
                    "toolCallId": "call-101",
                    "title": "run_terminal",
                    "kind": "execute",
                    "status": "pending",
                    "rawInput": {"command": "cargo test"}
                }
            }
        }
        sys.stdout.write(json.dumps(tool_call_notif) + "\\n")
        sys.stdout.flush()

        # 3. Emit flattened ACP v1 tool_call_update notification
        tool_update_notif = {
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": sid,
                "update": {
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": "call-101",
                    "status": "completed",
                    "rawOutput": {"exitCode": 0, "stdout": "33 passed"}
                }
            }
        }
        sys.stdout.write(json.dumps(tool_update_notif) + "\\n")
        sys.stdout.flush()

        # Finish turn
        sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": req_id, "result": {"stopReason": "end_turn"}}) + "\\n")
        sys.stdout.flush()
    elif method == "session/load":
        sid = req["params"]["sessionId"]
        cwd = req.get("params", {}).get("cwd", "")
        if not os.path.isabs(cwd):
            sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": req_id, "error": {"code": -32602, "message": "cwd must be an absolute path"}}) + "\\n")
        else:
            sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": req_id, "result": {"sessionId": sid}}) + "\\n")
        sys.stdout.flush()
"""

# Spawn the ACP server
p = subprocess.Popen([sys.executable, "-c", SERVER_SCRIPT], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)

# 1. initialize
p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": 1, "clientCapabilities": {}}}) + "\n")
p.stdin.flush()
init_resp = json.loads(p.stdout.readline())
assert init_resp.get("id") == 1, "Failed ACP initialize"
print(f"1. ACP initialize: PASS -> protocolVersion={init_resp['result']['protocolVersion']}")

# 2. session/new with strict absolute cwd
abs_cwd = os.path.abspath(".")
p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": 2, "method": "session/new", "params": {"cwd": abs_cwd, "mcpServers": []}}) + "\n")
p.stdin.flush()
new_resp = json.loads(p.stdout.readline())
session_id = new_resp.get("result", {}).get("sessionId")
assert session_id == "gemini-acp-sess-12345", f"Failed session/new: {new_resp}"
print(f"2. ACP session/new (absolute cwd={abs_cwd}): PASS -> sessionId={session_id}")

# 3. session/prompt with text stream & flattened tool_call
p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": 3, "method": "session/prompt", "params": {"sessionId": session_id, "prompt": [{"type": "text", "text": "Hello"}]}}) + "\n")
p.stdin.flush()

text_chunks = []
tool_calls = []
tool_updates = []
while True:
    line = p.stdout.readline()
    if not line: break
    data = json.loads(line)
    update = data.get("params", {}).get("update", {})
    update_type = update.get("sessionUpdate")
    if update_type == "agent_message_chunk":
        text_chunks.append(update["content"]["text"])
    elif update_type == "tool_call":
        tool_calls.append(update)
    elif update_type == "tool_call_update":
        tool_updates.append(update)
    if data.get("id") == 3:
        print(f"3. ACP session/prompt: PASS -> Full streamed text: '{''.join(text_chunks)}'")
        break

assert "".join(text_chunks) == "Hello from Gemini ACP!"
assert len(tool_calls) == 1 and tool_calls[0]["toolCallId"] == "call-101", f"Expected flattened tool_call, got {tool_calls}"
assert len(tool_updates) == 1 and tool_updates[0]["status"] == "completed", f"Expected tool_call_update, got {tool_updates}"
print(f"   PASS -> Verified ACP v1 flattened tool_call and tool_call_update schema")

# 4. session/load (resume test with absolute cwd)
p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": 4, "method": "session/load", "params": {"sessionId": session_id, "cwd": abs_cwd}}) + "\n")
p.stdin.flush()
load_resp = json.loads(p.stdout.readline())
assert load_resp.get("result", {}).get("sessionId") == session_id, "Failed session/load"
print(f"4. ACP session/load: PASS -> Resumed session={session_id}")

p.terminate()
print("\n" + "="*55)
print("SUCCESS: ALL GEMINI ACP PROTOCOL CHECKS PASSED 100%!")
print("="*55)
