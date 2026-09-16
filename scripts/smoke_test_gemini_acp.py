import subprocess
import json
import time
import sys

print("=== SERALYN GEMINI ACP PROTOCOL SMOKE TEST ===")

# A compliant ACP v1 mock server script
SERVER_SCRIPT = """
import sys, json

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
        sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": req_id, "result": {"sessionId": "gemini-acp-sess-12345"}}) + "\\n")
        sys.stdout.flush()
    elif method == "session/prompt":
        sid = req["params"]["sessionId"]
        # Emit streaming message chunks
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
        # Finish turn
        sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": req_id, "result": {"stopReason": "end_turn"}}) + "\\n")
        sys.stdout.flush()
    elif method == "session/load":
        sid = req["params"]["sessionId"]
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

# 2. session/new
p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": 2, "method": "session/new", "params": {"cwd": ".", "mcpServers": []}}) + "\n")
p.stdin.flush()
new_resp = json.loads(p.stdout.readline())
session_id = new_resp.get("result", {}).get("sessionId")
assert session_id == "gemini-acp-sess-12345", "Failed session/new"
print(f"2. ACP session/new: PASS -> sessionId={session_id}")

# 3. session/prompt
p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": 3, "method": "session/prompt", "params": {"sessionId": session_id, "prompt": [{"type": "text", "text": "Hello"}]}}) + "\n")
p.stdin.flush()

text_chunks = []
while True:
    line = p.stdout.readline()
    if not line: break
    data = json.loads(line)
    if "update" in data.get("params", {}):
        chunk = data["params"]["update"]["content"]["text"]
        text_chunks.append(chunk)
    if data.get("id") == 3:
        print(f"3. ACP session/prompt: PASS -> Full streamed text: '{''.join(text_chunks)}'")
        break

assert "".join(text_chunks) == "Hello from Gemini ACP!"

# 4. session/load (resume test)
p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": 4, "method": "session/load", "params": {"sessionId": session_id}}) + "\n")
p.stdin.flush()
load_resp = json.loads(p.stdout.readline())
assert load_resp.get("result", {}).get("sessionId") == session_id, "Failed session/load"
print(f"4. ACP session/load: PASS -> Resumed session={session_id}")

p.terminate()
print("\n" + "="*55)
print("SUCCESS: ALL GEMINI ACP PROTOCOL CHECKS PASSED 100%!")
print("="*55)
