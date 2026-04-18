// use std::env;
// use std::fs::{self, File};
// use std::io::{BufReader, BufWriter};
// use std::path::{Path, PathBuf};
// use std::process::ExitCode;
// use std::result::Result;
// use std::str;
// use std::sync::{Arc, Mutex};
// use std::thread;
// use std::time::SystemTime;

// use xml::common::{Position, TextPosition};
// use xml::reader::{EventReader, XmlEvent};

// mod model;
// use model::*;
// mod lexer;
// mod server;
// pub mod snowball;
// mod beir;    // ← NEW
// mod metrics; // ← NEW

// // ── existing file parsers (unchanged) ────────────────────────────────────────

// fn parse_entire_txt_file(file_path: &Path) -> Result<String, ()> {
//     fs::read_to_string(file_path).map_err(|err| {
//         eprintln!("ERROR: could not open file {}: {err}", file_path.display());
//     })
// }

// fn parse_entire_pdf_file(file_path: &Path) -> Result<String, ()> {
//     use poppler::Document;
//     use std::io::Read;
//     let mut content = Vec::new();
//     File::open(file_path)
//         .and_then(|mut f| f.read_to_end(&mut content))
//         .map_err(|err| eprintln!("ERROR: could not read file {}: {err}", file_path.display()))?;
//     let pdf = Document::from_data(&content, None).map_err(|err| {
//         eprintln!("ERROR: could not parse PDF {}: {err}", file_path.display());
//     })?;
//     let mut result = String::new();
//     for i in 0..pdf.n_pages() {
//         if let Some(text) = pdf.page(i).and_then(|p| p.text()) {
//             result.push_str(text.as_str());
//             result.push(' ');
//         }
//     }
//     Ok(result)
// }

// fn parse_entire_xml_file(file_path: &Path) -> Result<String, ()> {
//     let file = File::open(file_path).map_err(|err| {
//         eprintln!("ERROR: could not open file {}: {err}", file_path.display());
//     })?;
//     let er = EventReader::new(BufReader::new(file));
//     let mut content = String::new();
//     for event in er.into_iter() {
//         let event = event.map_err(|err| {
//             let TextPosition { row, column } = err.position();
//             eprintln!("{}:{}:{}: ERROR: {}", file_path.display(), row, column, err.msg());
//         })?;
//         if let XmlEvent::Characters(text) = event {
//             content.push_str(&text);
//             content.push(' ');
//         }
//     }
//     Ok(content)
// }

// fn parse_entire_file_by_extension(file_path: &Path) -> Result<String, ()> {
//     let extension = file_path
//         .extension()
//         .ok_or_else(|| eprintln!("ERROR: no extension for {}", file_path.display()))?
//         .to_string_lossy();
//     match extension.as_ref() {
//         "xhtml" | "xml" => parse_entire_xml_file(file_path),
//         "txt" | "md"    => parse_entire_txt_file(file_path),
//         "pdf"           => parse_entire_pdf_file(file_path),
//         _ => {
//             eprintln!("ERROR: unsupported extension '{extension}' for {}", file_path.display());
//             Err(())
//         }
//     }
// }

// fn save_model_as_json(model: &Model, index_path: &Path) -> Result<(), ()> {
//     println!("Saving {}...", index_path.display());
//     let index_file = File::create(index_path).map_err(|err| {
//         eprintln!("ERROR: could not create index {}: {err}", index_path.display());
//     })?;
//     serde_json::to_writer(BufWriter::new(index_file), model).map_err(|err| {
//         eprintln!("ERROR: could not serialize index {}: {err}", index_path.display());
//     })?;
//     Ok(())
// }

// fn add_folder_to_model(
//     dir_path: &Path,
//     model: Arc<Mutex<Model>>,
//     processed: &mut usize,
// ) -> Result<(), ()> {
//     let dir = fs::read_dir(dir_path).map_err(|err| {
//         eprintln!("ERROR: could not open directory {}: {err}", dir_path.display());
//     })?;
//     'next_file: for file in dir {
//         let file = file.map_err(|err| {
//             eprintln!("ERROR: could not read directory {}: {err}", dir_path.display());
//         })?;
//         let file_path = file.path();
//         let dot_file = file_path.file_name()
//             .and_then(|s| s.to_str())
//             .map(|s| s.starts_with('.'))
//             .unwrap_or(false);
//         if dot_file {
//             continue 'next_file;
//         }
//         let file_type = file.file_type().map_err(|err| {
//             eprintln!("ERROR: could not determine type of {}: {err}", file_path.display());
//         })?;
//         let last_modified = file.metadata().map_err(|err| {
//             eprintln!("ERROR: metadata of {}: {err}", file_path.display());
//         })?.modified().map_err(|err| {
//             eprintln!("ERROR: mtime of {}: {err}", file_path.display());
//         })?;
//         if file_type.is_dir() {
//             add_folder_to_model(&file_path, Arc::clone(&model), processed)?;
//             continue 'next_file;
//         }
//         let mut model = model.lock().unwrap();
//         if model.requires_reindexing(&file_path, last_modified) {
//             println!("Indexing {:?}...", &file_path);
//             let content = match parse_entire_file_by_extension(&file_path) {
//                 Ok(c) => c.chars().collect::<Vec<_>>(),
//                 Err(()) => continue 'next_file,
//             };
//             model.add_document(file_path, last_modified, &content);
//             *processed += 1;
//         }
//     }
//     Ok(())
// }

// // ── NEW: BEIR evaluation ──────────────────────────────────────────────────────

// fn run_evaluate(dataset_path: &str, split: &str) -> Result<(), ()> {
//     let base = Path::new(dataset_path);

//     // ── 1. Load corpus ────────────────────────────────────────────────────────
//     let corpus_path = base.join("corpus.jsonl");
//     println!("Loading corpus from {}...", corpus_path.display());
//     let corpus_docs = beir::load_corpus(&corpus_path).map_err(|e| {
//         eprintln!("ERROR: {e}");
//     })?;
//     println!("  {} documents loaded.", corpus_docs.len());

//     // ── 2. Index corpus into a fresh TF-IDF model ─────────────────────────────
//     //
//     // We reuse the existing Model but use each document's BEIR _id as the
//     // "path" key. SystemTime::UNIX_EPOCH is a harmless placeholder for
//     // last_modified since we never persist this model to disk.
//     println!("Indexing corpus (this may take a moment)...");
//     let mut model = Model::default();
//     let epoch = SystemTime::UNIX_EPOCH;
//     for (i, doc) in corpus_docs.iter().enumerate() {
//         if (i + 1) % 1000 == 0 {
//             println!("  Indexed {}/{}", i + 1, corpus_docs.len());
//         }
//         let content: Vec<char> = doc.full_text().chars().collect();
//         // PathBuf::from(id) lets us reuse the existing model unchanged.
//         model.add_document(PathBuf::from(&doc.id), epoch, &content);
//     }
//     println!("Indexing complete.");

//     // ── 3. Load queries ───────────────────────────────────────────────────────
//     let queries_path = base.join("queries.jsonl");
//     println!("Loading queries from {}...", queries_path.display());
//     let queries = beir::load_queries(&queries_path).map_err(|e| {
//         eprintln!("ERROR: {e}");
//     })?;
//     println!("  {} queries loaded.", queries.len());

//     // ── 4. Load relevance judgements (qrels) ──────────────────────────────────
//     let qrels_path = base.join("qrels").join(format!("{split}.tsv"));
//     println!("Loading qrels from {}...", qrels_path.display());
//     let qrels = beir::load_qrels(&qrels_path).map_err(|e| {
//         eprintln!("ERROR: {e}");
//     })?;
//     println!(
//         "  {} queries have relevance judgements.",
//         qrels.len()
//     );

//     // ── 5. Run every query and collect ranked results ─────────────────────────
//     const TOP_K: usize = 100; // retrieve up to 100 results per query
//     let mut ranked_results: metrics::RankedResults = Vec::new();
//     let queries_with_qrels: Vec<_> = queries.iter()
//         .filter(|q| qrels.contains_key(&q.id))
//         .collect();
//     println!(
//         "Evaluating {} queries (those with qrels)...",
//         queries_with_qrels.len()
//     );
//     for (i, query) in queries_with_qrels.iter().enumerate() {
//         if (i + 1) % 50 == 0 {
//             println!("  Processed {}/{}", i + 1, queries_with_qrels.len());
//         }
//         let query_chars: Vec<char> = query.text.chars().collect();
//         let ranked_doc_ids = model.search_query_ids(&query_chars, TOP_K);
//         ranked_results.push((query.id.clone(), ranked_doc_ids));
//     }

//     // ── 6. Compute and print all metrics ──────────────────────────────────────
//     println!("\n══════════════════════════════════════════════════════════════");
//     println!("  TF-IDF Evaluation on BEIR dataset: {}", dataset_path);
//     println!("  Split: {split}  |  Corpus size: {}  |  Queries: {}",
//         corpus_docs.len(), ranked_results.len());
//     println!("══════════════════════════════════════════════════════════════");
//     metrics::print_all_metrics(&ranked_results, &qrels);

//     // ── 7. Save results to JSON (for comparison with other models later) ───────
//     let results_path = format!("{dataset_path}/tfidf_results.json");
//     save_results_json(&ranked_results, &results_path)?;
//     println!("\nRaw results saved to: {results_path}");
//     println!("(You can use this file to compare with BM25, BERT, etc.)");

//     Ok(())
// }

// fn save_results_json(
//     results: &metrics::RankedResults,
//     path: &str,
// ) -> Result<(), ()> {
//     let file = File::create(path).map_err(|e| {
//         eprintln!("ERROR: could not create results file {path}: {e}");
//     })?;
//     serde_json::to_writer_pretty(BufWriter::new(file), results).map_err(|e| {
//         eprintln!("ERROR: could not write results JSON: {e}");
//     })?;
//     Ok(())
// }

// // ── CLI entry point ───────────────────────────────────────────────────────────

// fn usage(program: &str) {
//     eprintln!("Usage: {program} [SUBCOMMAND] [OPTIONS]");
//     eprintln!("Subcommands:");
//     eprintln!("    serve    <folder> [address]    start HTTP server with web UI");
//     eprintln!("    evaluate <dataset> [split]     evaluate on BEIR dataset");
//     eprintln!("                                   split defaults to 'test'");
// }

// fn entry() -> Result<(), ()> {
//     let mut args = env::args();
//     let program = args.next().expect("path to program is provided");
//     let subcommand = args.next().ok_or_else(|| {
//         usage(&program);
//         eprintln!("ERROR: no subcommand provided");
//     })?;

//     match subcommand.as_str() {
//         "serve" => {
//             let dir_path = args.next().ok_or_else(|| {
//                 usage(&program);
//                 eprintln!("ERROR: no directory provided for serve");
//             })?;
//             let mut index_path = Path::new(&dir_path).to_path_buf();
//             index_path.push(".ir.json");
//             let address = args.next().unwrap_or_else(|| "127.0.0.1:6060".to_string());

//             let exists = index_path.try_exists().map_err(|err| {
//                 eprintln!("ERROR: could not check {}: {err}", index_path.display());
//             })?;
//             let model: Arc<Mutex<Model>> = if exists {
//                 let index_file = File::open(&index_path).map_err(|err| {
//                     eprintln!("ERROR: could not open {}: {err}", index_path.display());
//                 })?;
//                 Arc::new(Mutex::new(
//                     serde_json::from_reader(index_file).map_err(|err| {
//                         eprintln!("ERROR: could not parse {}: {err}", index_path.display());
//                     })?,
//                 ))
//             } else {
//                 Arc::new(Mutex::new(Model::default()))
//             };
//             {
//                 let model = Arc::clone(&model);
//                 thread::spawn(move || {
//                     let mut processed = 0;
//                     add_folder_to_model(Path::new(&dir_path), Arc::clone(&model), &mut processed)
//                         .unwrap();
//                     if processed > 0 {
//                         let model = model.lock().unwrap();
//                         save_model_as_json(&model, &index_path).unwrap();
//                     }
//                     println!("Finished indexing");
//                 });
//             }
//             server::start(&address, Arc::clone(&model))
//         }

//         "evaluate" => {   // ← NEW SUBCOMMAND
//             let dataset_path = args.next().ok_or_else(|| {
//                 usage(&program);
//                 eprintln!("ERROR: no dataset path provided for evaluate");
//             })?;
//             let split = args.next().unwrap_or_else(|| "test".to_string());
//             run_evaluate(&dataset_path, &split)
//         }

//         _ => {
//             usage(&program);
//             eprintln!("ERROR: unknown subcommand '{subcommand}'");
//             Err(())
//         }
//     }
// }

// fn main() -> ExitCode {
//     match entry() {
//         Ok(()) => ExitCode::SUCCESS,
//         Err(()) => ExitCode::FAILURE,
//     }
// }
use std::env;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::result::Result;
use std::str;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::SystemTime;

use xml::common::{Position, TextPosition};
use xml::reader::{EventReader, XmlEvent};

mod model;
use model::Model;

mod bm25_model;
use bm25_model::Bm25Model;

mod beir;
mod lexer;
mod metrics;
mod server;
pub mod snowball;

// ── File parsers ──────────────────────────────────────────────────────────────

fn parse_entire_txt_file(file_path: &Path) -> Result<String, ()> {
    fs::read_to_string(file_path).map_err(|err| {
        eprintln!("ERROR: could not open file {}: {err}", file_path.display());
    })
}

fn parse_entire_pdf_file(file_path: &Path) -> Result<String, ()> {
    use poppler::Document;
    use std::io::Read;
    let mut content = Vec::new();
    File::open(file_path)
        .and_then(|mut f| f.read_to_end(&mut content))
        .map_err(|err| {
            eprintln!("ERROR: could not read file {}: {err}", file_path.display());
        })?;
    let pdf = Document::from_data(&content, None).map_err(|err| {
        eprintln!("ERROR: could not parse PDF {}: {err}", file_path.display());
    })?;
    let mut result = String::new();
    for i in 0..pdf.n_pages() {
        if let Some(text) = pdf.page(i).and_then(|p| p.text()) {
            result.push_str(text.as_str());
            result.push(' ');
        }
    }
    Ok(result)
}

fn parse_entire_xml_file(file_path: &Path) -> Result<String, ()> {
    let file = File::open(file_path).map_err(|err| {
        eprintln!("ERROR: could not open file {}: {err}", file_path.display());
    })?;
    let er = EventReader::new(BufReader::new(file));
    let mut content = String::new();
    for event in er.into_iter() {
        let event = event.map_err(|err| {
            let TextPosition { row, column } = err.position();
            eprintln!(
                "{}:{}:{}: ERROR: {}",
                file_path.display(),
                row,
                column,
                err.msg()
            );
        })?;
        if let XmlEvent::Characters(text) = event {
            content.push_str(&text);
            content.push(' ');
        }
    }
    Ok(content)
}

fn parse_entire_file_by_extension(file_path: &Path) -> Result<String, ()> {
    let extension = file_path
        .extension()
        .ok_or_else(|| {
            eprintln!("ERROR: no extension for {}", file_path.display());
        })?
        .to_string_lossy();
    match extension.as_ref() {
        "xhtml" | "xml" => parse_entire_xml_file(file_path),
        "txt" | "md" => parse_entire_txt_file(file_path),
        "pdf" => parse_entire_pdf_file(file_path),
        _ => {
            eprintln!(
                "ERROR: unsupported extension '{extension}' for {}",
                file_path.display()
            );
            Err(())
        }
    }
}

fn save_model_as_json(model: &Model, index_path: &Path) -> Result<(), ()> {
    println!("Saving {}...", index_path.display());
    let index_file = File::create(index_path).map_err(|err| {
        eprintln!(
            "ERROR: could not create index {}: {err}",
            index_path.display()
        );
    })?;
    serde_json::to_writer(BufWriter::new(index_file), model).map_err(|err| {
        eprintln!(
            "ERROR: could not serialize index {}: {err}",
            index_path.display()
        );
    })?;
    Ok(())
}

fn add_folder_to_model(
    dir_path: &Path,
    model: Arc<Mutex<Model>>,
    processed: &mut usize,
) -> Result<(), ()> {
    let dir = fs::read_dir(dir_path).map_err(|err| {
        eprintln!(
            "ERROR: could not open directory {}: {err}",
            dir_path.display()
        );
    })?;
    'next_file: for file in dir {
        let file = file.map_err(|err| {
            eprintln!(
                "ERROR: could not read directory {}: {err}",
                dir_path.display()
            );
        })?;
        let file_path = file.path();
        let dot_file = file_path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.starts_with('.'))
            .unwrap_or(false);
        if dot_file {
            continue 'next_file;
        }
        let file_type = file.file_type().map_err(|err| {
            eprintln!(
                "ERROR: could not determine type of {}: {err}",
                file_path.display()
            );
        })?;
        let last_modified = file
            .metadata()
            .map_err(|err| {
                eprintln!("ERROR: metadata of {}: {err}", file_path.display());
            })?
            .modified()
            .map_err(|err| {
                eprintln!("ERROR: mtime of {}: {err}", file_path.display());
            })?;
        if file_type.is_dir() {
            add_folder_to_model(&file_path, Arc::clone(&model), processed)?;
            continue 'next_file;
        }
        let mut model = model.lock().unwrap();
        if model.requires_reindexing(&file_path, last_modified) {
            println!("Indexing {:?}...", &file_path);
            let content = match parse_entire_file_by_extension(&file_path) {
                Ok(c) => c.chars().collect::<Vec<_>>(),
                Err(()) => continue 'next_file,
            };
            model.add_document(file_path, last_modified, &content);
            *processed += 1;
        }
    }
    Ok(())
}

// ── Shared helpers ────────────────────────────────────────────────────────────

fn save_results_json(results: &metrics::RankedResults, path: &str) -> Result<(), ()> {
    let file = File::create(path).map_err(|e| {
        eprintln!("ERROR: could not create results file {path}: {e}");
    })?;
    serde_json::to_writer_pretty(BufWriter::new(file), results).map_err(|e| {
        eprintln!("ERROR: could not write results JSON: {e}");
    })?;
    Ok(())
}

// ── TF-IDF evaluation ─────────────────────────────────────────────────────────

fn run_evaluate(dataset_path: &str, split: &str) -> Result<(), ()> {
    let base = Path::new(dataset_path);

    println!(
        "Loading corpus from {}...",
        base.join("corpus.jsonl").display()
    );
    let corpus_docs =
        beir::load_corpus(&base.join("corpus.jsonl")).map_err(|e| eprintln!("ERROR: {e}"))?;
    println!("  {} documents loaded.", corpus_docs.len());

    println!("Indexing corpus with TF-IDF...");
    let mut model = Model::default();
    let epoch = SystemTime::UNIX_EPOCH;
    for (i, doc) in corpus_docs.iter().enumerate() {
        if (i + 1) % 1000 == 0 {
            println!("  Indexed {}/{}", i + 1, corpus_docs.len());
        }
        let content: Vec<char> = doc.full_text().chars().collect();
        model.add_document(PathBuf::from(&doc.id), epoch, &content);
    }
    println!("Indexing complete.");

    println!(
        "Loading queries from {}...",
        base.join("queries.jsonl").display()
    );
    let queries =
        beir::load_queries(&base.join("queries.jsonl")).map_err(|e| eprintln!("ERROR: {e}"))?;
    println!("  {} queries loaded.", queries.len());

    let qrels_path = base.join("qrels").join(format!("{split}.tsv"));
    println!("Loading qrels from {}...", qrels_path.display());
    let qrels = beir::load_qrels(&qrels_path).map_err(|e| eprintln!("ERROR: {e}"))?;
    println!("  {} queries have relevance judgements.", qrels.len());

    const TOP_K: usize = 100;
    let mut ranked_results: metrics::RankedResults = Vec::new();
    let queries_with_qrels: Vec<_> = queries
        .iter()
        .filter(|q| qrels.contains_key(&q.id))
        .collect();
    println!("Evaluating {} queries...", queries_with_qrels.len());
    for (i, query) in queries_with_qrels.iter().enumerate() {
        if (i + 1) % 50 == 0 {
            println!("  Processed {}/{}", i + 1, queries_with_qrels.len());
        }
        let qchars: Vec<char> = query.text.chars().collect();
        ranked_results.push((query.id.clone(), model.search_query_ids(&qchars, TOP_K)));
    }

    println!("\n══════════════════════════════════════════════════════════════");
    println!("  TF-IDF Evaluation on BEIR dataset: {}", dataset_path);
    println!(
        "  Split: {split}  |  Corpus size: {}  |  Queries: {}",
        corpus_docs.len(),
        ranked_results.len()
    );
    println!("══════════════════════════════════════════════════════════════");
    metrics::print_all_metrics(&ranked_results, &qrels);

    let results_path = format!("{dataset_path}/tfidf_results.json");
    save_results_json(&ranked_results, &results_path)?;
    println!("\nRaw results saved to: {results_path}");

    Ok(())
}

// ── BM25 evaluation ───────────────────────────────────────────────────────────

fn run_evaluate_bm25(dataset_path: &str, split: &str) -> Result<(), ()> {
    let base = Path::new(dataset_path);

    println!(
        "Loading corpus from {}...",
        base.join("corpus.jsonl").display()
    );
    let corpus_docs =
        beir::load_corpus(&base.join("corpus.jsonl")).map_err(|e| eprintln!("ERROR: {e}"))?;
    println!("  {} documents loaded.", corpus_docs.len());

    println!("Indexing corpus with BM25...");
    let mut model = Bm25Model::default();
    let epoch = SystemTime::UNIX_EPOCH;
    for (i, doc) in corpus_docs.iter().enumerate() {
        if (i + 1) % 1000 == 0 {
            println!("  Indexed {}/{}", i + 1, corpus_docs.len());
        }
        let content: Vec<char> = doc.full_text().chars().collect();
        model.add_document(PathBuf::from(&doc.id), epoch, &content);
    }
    println!(
        "Indexing complete. Average document length: {:.1} tokens",
        model.avg_doc_len()
    );

    println!(
        "Loading queries from {}...",
        base.join("queries.jsonl").display()
    );
    let queries =
        beir::load_queries(&base.join("queries.jsonl")).map_err(|e| eprintln!("ERROR: {e}"))?;
    println!("  {} queries loaded.", queries.len());

    let qrels_path = base.join("qrels").join(format!("{split}.tsv"));
    println!("Loading qrels from {}...", qrels_path.display());
    let qrels = beir::load_qrels(&qrels_path).map_err(|e| eprintln!("ERROR: {e}"))?;
    println!("  {} queries have relevance judgements.", qrels.len());

    const TOP_K: usize = 100;
    let mut ranked_results: metrics::RankedResults = Vec::new();
    let queries_with_qrels: Vec<_> = queries
        .iter()
        .filter(|q| qrels.contains_key(&q.id))
        .collect();
    println!("Evaluating {} queries...", queries_with_qrels.len());
    for (i, query) in queries_with_qrels.iter().enumerate() {
        if (i + 1) % 50 == 0 {
            println!("  Processed {}/{}", i + 1, queries_with_qrels.len());
        }
        let qchars: Vec<char> = query.text.chars().collect();
        ranked_results.push((query.id.clone(), model.search_query_ids(&qchars, TOP_K)));
    }

    println!("\n══════════════════════════════════════════════════════════════");
    println!("  BM25 Evaluation on BEIR dataset: {}", dataset_path);
    println!(
        "  Split: {split}  |  Corpus size: {}  |  Queries: {}",
        corpus_docs.len(),
        ranked_results.len()
    );
    println!("══════════════════════════════════════════════════════════════");
    metrics::print_all_metrics(&ranked_results, &qrels);

    let results_path = format!("{dataset_path}/bm25_results.json");
    save_results_json(&ranked_results, &results_path)?;
    println!("\nRaw results saved to: {results_path}");

    Ok(())
}

// ── CLI ───────────────────────────────────────────────────────────────────────

fn usage(program: &str) {
    eprintln!("Usage: {program} [SUBCOMMAND] [OPTIONS]");
    eprintln!("Subcommands:");
    eprintln!("    serve         <folder> [address]   start HTTP server with web UI");
    eprintln!("    evaluate      <dataset> [split]    evaluate TF-IDF on BEIR dataset");
    eprintln!("    evaluate-bm25 <dataset> [split]    evaluate BM25   on BEIR dataset");
}

fn entry() -> Result<(), ()> {
    let mut args = env::args();
    let program = args.next().expect("path to program is provided");
    let subcommand = args.next().ok_or_else(|| {
        usage(&program);
        eprintln!("ERROR: no subcommand provided");
    })?;

    match subcommand.as_str() {
        "serve" => {
            let dir_path = args.next().ok_or_else(|| {
                usage(&program);
                eprintln!("ERROR: no directory provided for serve");
            })?;
            let mut index_path = Path::new(&dir_path).to_path_buf();
            index_path.push(".ir.json");
            let address = args.next().unwrap_or_else(|| "127.0.0.1:6060".to_string());

            let exists = index_path.try_exists().map_err(|err| {
                eprintln!("ERROR: could not check {}: {err}", index_path.display());
            })?;
            let model: Arc<Mutex<Model>> = if exists {
                let index_file = File::open(&index_path).map_err(|err| {
                    eprintln!("ERROR: could not open {}: {err}", index_path.display());
                })?;
                Arc::new(Mutex::new(serde_json::from_reader(index_file).map_err(
                    |err| {
                        eprintln!("ERROR: could not parse {}: {err}", index_path.display());
                    },
                )?))
            } else {
                Arc::new(Mutex::new(Model::default()))
            };
            {
                let model = Arc::clone(&model);
                thread::spawn(move || {
                    let mut processed = 0;
                    add_folder_to_model(Path::new(&dir_path), Arc::clone(&model), &mut processed)
                        .unwrap();
                    if processed > 0 {
                        let model = model.lock().unwrap();
                        save_model_as_json(&model, &index_path).unwrap();
                    }
                    println!("Finished indexing");
                });
            }
            server::start(&address, Arc::clone(&model))
        }

        "evaluate" => {
            let dataset_path = args.next().ok_or_else(|| {
                usage(&program);
                eprintln!("ERROR: no dataset path provided for evaluate");
            })?;
            let split = args.next().unwrap_or_else(|| "test".to_string());
            run_evaluate(&dataset_path, &split)
        }

        "evaluate-bm25" => {
            let dataset_path = args.next().ok_or_else(|| {
                usage(&program);
                eprintln!("ERROR: no dataset path provided for evaluate-bm25");
            })?;
            let split = args.next().unwrap_or_else(|| "test".to_string());
            run_evaluate_bm25(&dataset_path, &split)
        }

        _ => {
            usage(&program);
            eprintln!("ERROR: unknown subcommand '{subcommand}'");
            Err(())
        }
    }
}

fn main() -> ExitCode {
    match entry() {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => ExitCode::FAILURE,
    }
}
