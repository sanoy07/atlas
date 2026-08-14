pub mod c_structural;
pub mod python_structural;
pub mod rust_structural;
pub mod gh_json;
pub mod git_log;
pub mod git_renames;
pub mod ts_structural;

/// Per-file outcome from a language extractor.  Emitted alongside the
/// structural edges so `ingest_X` can write an authoritative
/// `files.analysis_status` for every source file the extractor attempted.
///
/// This is what distinguishes `analyzed` (extractor ran successfully — zero
/// edges is a legitimate answer) from `parser_failure` (extractor tried but
/// could not process the file).  Silently skipping a read failure would
/// look identical to a real "no imports" result, which is exactly the
/// gap this type closes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileAnalysis {
    /// Repository-relative path.
    pub file:   String,
    pub status: FileAnalysisStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileAnalysisStatus {
    /// Extractor read and parsed the file successfully (edges may be zero).
    Analyzed,
    /// Extractor attempted the file but could not process it.  `reason`
    /// is a short human-readable string (e.g. "invalid utf-8", "read error").
    ParserFailure { reason: String },
}
