use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// A single document from corpus.jsonl
#[derive(Deserialize, Debug)]
pub struct CorpusDoc {
    #[serde(rename = "_id")]
    pub id: String,
    pub title: Option<String>,
    pub text: String,
}

impl CorpusDoc {
    /// Combine title + text into a single indexable string
    pub fn full_text(&self) -> String {
        match &self.title {
            Some(t) if !t.is_empty() => format!("{} {}", t, self.text),
            _ => self.text.clone(),
        }
    }
}

/// A single query from queries.jsonl
#[derive(Deserialize, Debug)]
pub struct Query {
    #[serde(rename = "_id")]
    pub id: String,
    pub text: String,
}

/// qrels[query_id][doc_id] = relevance_score (only scores > 0 are stored)
pub type Qrels = HashMap<String, HashMap<String, u8>>;

pub fn load_corpus(path: &Path) -> Result<Vec<CorpusDoc>, String> {
    let file = File::open(path).map_err(|e| format!("Cannot open corpus: {e}"))?;
    let reader = BufReader::new(file);
    let mut docs = Vec::new();
    for (i, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let doc: CorpusDoc = serde_json::from_str(&line)
            .map_err(|e| format!("corpus.jsonl line {}: {}", i + 1, e))?;
        docs.push(doc);
    }
    Ok(docs)
}

pub fn load_queries(path: &Path) -> Result<Vec<Query>, String> {
    let file = File::open(path).map_err(|e| format!("Cannot open queries: {e}"))?;
    let reader = BufReader::new(file);
    let mut queries = Vec::new();
    for (i, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let q: Query = serde_json::from_str(&line)
            .map_err(|e| format!("queries.jsonl line {}: {}", i + 1, e))?;
        queries.push(q);
    }
    Ok(queries)
}

pub fn load_qrels(path: &Path) -> Result<Qrels, String> {
    let file = File::open(path).map_err(|e| format!("Cannot open qrels: {e}"))?;
    let reader = BufReader::new(file);
    let mut qrels: Qrels = HashMap::new();
    let mut is_header = true;
    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        if is_header {
            is_header = false;
            continue; // skip "query-id\tcorpus-id\tscore"
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 3 {
            continue;
        }
        let qid   = cols[0].trim().to_string();
        let did   = cols[1].trim().to_string();
        let score: u8 = cols[2].trim().parse().unwrap_or(0);
        // Only store relevant documents (score > 0)
        if score > 0 {
            qrels.entry(qid).or_default().insert(did, score);
        }
    }
    Ok(qrels)
}
