//! Audiobook data as the app uses it, read from Audiobookshelf's JSON.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One book in the library list.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct BookSummary {
    pub id: String,
    pub title: String,
    pub author: String,
    pub narrator: String,
    pub series: String,
    pub duration_secs: f64,
    /// Unix milliseconds.
    pub added_at: i64,
    pub has_cover: bool,
}

/// Where the listener is in a book.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct BookProgress {
    pub current_secs: f64,
    /// 0.0–1.0.
    pub progress: f32,
    pub finished: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chapter {
    pub title: String,
    pub start_secs: f64,
    pub end_secs: f64,
}

/// One book with what its page shows.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct BookDetail {
    pub summary: BookSummary,
    pub description: String,
    pub chapters: Vec<Chapter>,
}

/// Everything the Books screen lists.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct BooksLibrary {
    pub books: Vec<BookSummary>,
    pub progress: HashMap<String, BookProgress>,
    /// Book ids in "continue listening" order.
    pub continue_ids: Vec<String>,
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

/// A library item (full or minified) as a book.
pub fn book_summary(item: &Value) -> Option<BookSummary> {
    let media = &item["media"];
    let meta = &media["metadata"];
    Some(BookSummary {
        id: item["id"].as_str()?.to_string(),
        title: s(&meta["title"]),
        author: meta["authorName"]
            .as_str()
            .map(str::to_string)
            .or_else(|| {
                // Expanded items list authors instead of authorName.
                meta["authors"].as_array().map(|a| {
                    a.iter()
                        .map(|x| s(&x["name"]))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
            })
            .unwrap_or_default(),
        narrator: meta["narratorName"]
            .as_str()
            .map(str::to_string)
            .or_else(|| {
                meta["narrators"].as_array().map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
            })
            .unwrap_or_default(),
        series: meta["seriesName"]
            .as_str()
            .map(str::to_string)
            .or_else(|| {
                meta["series"].as_array().and_then(|a| {
                    a.first().map(|x| {
                        let name = s(&x["name"]);
                        match x["sequence"].as_str() {
                            Some(seq) if !seq.is_empty() => format!("{name} #{seq}"),
                            _ => name,
                        }
                    })
                })
            })
            .unwrap_or_default(),
        duration_secs: media["duration"].as_f64().unwrap_or_default(),
        added_at: item["addedAt"].as_i64().unwrap_or_default(),
        has_cover: !media["coverPath"].is_null(),
    })
}

/// `GET /api/libraries/:id/items` → books and the total count.
pub fn parse_items(page: &Value) -> (Vec<BookSummary>, usize) {
    let books = page["results"]
        .as_array()
        .map(|r| r.iter().filter_map(book_summary).collect())
        .unwrap_or_default();
    (books, page["total"].as_u64().unwrap_or_default() as usize)
}

/// A media-progress object (from `/api/me` or `/api/me/progress/:id`).
pub fn parse_progress(p: &Value) -> Option<(String, BookProgress)> {
    Some((
        p["libraryItemId"].as_str()?.to_string(),
        BookProgress {
            current_secs: p["currentTime"].as_f64().unwrap_or_default(),
            progress: p["progress"].as_f64().unwrap_or_default() as f32,
            finished: p["isFinished"].as_bool().unwrap_or_default(),
        },
    ))
}

/// The "continue-listening" shelf from `GET /api/libraries/:id/personalized`.
pub fn parse_continue_ids(shelves: &Value) -> Vec<String> {
    shelves
        .as_array()
        .into_iter()
        .flatten()
        .find(|shelf| shelf["id"] == "continue-listening")
        .and_then(|shelf| shelf["entities"].as_array())
        .map(|e| {
            e.iter()
                .filter_map(|x| x["id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// `GET /api/items/:id?expanded=1` → the book page.
pub fn parse_detail(item: &Value) -> Option<BookDetail> {
    let media = &item["media"];
    Some(BookDetail {
        summary: book_summary(item)?,
        description: s(&media["metadata"]["description"]),
        chapters: media["chapters"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| Chapter {
                title: s(&c["title"]),
                start_secs: c["start"].as_f64().unwrap_or_default(),
                end_secs: c["end"].as_f64().unwrap_or_default(),
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/abs/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn library_items_become_books() {
        let (books, total) = parse_items(&fixture("items.json"));
        assert_eq!(total, 2);
        let test_book = books.iter().find(|b| b.title == "The Test Book").unwrap();
        assert_eq!(test_book.author, "Jane Doe");
        assert_eq!(test_book.duration_secs, 180.0);
        assert!(!test_book.has_cover);
    }

    #[test]
    fn an_expanded_item_gives_chapters() {
        let detail = parse_detail(&fixture("item.json")).unwrap();
        assert_eq!(detail.summary.author, "Jane Doe");
        let titles: Vec<&str> = detail.chapters.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["Chapter One", "Chapter Two", "Chapter Three"]);
        assert_eq!(detail.chapters[1].start_secs, 60.0);
    }

    #[test]
    fn progress_and_continue_listening_are_read() {
        let (id, p) = parse_progress(&fixture("progress.json")).unwrap();
        assert_eq!(id, "ecc70163-7034-4a56-a15b-db2f31568a32");
        assert_eq!(p.current_secs, 80.0);
        assert!(!p.finished);
        assert_eq!(parse_continue_ids(&fixture("personalized.json")), [id]);
    }
}
