import base64
import concurrent.futures
import json
import os
from pathlib import Path
import time
import urllib.error
import urllib.request


TOOL = {
    "name": "probe_read",
    "description": "Read a file. This probe must not call this tool.",
    "parameters": {
        "type": "object",
        "properties": {"file_path": {"type": "string"}},
        "required": ["file_path"],
        "additionalProperties": False,
    },
}
HISTORY = [
    {"role": "user", "content": "记住：工作目录是 /tmp/example；任务是修改 build.sh；尚未执行。"},
    {"role": "assistant", "content": "已记录，接下来需要查看 build.sh。"},
    {"role": "user", "content": "请总结此前的任务、决定和未完成事项。只输出简短总结，不继续执行任务，不调用工具。"},
]


def probe(kind):
    started = time.monotonic()
    headers = {"Content-Type": "application/json"}
    if kind == "deepseek":
        key = os.environ.get("DEEPSEEK_API_KEY")
        url = "https://api.deepseek.com/chat/completions"
        body = {
            "model": "deepseek-flash", "messages": HISTORY,
            "tools": [{"type": "function", "function": TOOL}],
            "tool_choice": "none", "stream": True,
            "stream_options": {"include_usage": True},
            "reasoning_effort": "low", "max_tokens": 1024,
        }
    else:
        key = os.environ.get("OPENAI_API_KEY")
        url = "https://api.openai.com/v1/responses"
        body = {
            "model": "gpt-5-mini", "input": HISTORY,
            "instructions": "You are a helpful assistant.",
            "tools": [{"type": "function", "strict": False, **TOOL}],
            "tool_choice": "none", "stream": True, "store": False,
            "reasoning": {"effort": "low", "summary": "auto"},
            "include": ["reasoning.encrypted_content"],
            "prompt_cache_key": "micnext-compaction-probe",
            "max_output_tokens": 1024,
        }
        if kind == "chatgpt":
            auth = json.loads((Path.home() / ".codex/auth.json").read_text())
            key = auth["tokens"]["access_token"]
            encoded = key.split(".")[1]
            claims = json.loads(base64.urlsafe_b64decode(encoded + "=" * (-len(encoded) % 4)))
            if claims["exp"] <= time.time():
                return {"provider": kind, "result": "credential_expired"}
            headers["chatgpt-account-id"] = claims["https://api.openai.com/auth"]["chatgpt_account_id"]
            headers["session_id"] = body["prompt_cache_key"]
            url = "https://chatgpt.com/backend-api/codex/responses"
            body["model"] = "gpt-6-luna"
            del body["max_output_tokens"]
    if not key:
        return {"provider": kind, "result": "missing_credential"}
    headers["Authorization"] = "Bearer " + key
    req = urllib.request.Request(url, json.dumps(body).encode(), headers, method="POST")
    out = {"provider": kind, "model": body["model"], "tool_calls": 0, "text": ""}
    try:
        with urllib.request.urlopen(req, timeout=45) as response:
            out["http_status"] = response.status
            for line in response:
                if not line.startswith(b"data:"):
                    continue
                raw = line[5:].strip()
                if raw == b"[DONE]":
                    out["finished"] = True
                    continue
                event = json.loads(raw)
                if kind == "deepseek":
                    for choice in event.get("choices", []):
                        delta = choice.get("delta", {})
                        out["text"] += delta.get("content") or ""
                        out["tool_calls"] += len(delta.get("tool_calls") or [])
                        if choice.get("finish_reason"):
                            out["stop"] = choice["finish_reason"]
                    if event.get("usage"):
                        out["input_tokens"] = event["usage"]["prompt_tokens"]
                else:
                    typ = event.get("type")
                    if typ == "response.output_text.delta":
                        out["text"] += event["delta"]
                    if typ == "response.output_item.added" and event.get("item", {}).get("type") == "function_call":
                        out["tool_calls"] += 1
                    if typ in ("response.completed", "response.incomplete", "response.failed"):
                        out["stop"] = typ
                        out["finished"] = True
                        usage = event.get("response", {}).get("usage")
                        if usage:
                            out["input_tokens"] = usage["input_tokens"]
                    if typ == "error":
                        out["stop"] = "error"
                        out["error_code"] = event.get("code")
    except urllib.error.HTTPError as error:
        out["http_status"] = error.code
        try:
            detail = json.loads(error.read()).get("error", {})
            if isinstance(detail, dict):
                out["error_code"] = detail.get("code")
                out["error_type"] = detail.get("type")
                # Provider error messages may echo input; omit them from this probe.
        except (ValueError, AttributeError):
            pass
    except (urllib.error.URLError, TimeoutError, OSError) as error:
        out["transport_error"] = type(error).__name__
        out["transport_reason"] = type(getattr(error, "reason", None)).__name__
    out["elapsed_seconds"] = round(time.monotonic() - started, 2)
    out["accepted"] = (
        out.get("http_status") == 200
        and out.get("finished", False)
        and out.get("stop") in ("stop", "response.completed")
        and out["tool_calls"] == 0
        and bool(out["text"])
    )
    return out


if __name__ == "__main__":
    with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
        for result in pool.map(probe, ("deepseek", "openai", "chatgpt")):
            print(json.dumps(result, ensure_ascii=False), flush=True)
