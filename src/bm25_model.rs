use super::lexer::Lexer;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Term frequency saturation — higher k1 = more weight to repeated terms.
/// Standard value is between 1.2 and 2.0.
const K1: f32 = 1.5;

/// Length normalization factor.
/// 0.0 = ignore document length entirely, 1.0 = full normalization.
/// Standard value is 0.75.
const B: f32 = 0.75;

type DocFreq = HashMap<String, usize>;
type TermFreq = HashMap<String, usize>;

#[derive(Deserialize, Serialize)]
pub struct Bm25Doc {
    tf: TermFreq,
    pub len: usize, // total number of tokens in this document
    last_modified: SystemTime,
}

type Docs = HashMap<PathBuf, Bm25Doc>;

#[derive(Default, Deserialize, Serialize)]
pub struct Bm25Model {
    pub docs: Docs,
    pub df: DocFreq,
    pub total_tokens: usize, // sum of all document lengths, used for avgdl
}

impl Bm25Model {
    pub fn avg_doc_len(&self) -> f32 {
        if self.docs.is_empty() {
            return 1.0;
        }
        self.total_tokens as f32 / self.docs.len() as f32
    }

    fn remove_document(&mut self, file_path: &Path) {
        if let Some(doc) = self.docs.remove(file_path) {
            self.total_tokens = self.total_tokens.saturating_sub(doc.len);
            for t in doc.tf.keys() {
                if let Some(f) = self.df.get_mut(t) {
                    *f -= 1;
                }
            }
        }
    }

    pub fn requires_reindexing(&mut self, file_path: &Path, last_modified: SystemTime) -> bool {
        if let Some(doc) = self.docs.get(file_path) {
            return doc.last_modified < last_modified;
        }
        true
    }

    pub fn add_document(
        &mut self,
        file_path: PathBuf,
        last_modified: SystemTime,
        content: &[char],
    ) {
        self.remove_document(&file_path);
        let mut tf = TermFreq::new();
        let mut len = 0usize;
        for t in Lexer::new(content) {
            *tf.entry(t).or_insert(0) += 1;
            len += 1;
        }
        for t in tf.keys() {
            *self.df.entry(t.to_string()).or_insert(0) += 1;
        }
        self.total_tokens += len;
        self.docs.insert(
            file_path,
            Bm25Doc {
                tf,
                len,
                last_modified,
            },
        );
    }

    pub fn search_query(&self, query: &[char]) -> Vec<(PathBuf, f32)> {
        let tokens = Lexer::new(query).collect::<Vec<_>>();
        let n = self.docs.len() as f32;
        let avgdl = self.avg_doc_len();
        let mut result = Vec::new();

        for (path, doc) in &self.docs {
            let mut score = 0f32;
            for token in &tokens {
                let tf = *doc.tf.get(token).unwrap_or(&0) as f32;
                if tf == 0.0 {
                    continue; // token not in document — contributes nothing
                }
                let df = *self.df.get(token).unwrap_or(&1) as f32;

                // Robertson-Spärck Jones IDF — always positive due to +1
                let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();

                // Saturated, length-normalized term frequency
                let tf_norm =
                    (tf * (K1 + 1.0)) / (tf + K1 * (1.0 - B + B * doc.len as f32 / avgdl));

                score += idf * tf_norm;
            }
            if !score.is_nan() {
                result.push((path.clone(), score));
            }
        }

        result.sort_by(|(_, r1), (_, r2)| r1.partial_cmp(r2).unwrap());
        result.reverse();
        result
    }

    pub fn search_query_ids(&self, query: &[char], top_k: usize) -> Vec<String> {
        self.search_query(query)
            .into_iter()
            .take(top_k)
            .map(|(path, _)| path.to_string_lossy().into_owned())
            .collect()
    }
}
