//! search.find — one JSON line in, one JSON line out.
//! Ranks the ce-net threads and the cell/tool registry against a query (BM25, field weighted).
//! The corpus is baked at build time from data/threads.json and data/cells.json.
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::BufRead;

const THREADS: &str = include_str!("../data/threads.json");
const CELLS: &str = include_str!("../data/cells.json");

struct Doc {
    kind: &'static str,
    id: String,
    title: String,
    fields: Vec<(String, f64)>, // text, weight
}

fn tokenize(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() {
            cur.push(c.to_ascii_lowercase());
        } else if !cur.is_empty() {
            if cur.len() > 1 && !is_stop(&cur) {
                out.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
        }
    }
    if cur.len() > 1 && !is_stop(&cur) {
        out.push(cur);
    }
    out
}

fn is_stop(w: &str) -> bool {
    matches!(w, "the" | "a" | "an" | "of" | "to" | "in" | "is" | "for" | "and" | "on" | "with"
        | "that" | "this" | "it" | "be" | "as" | "at" | "or" | "by" | "from" | "was" | "are"
        | "we" | "you" | "i" | "how" | "what" | "can" | "not" | "but" | "all" | "so" | "if")
}

fn load() -> Vec<Doc> {
    let mut docs: Vec<Doc> = Vec::new();
    if let Ok(v) = serde_json::from_str::<Value>(THREADS) {
        if let Some(arr) = v.as_array() {
            for t in arr {
                let tags = t["tags"].as_array().map(|a| {
                    a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" ")
                }).unwrap_or_default();
                docs.push(Doc {
                    kind: "thread",
                    id: t["id"].as_str().unwrap_or("").to_string(),
                    title: t["title"].as_str().unwrap_or("").to_string(),
                    fields: vec![
                        (t["title"].as_str().unwrap_or("").to_string(), 3.0),
                        (tags, 2.0),
                        (t["body"].as_str().unwrap_or("").to_string(), 1.0),
                    ],
                });
            }
        }
    }
    if let Ok(v) = serde_json::from_str::<Value>(CELLS) {
        if let Some(arr) = v.as_array() {
            for t in arr {
                docs.push(Doc {
                    kind: "tool",
                    id: t["name"].as_str().unwrap_or("").to_string(),
                    title: format!("{} — {}", t["name"].as_str().unwrap_or(""), t["title"].as_str().unwrap_or("")),
                    fields: vec![
                        (t["name"].as_str().unwrap_or("").to_string(), 3.0),
                        (t["title"].as_str().unwrap_or("").to_string(), 2.0),
                        (t["text"].as_str().unwrap_or("").to_string(), 1.0),
                    ],
                });
            }
        }
    }
    docs
}

fn main() {
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).unwrap();
    let v: Value = serde_json::from_str(&line).unwrap_or_default();
    let q = v["q"].as_str().unwrap_or("").to_string();
    let kind = v["kind"].as_str().unwrap_or("all").to_string();
    let k = v["k"].as_u64().unwrap_or(8).min(25) as usize;

    let docs = load();
    let terms = tokenize(&q);
    if terms.is_empty() {
        println!("{}", json!({"hits": [], "n": docs.len(), "error": "give q: one or more words"}));
        return;
    }
    let qlow = q.to_ascii_lowercase();

    // term frequencies per doc, weighted
    let mut tfs: Vec<HashMap<String, f64>> = Vec::with_capacity(docs.len());
    let mut lens: Vec<f64> = Vec::with_capacity(docs.len());
    for d in &docs {
        let mut tf: HashMap<String, f64> = HashMap::new();
        let mut len = 0.0;
        for (text, w) in &d.fields {
            let toks = tokenize(text);
            len += toks.len() as f64;
            for t in toks {
                *tf.entry(t).or_insert(0.0) += w;
            }
        }
        tfs.push(tf);
        lens.push(len.max(1.0));
    }
    let n = docs.len() as f64;
    let avgdl = lens.iter().sum::<f64>() / n.max(1.0);
    let mut idf: HashMap<&str, f64> = HashMap::new();
    for t in &terms {
        let df = tfs.iter().filter(|m| m.contains_key(t)).count() as f64;
        idf.insert(t.as_str(), (1.0 + (n - df + 0.5) / (df + 0.5)).ln());
    }

    let mut scored: Vec<(f64, usize, Vec<String>)> = Vec::new();
    for (i, d) in docs.iter().enumerate() {
        if kind == "threads" && d.kind != "thread" { continue; }
        if kind == "tools" && d.kind != "tool" { continue; }
        let mut score = 0.0;
        let mut matched = Vec::new();
        for t in &terms {
            if let Some(&f) = tfs[i].get(t) {
                let k1 = 1.2;
                let b = 0.75;
                let denom = f + k1 * (1.0 - b + b * lens[i] / avgdl);
                score += idf[t.as_str()] * (f * (k1 + 1.0)) / denom;
                matched.push(t.clone());
            }
        }
        // phrase bonus in the title / id
        if d.title.to_ascii_lowercase().contains(&qlow) || d.id.to_ascii_lowercase().contains(&qlow) {
            score += 4.0;
        }
        if score > 0.0 {
            scored.push((score, i, matched));
        }
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let hits: Vec<Value> = scored.iter().take(k).map(|(s, i, m)| {
        let d = &docs[*i];
        json!({
            "kind": d.kind,
            "id": d.id,
            "title": d.title,
            "score": (s * 100.0).round() / 100.0,
            "why": m.join(" "),
            "call": if d.kind == "thread" { json!(format!("thread_read {{\"id\": \"{}\"}}", d.id)) }
                    else { json!(format!("code_call {{\"cell\": \"{}\", \"input\": {{...}}}}", d.id)) }
        })
    }).collect();

    println!("{}", json!({
        "q": q,
        "hits": hits,
        "n": docs.len(),
        "corpus": {"threads": docs.iter().filter(|d| d.kind == "thread").count(),
                   "tools": docs.iter().filter(|d| d.kind == "tool").count()},
        "built": "2026-10-26T08:40+08"
    }));
}
