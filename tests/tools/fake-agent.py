#!/usr/bin/env python3
"""A stand-in for a coding agent, for testing `omareel agent` and the editor's AI drawer
without spending tokens.

Started the way omareel starts Claude Code (`claude -p --output-format stream-json ...`) it
prints Claude's stream-json events; started as Codex (`codex exec --json ...`) it prints
Codex's. Either way it runs real `omareel` commands in the bundle (its working directory),
as an agent would: describe, then one apply (cuts and a zoom for polish; set-chapters for
chapters; a trim of the start for anything asked), then a summary.

FAKE_AGENT_SLEEP=SECONDS sleeps before the end (with a child in its group, one that leaves
the group, and a daemon re-parented away from it; their pids go to FAKE_AGENT_PIDS), to test
stopping and the sweep at the end. FAKE_AGENT_LOG=FILE records the arguments it was started with.
"""

import json
import shlex
import os
import subprocess
import sys
import time

codex = len(sys.argv) > 1 and sys.argv[1] == "exec"
task = os.environ.get("OMAREEL_AGENT_TASK", "ask")
if os.environ.get("FAKE_AGENT_LOG"):
    with open(os.environ["FAKE_AGENT_LOG"], "w") as log:
        json.dump({"argv": sys.argv, "cwd": os.getcwd(), "path": os.environ.get("PATH", "")}, log)

counter = 0
# The directory omareel lets the agent write images to.
scratch = sys.argv[sys.argv.index("--add-dir") + 1] if "--add-dir" in sys.argv else "/nonexistent"


def emit(event):
    print(json.dumps(event), flush=True)


def shell(command, stdin=None):
    """Runs a command as the agent's Bash tool would; its output and exit status."""
    global counter
    counter += 1
    tool_id = f"toolu_{counter:02d}" if not codex else f"item_{counter}"
    full = command if stdin is None else f"{command} <<'OPS'\n{stdin}\nOPS"
    if codex:
        emit({"type": "item.started", "item": {"id": tool_id, "type": "command_execution",
              "command": f"bash -lc '{full}'", "aggregated_output": "", "exit_code": None, "status": "in_progress"}})
    else:
        emit({"type": "assistant", "message": {"role": "assistant", "content": [
            {"type": "tool_use", "id": tool_id, "name": "Bash", "input": {"command": full}}]},
            "parent_tool_use_id": None})
    result = subprocess.run(command, shell=True, input=stdin, capture_output=True, text=True)
    output = result.stdout + result.stderr
    if codex:
        emit({"type": "item.completed", "item": {"id": tool_id, "type": "command_execution", "command": f"bash -lc '{full}'",
              "aggregated_output": output, "exit_code": result.returncode,
              "status": "completed" if result.returncode == 0 else "failed"}})
    else:
        emit({"type": "user", "message": {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": tool_id, "content": output, "is_error": result.returncode != 0}]},
            "parent_tool_use_id": None})
    return output, result.returncode


def say(text):
    global counter
    counter += 1
    if codex:
        emit({"type": "item.completed", "item": {"id": f"item_{counter}", "type": "agent_message", "text": text}})
    else:
        emit({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": text}]},
              "parent_tool_use_id": None})


started = time.time()
if codex:
    emit({"type": "thread.started", "thread_id": "fake-thread"})
    emit({"type": "turn.started"})
else:
    emit({"type": "system", "subtype": "init", "model": "fake-model", "tools": ["Bash", "Read"], "session_id": "fake"})

say("Reading the timeline.")
described, status = shell("omareel describe .")
summary = json.loads(described) if status == 0 else {}
duration = summary.get("output", {}).get("duration", 0)
source_duration = summary.get("source", {}).get("duration", 30)
if task == "polish":
    ops = [{"op": "delete", "ranges": [[0, 1.5], [9, 15]], "timeline": "source"},
           {"op": "set", "path": "keystrokes.enabled", "value": True}]
    shell("omareel apply . " + shlex.quote(json.dumps(ops)))
    shell(f"omareel frame . --count 4 --sheet -o {scratch}/sheet.jpg")
    text = f"Cut the first 1.5 s and the idle stretch at 7.5–13.5 s; the video was {duration} s."
elif task == "chapters":
    ops = [{"op": "set-chapters", "timeline": "source", "title": "A synthetic test", "description": "Test bars and a tone.",
            "chapters": [{"at": 0, "title": "Start"}, {"at": round(source_duration * 0.55, 2), "title": "Clicking around"}]}]
    shell("omareel apply . " + shlex.quote(json.dumps(ops)))
    text = "Wrote two chapters, a title and a description."
else:
    shell("omareel apply . " + shlex.quote(json.dumps([{"op": "delete", "from": 0, "to": 1}])))
    text = "Cut the first second."

if os.environ.get("FAKE_AGENT_SLEEP"):
    pids = os.environ.get("FAKE_AGENT_PIDS", "/dev/null")
    child = subprocess.Popen(["sleep", "300"])
    loner = subprocess.Popen(["sleep", "300"], start_new_session=True)
    # A daemon: its parent exits, so it is re-parented away from the agent.
    daemon = subprocess.run(["setsid", "sh", "-c", "sleep 300 >/dev/null 2>&1 & echo $!"], capture_output=True, text=True).stdout.strip()
    with open(pids + ".tmp", "w") as out:
        out.write(f"{os.getpid()} {child.pid} {loner.pid} {daemon}\n")
    os.rename(pids + ".tmp", pids)
    time.sleep(float(os.environ["FAKE_AGENT_SLEEP"]))

say(text)
if codex:
    emit({"type": "turn.completed", "usage": {"input_tokens": 1200, "cached_input_tokens": 0, "output_tokens": 80}})
else:
    emit({"type": "result", "subtype": "success", "is_error": False, "duration_ms": round((time.time() - started) * 1000),
          "num_turns": counter, "result": text, "total_cost_usd": 0.0123,
          "usage": {"input_tokens": 1200, "output_tokens": 80}})
