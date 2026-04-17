use std::collections::HashMap;

/// qrels[query_id] = set of relevant doc_ids (only relevant ones are stored)
pub type Qrels = HashMap<String, HashMap<String, u8>>;

/// Evaluation results: list of (query_id, ranked list of doc_ids)
pub type RankedResults = Vec<(String, Vec<String>)>;

// ─────────────────────────────────────────────────────────────────────────────
// MRR@k  —  Mean Reciprocal Rank
//
// For each query, find the rank of the FIRST relevant document in the top-k.
// Take the reciprocal (1/rank). Average across all queries.
//
// Example: relevant doc is at rank 3 → reciprocal = 1/3 = 0.333
// ─────────────────────────────────────────────────────────────────────────────
pub fn mrr_at_k(results: &RankedResults, qrels: &Qrels, k: usize) -> f64 {
    let mut total = 0.0f64;
    let mut evaluated = 0usize;

    for (qid, ranked_docs) in results {
        let rel_docs = match qrels.get(qid) {
            Some(r) if !r.is_empty() => r,
            _ => continue,
        };
        evaluated += 1;
        for (rank, doc_id) in ranked_docs.iter().take(k).enumerate() {
            if rel_docs.contains_key(doc_id) {
                total += 1.0 / (rank + 1) as f64;
                break; // only the first relevant hit counts
            }
        }
    }
    if evaluated == 0 { 0.0 } else { total / evaluated as f64 }
}

// ─────────────────────────────────────────────────────────────────────────────
// NDCG@k  —  Normalized Discounted Cumulative Gain
//
// Rewards highly relevant documents retrieved at higher ranks.
// DCG  = Σ  rel_i / log2(i + 2)   for i in 0..k
// IDCG = DCG of the ideal (perfect) ranking
// NDCG = DCG / IDCG
// ─────────────────────────────────────────────────────────────────────────────
pub fn ndcg_at_k(results: &RankedResults, qrels: &Qrels, k: usize) -> f64 {
    let mut total = 0.0f64;
    let mut evaluated = 0usize;

    for (qid, ranked_docs) in results {
        let rel_docs = match qrels.get(qid) {
            Some(r) if !r.is_empty() => r,
            _ => continue,
        };
        evaluated += 1;

        // Actual DCG
        let dcg: f64 = ranked_docs.iter().take(k).enumerate()
            .map(|(i, did)| {
                let rel = *rel_docs.get(did).unwrap_or(&0) as f64;
                if rel > 0.0 { rel / (i as f64 + 2.0).log2() } else { 0.0 }
            })
            .sum();

        // Ideal DCG: sort relevance scores descending
        let mut ideal_rels: Vec<f64> = rel_docs.values().map(|&v| v as f64).collect();
        ideal_rels.sort_by(|a, b| b.partial_cmp(a).unwrap());
        let idcg: f64 = ideal_rels.iter().take(k).enumerate()
            .map(|(i, &rel)| rel / (i as f64 + 2.0).log2())
            .sum();

        if idcg > 0.0 {
            total += dcg / idcg;
        }
    }
    if evaluated == 0 { 0.0 } else { total / evaluated as f64 }
}

// ─────────────────────────────────────────────────────────────────────────────
// Recall@k  —  Fraction of ALL relevant docs retrieved in top-k
//
// Recall@k = |{relevant} ∩ {top-k}| / |{relevant}|
// ─────────────────────────────────────────────────────────────────────────────
pub fn recall_at_k(results: &RankedResults, qrels: &Qrels, k: usize) -> f64 {
    let mut total = 0.0f64;
    let mut evaluated = 0usize;

    for (qid, ranked_docs) in results {
        let rel_docs = match qrels.get(qid) {
            Some(r) if !r.is_empty() => r,
            _ => continue,
        };
        evaluated += 1;
        let n_relevant = rel_docs.len();
        let hits = ranked_docs.iter().take(k)
            .filter(|did| rel_docs.contains_key(*did))
            .count();
        total += hits as f64 / n_relevant as f64;
    }
    if evaluated == 0 { 0.0 } else { total / evaluated as f64 }
}

// ─────────────────────────────────────────────────────────────────────────────
// Precision@k  —  Fraction of top-k results that are relevant
//
// Precision@k = |{relevant} ∩ {top-k}| / k
// ─────────────────────────────────────────────────────────────────────────────
pub fn precision_at_k(results: &RankedResults, qrels: &Qrels, k: usize) -> f64 {
    let mut total = 0.0f64;
    let mut evaluated = 0usize;

    for (qid, ranked_docs) in results {
        let rel_docs = match qrels.get(qid) {
            Some(r) if !r.is_empty() => r,
            _ => continue,
        };
        evaluated += 1;
        let top_k: Vec<_> = ranked_docs.iter().take(k).collect();
        if top_k.is_empty() {
            continue;
        }
        let hits = top_k.iter().filter(|did| rel_docs.contains_key(**did)).count();
        total += hits as f64 / top_k.len() as f64;
    }
    if evaluated == 0 { 0.0 } else { total / evaluated as f64 }
}

// ─────────────────────────────────────────────────────────────────────────────
// F1@k  —  Harmonic mean of Precision@k and Recall@k
//
// F1@k = 2 * P@k * R@k / (P@k + R@k)
// ─────────────────────────────────────────────────────────────────────────────
pub fn f1_at_k(results: &RankedResults, qrels: &Qrels, k: usize) -> f64 {
    let p = precision_at_k(results, qrels, k);
    let r = recall_at_k(results, qrels, k);
    if p + r == 0.0 { 0.0 } else { 2.0 * p * r / (p + r) }
}

// ─────────────────────────────────────────────────────────────────────────────
// Print a full metrics table for common k values
// ─────────────────────────────────────────────────────────────────────────────
pub fn print_all_metrics(results: &RankedResults, qrels: &Qrels) {
    println!("\n{:<15} {:>10} {:>10} {:>10} {:>10} {:>10}",
        "Metric", "@1", "@5", "@10", "@20", "@100");
    println!("{}", "-".repeat(65));

    let ks = [1usize, 5, 10, 20, 100];

    macro_rules! row {
        ($label:expr, $func:ident) => {
            print!("{:<15}", $label);
            for &k in &ks {
                print!(" {:>10.4}", $func(results, qrels, k));
            }
            println!();
        };
    }

    row!("MRR",       mrr_at_k);
    row!("NDCG",      ndcg_at_k);
    row!("Recall",    recall_at_k);
    row!("Precision", precision_at_k);
    row!("F1",        f1_at_k);

    println!("\nTotal queries evaluated: {}", results.len());
}
