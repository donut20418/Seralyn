import subprocess
import json
import time
import os
import sys

print("=== SERALYN REAL GEMINI CLI ACP 3-TURN SMOKE TEST ===")

# Detect gemini executable
cmd = "gemini.cmd" if os.name == "nt" else "gemini"
print(f"Target CLI: {cmd}")

env = os.environ.copy()
api_key = env.get("GEMINI_API_KEY")

# Check settings.json for user's configured auth method
settings_path = os.path.expanduser("~/.gemini/settings.json")
selected_method = "oauth-personal"
if os.path.exists(settings_path):
    try:
        with open(settings_path, "r", encoding="utf-8") as f:
            s = json.load(f)
            selected_method = s.get("security", {}).get("auth", {}).get("selectedType", "oauth-personal")
    except Exception:
        pass

if api_key:
    print("Authentication mode: Optional GEMINI_API_KEY override detected.")
    auth_params = {
        "methodId": "gemini-api-key",
        "_meta": {
            "api-key": api_key
        }
    }
else:
    print(f"Authentication mode: Using configured CLI credentials ('{selected_method}'). No explicit authenticate call needed.")
    auth_params = None

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(line_buffering=True)

def read_until_response(proc, req_id, timeout_sec=30):
    t0 = time.time()
    resp = None
    accumulated_text = []
    events = []
    
    while time.time() - t0 < timeout_sec:
        line = proc.stdout.readline()
        if not line:
            if proc.poll() is not None:
                break
            time.sleep(0.05)
            continue
            
        line_str = line.strip()
        idx = line_str.find("{")
        if idx == -1:
            continue
            
        try:
            data = json.loads(line_str[idx:])
        except json.JSONDecodeError:
            continue
            
        # Collect streaming notifications
        if data.get("method") == "session/update":
            events.append("session/update")
            params = data.get("params", {})
            update = params.get("update", {})
            if update.get("sessionUpdate") == "agent_message_chunk":
                content = update.get("content", {})
                if isinstance(content, dict) and "text" in content:
                    accumulated_text.append(content["text"])
                elif isinstance(content, str):
                    accumulated_text.append(content)
            elif "content" in params and isinstance(params["content"], str):
                accumulated_text.append(params["content"])
                
        if data.get("id") == req_id:
            resp = data
            break
                
    return resp, "".join(accumulated_text), events

def send_msg(proc, msg):
    raw = json.dumps(msg) + "\n"
    proc.stdin.write(raw)
    proc.stdin.flush()

CODEWORD = "SERALYN-8427"
cwd = os.path.abspath(".")

model = env.get("GEMINI_MODEL", "gemini-3.1-flash-lite")
print(f"Target Model: {model}")

# ==========================================
# PROCESS 1: Session Creation & Turn 1 & 2
# ==========================================
print("\n--- [Process 1] Initializing & Creating Session ---")
p1 = subprocess.Popen(
    [cmd, "--acp", "--skip-trust", "-m", model],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
    bufsize=1,
    env=env
)

# 1. Initialize
print("1. Sending 'initialize'...")
send_msg(p1, {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "initialize",
    "params": {
        "protocolVersion": 1,
        "clientCapabilities": {}
    }
})
init_resp, _, _ = read_until_response(p1, 1, timeout_sec=10)
assert init_resp is not None, "Failed to get initialize response from Process 1"
assert "error" not in init_resp, f"Initialize returned error: {init_resp.get('error')}"
res = init_resp.get("result", {})
agent_info = res.get("agentInfo", {})
print(f"   PASS -> Agent: {agent_info.get('name')} v{agent_info.get('version')}, protocol: {res.get('protocolVersion')}")

# 2. Authenticate (optional override)
if auth_params:
    print(f"2. Sending 'authenticate' ({auth_params['methodId']})...")
    send_msg(p1, {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "authenticate",
        "params": auth_params
    })
    auth_resp, _, _ = read_until_response(p1, 2, timeout_sec=10)
    assert auth_resp is not None, "Failed to get authenticate response from Process 1"
    assert "error" not in auth_resp, f"Authenticate returned error: {auth_resp.get('error')}"
    print("   PASS -> Authentication verified successfully by Gemini ACP.")
else:
    print("2. Skipping explicit 'authenticate' (Gemini CLI will use configured credentials)...")

# 3. Session New
print("3. Sending 'session/new' with canonical cwd...")
send_msg(p1, {
    "jsonrpc": "2.0",
    "id": 3,
    "method": "session/new",
    "params": {
        "cwd": cwd,
        "mcpServers": []
    }
})
new_resp, _, _ = read_until_response(p1, 3, timeout_sec=10)
assert new_resp is not None, "Failed to get session/new response from Process 1"
assert "error" not in new_resp, f"session/new returned error: {new_resp.get('error')}"
session_id = new_resp.get("result", {}).get("sessionId")
assert session_id, f"No sessionId returned: {new_resp}"
print(f"   PASS -> Session created: {session_id}")

# 4. Turn 1 (Set codeword)
print(f"\n4. [Turn 1] Sending prompt with codeword: {CODEWORD}...")
send_msg(p1, {
    "jsonrpc": "2.0",
    "id": 4,
    "method": "session/prompt",
    "params": {
        "sessionId": session_id,
        "prompt": [{"type": "text", "text": f"Please remember this secret codeword: {CODEWORD}. Respond only with: 'Codeword acknowledged'."}]
    }
})
prompt_resp1, text1, _ = read_until_response(p1, 4, timeout_sec=40)
assert prompt_resp1 is not None, "Timed out waiting for Turn 1 prompt response"
assert "error" not in prompt_resp1, f"Turn 1 prompt returned error: {prompt_resp1.get('error')}"
print(f"   PASS -> Turn 1 completed. Model output: {text1.strip()[:100]}")

# 5. Turn 2 (Recall codeword in same session)
print("\n5. [Turn 2] Asking for codeword in current session...")
send_msg(p1, {
    "jsonrpc": "2.0",
    "id": 5,
    "method": "session/prompt",
    "params": {
        "sessionId": session_id,
        "prompt": [{"type": "text", "text": "What is the secret codeword I asked you to remember?"}]
    }
})
prompt_resp2, text2, _ = read_until_response(p1, 5, timeout_sec=40)
assert prompt_resp2 is not None, "Timed out waiting for Turn 2 prompt response"
assert "error" not in prompt_resp2, f"Turn 2 prompt returned error: {prompt_resp2.get('error')}"
print(f"   PASS -> Turn 2 completed. Model output: {text2.strip()}")
assert CODEWORD in text2, f"Turn 2 failed! Expected '{CODEWORD}' in model response, got: '{text2}'"
print(f"   PASS -> Native memory verified in same session: '{CODEWORD}' found in response!")

# Terminate Process 1
print("\n--- Terminating Process 1 (simulating process exit/crash) ---")
p1.stdin.close()
try:
    p1.wait(timeout=6)
except subprocess.TimeoutExpired:
    p1.terminate()
    p1.wait()

# Gemini CLI partitions session files by minute: session-YYYY-MM-DDTHH-mm-shortId.jsonl.
# To prevent Process 2 startup initializer from colliding with Process 1's file in the same minute,
# ensure Process 2 spawns across the minute boundary.
p1_min = time.gmtime().tm_min
while time.gmtime().tm_min == p1_min:
    time.sleep(1)

# ==========================================
# PROCESS 2: Session Resumption & Turn 3
# ==========================================
print("\n--- [Process 2] Spawning new process to verify Cross-Process session/load ---")
p2 = subprocess.Popen(
    [cmd, "--acp", "--skip-trust", "-m", model],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
    bufsize=1,
    env=env
)

print("6. Sending 'initialize' to Process 2...")
send_msg(p2, {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "initialize",
    "params": {
        "protocolVersion": 1,
        "clientCapabilities": {}
    }
})
init_resp2, _, _ = read_until_response(p2, 1, timeout_sec=10)
assert init_resp2 is not None and "error" not in init_resp2, f"P2 initialize failed: {init_resp2}"

# 7. Authenticate Process 2 (optional override)
if auth_params:
    print(f"7. Sending 'authenticate' ({auth_params['methodId']}) to Process 2...")
    send_msg(p2, {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "authenticate",
        "params": auth_params
    })
    auth_resp2, _, _ = read_until_response(p2, 2, timeout_sec=10)
    assert auth_resp2 is not None and "error" not in auth_resp2, f"P2 authenticate failed: {auth_resp2}"
else:
    print("7. Skipping explicit 'authenticate' in Process 2...")

print(f"8. Sending 'session/load' for existing sessionId: {session_id}...")
send_msg(p2, {
    "jsonrpc": "2.0",
    "id": 3,
    "method": "session/load",
    "params": {
        "sessionId": session_id,
        "cwd": cwd,
        "mcpServers": []
    }
})
load_resp, _, _ = read_until_response(p2, 3, timeout_sec=15)
assert load_resp is not None, "Timed out waiting for session/load response"
assert "error" not in load_resp, f"session/load returned error: {load_resp.get('error')}"
print("   PASS -> Native session successfully restored from disk via session/load!")

print(f"\n9. [Turn 3] Asking for codeword in resumed session (Process 2)...")
send_msg(p2, {
    "jsonrpc": "2.0",
    "id": 4,
    "method": "session/prompt",
    "params": {
        "sessionId": session_id,
        "prompt": [{"type": "text", "text": "What was the secret codeword we established?"}]
    }
})
prompt_resp3, text3, _ = read_until_response(p2, 4, timeout_sec=40)
assert prompt_resp3 is not None, "Timed out waiting for Turn 3 prompt response"
assert "error" not in prompt_resp3, f"Turn 3 prompt returned error: {prompt_resp3.get('error')}"
print(f"   PASS -> Turn 3 completed. Model output: {text3.strip()}")
assert CODEWORD in text3, f"Turn 3 resume test failed! Expected '{CODEWORD}' in model response, got: '{text3}'"
print(f"   PASS -> Native cross-process session resumption verified: '{CODEWORD}' preserved across restart!")

p2.stdin.close()
p2.terminate()
p2.wait()

print("\n" + "="*70)
print("SUCCESS: REAL GEMINI ACP 3-TURN CODEWORD RESUME SMOKE TEST PASSED 100%!")
print("="*70)
