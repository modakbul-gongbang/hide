"""A bounded stdio MCP fixture with one inert tool and no filesystem/network."""

import json
import sys

for _ in range(32):
    line = sys.stdin.buffer.readline(65537)
    if not line:
        break
    if len(line) > 65536:
        raise SystemExit(2)
    request = json.loads(line)
    if "id" not in request:
        continue
    method = request.get("method")
    if method == "initialize":
        result = {"protocolVersion": request["params"]["protocolVersion"],
                  "capabilities": {"tools": {}}, "serverInfo": {"name": "live_probe", "version": "1"}}
    elif method == "tools/list":
        result = {"tools": [{"name": "ping", "description": "Return the word probe without any side effect",
                             "inputSchema": {"type": "object", "properties": {}, "additionalProperties": False}}]}
    elif method == "tools/call" and request.get("params", {}).get("name") == "ping":
        result = {"content": [{"type": "text", "text": "probe"}]}
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "error": {"code": -32601, "message": "Unknown method"}}), flush=True)
        continue
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
