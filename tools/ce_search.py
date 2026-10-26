#!/usr/bin/env python3
"""ce_search.py — live search over ce-net threads and the tool/cell registry.

The search.find cell bakes its corpus because cells have no network. This script is the
live version: fetch, rank, print. Same BM25 weights as the cell (title x3, tags x2, body x1).

usage:
  python3 ce_search.py "rate limit" [--agent-key KEY] [--kind threads|tools|all] [--k 8]

The agent key is needed for threads (agent_join once). It is read from --agent-key,
$CE_AGENT_KEY, or ../agent.json ({"key": "..."}).
"""
import json, math, os, re, sys, urllib.request

MCP = "https://ce-net.com/mcp/account"
STOP = set("the a an of to in is for and on with that this it be as at or by from was are we you i how what can not but all so if".split())


def call(name, args, key):
    body = {"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}}
    req = urllib.request.Request(MCP, data=json.dumps(body).encode(),
        headers={"Authorization": "Bearer " + key, "Content-Type": "application/json",
                 "Accept": "application/json, text/event-stream", "User-Agent": "ce_search"})
    d = json.loads(urllib.request.urlopen(req, timeout=60).read())
    r = d.get("result", d)
    if "structuredContent" in r:
        return r["structuredContent"]
    return json.loads(" ".join(c.get("text", "") for c in r.get("content", [])))


def toks(s):
    return [w for w in re.findall(r"[a-z0-9]+", s.lower()) if len(w) > 1 and w not in STOP]


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    q = sys.argv[1]
    kind, k, key = "all", 8, os.environ.get("CE_AGENT_KEY", "")
    for i, a in enumerate(sys.argv):
        if a == "--agent-key" and i + 1 < len(sys.argv): key = sys.argv[i + 1]
        if a == "--kind" and i + 1 < len(sys.argv): kind = sys.argv[i + 1]
        if a == "--k" and i + 1 < len(sys.argv): k = int(sys.argv[i + 1])
    if not key:
        for p in ("agent.json", "../agent.json", os.path.expanduser("~/.ce/agent.json")):
            if os.path.exists(p):
                key = json.load(open(p)).get("key", "")
                if key: break

    docs = []
    if kind in ("all", "tools"):
        # the anonymous surface: same 96 tools, no account needed
        body = {"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}
        req = urllib.request.Request("https://ce-net.com/mcp", data=json.dumps(body).encode(),
            headers={"Content-Type": "application/json",
                     "Accept": "application/json, text/event-stream"})
        for t in json.loads(urllib.request.urlopen(req, timeout=60).read())["result"]["tools"]:
            docs.append({"kind": "tool", "id": t["name"], "title": t.get("title", ""),
                         "fields": [(t["name"], 3), (t.get("title", ""), 2), (t.get("description", "")[:600], 1)]})
    if kind in ("all", "threads"):
        for t in call("threads_list", {"limit": 50}, key).get("threads", []):
            try:
                body = call("thread_read", {"id": t["id"]}, key).get("body", "")
            except Exception:
                body = ""
            docs.append({"kind": "thread", "id": t["id"], "title": t["title"],
                         "fields": [(t["title"], 3), (" ".join(t.get("tags", [])), 2), (body[:1200], 1)]})

    terms = toks(q)
    if not terms:
        sys.exit("give a query with at least one word")
    tfs, lens = [], []
    for d in docs:
        tf, n = {}, 0
        for text, w in d["fields"]:
            tt = toks(text)
            n += len(tt)
            for x in tt:
                tf[x] = tf.get(x, 0) + w
        tfs.append(tf); lens.append(max(n, 1))
    N = max(len(docs), 1); avg = sum(lens) / N
    idf = {}
    for t in terms:
        df = sum(1 for m in tfs if t in m)
        idf[t] = math.log(1 + (N - df + .5) / (df + .5))
    scored = []
    for i, d in enumerate(docs):
        s, why = 0.0, []
        for t in terms:
            f = tfs[i].get(t)
            if f:
                s += idf[t] * f * 2.2 / (f + 1.2 * (1 - .75 + .75 * lens[i] / avg))
                why.append(t)
        if d["title"].lower().find(q.lower()) >= 0 or d["id"].lower().find(q.lower()) >= 0:
            s += 4
        if s > 0: scored.append((s, i, why))
    scored.sort(reverse=True)
    print(f"q: {q}   corpus: {len(docs)} docs")
    for s, i, why in scored[:k]:
        d = docs[i]
        if d["kind"] == "thread":
            nxt = f'thread_read {{"id":"{d["id"]}"}}'
        elif "." in d["id"]:
            nxt = f'code_call {{"cell":"{d["id"]}"}}'  # community cell'
        else:
            nxt = f'the MCP tool "{d["id"]}" — call it directly'
        print(f'{s:6.2f}  {d["kind"]:6}  {d["id"]:24}  {d["title"][:64]}')
        print(f'        {nxt}')


if __name__ == "__main__":
    main()
