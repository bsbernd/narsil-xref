//! Git integration tool handlers

use anyhow::Result;
use serde_json::Value;

use super::{ArgExtractor, ToolHandler};
use crate::git::{ChangeQuery, CommitFilter};
use crate::index::CodeIntelEngine;

/// Contributor rows returned when the caller passes no `limit`. Lower than the
/// generic list default: the list is ranked, so the tail is never the answer.
const DEFAULT_CONTRIBUTOR_LIMIT: u64 = 30;

/// get_branch_info rows (modified files, unpushed commits) returned when the
/// caller passes no `limit`. Matches get_recent_changes' render cap for the
/// same commit-list shape.
const DEFAULT_BRANCH_INFO_LIMIT: u64 = 20;

/// Handler for get_blame tool
pub struct GetBlameHandler;

#[async_trait::async_trait]
impl ToolHandler for GetBlameHandler {
    fn name(&self) -> &'static str {
        "get_blame"
    }

    async fn execute(&self, engine: &CodeIntelEngine, args: Value) -> Result<String> {
        let repo = args.get_str("repo").unwrap_or("");
        let path = args.get_str("path").unwrap_or("");
        let start_line = args.get_u64("start_line").map(|v| v as usize);
        let end_line = args.get_u64("end_line").map(|v| v as usize);
        let revision = args.get_str("revision");
        engine
            .get_blame(repo, path, start_line, end_line, revision)
            .await
    }
}

/// Handler for get_file_history tool
pub struct GetFileHistoryHandler;

#[async_trait::async_trait]
impl ToolHandler for GetFileHistoryHandler {
    fn name(&self) -> &'static str {
        "get_file_history"
    }

    async fn execute(&self, engine: &CodeIntelEngine, args: Value) -> Result<String> {
        let repo = args.get_str("repo").unwrap_or("");
        let path = args.get_str("path").unwrap_or("");
        let max_commits = args.get_u64_or("max_commits", 20) as usize;
        engine.get_file_history(repo, path, max_commits).await
    }
}

/// Handler for get_recent_changes tool
pub struct GetRecentChangesHandler;

#[async_trait::async_trait]
impl ToolHandler for GetRecentChangesHandler {
    fn name(&self) -> &'static str {
        "get_recent_changes"
    }

    async fn execute(&self, engine: &CodeIntelEngine, args: Value) -> Result<String> {
        let repo = args.get_str("repo").unwrap_or("");
        let days = args.get_u64_or("days", 7) as u32;
        let window = super::list_window(&args, DEFAULT_BRANCH_INFO_LIMIT);
        engine.get_recent_changes(repo, days, window).await
    }
}

pub struct GetCommitRangeHandler;

#[async_trait::async_trait]
impl ToolHandler for GetCommitRangeHandler {
    fn name(&self) -> &'static str {
        "get_commit_range"
    }

    async fn execute(&self, engine: &CodeIntelEngine, args: Value) -> Result<String> {
        let repo = args.get_str("repo").unwrap_or("");
        let base = args.get_str("base").filter(|base| !base.is_empty());
        let branch = args.get_str("branch").unwrap_or("HEAD");
        let paths: Vec<String> = args
            .get_array("paths")
            .into_iter()
            .flatten()
            .filter_map(|path| path.as_str().map(str::to_string))
            .collect();
        let change = match args.get_str("query").filter(|query| !query.is_empty()) {
            None => None,
            Some(query) => match args.get_str_or("mode", "text").as_str() {
                "text" => Some(ChangeQuery::Text(query)),
                "regex" => Some(ChangeQuery::Regex(query)),
                other => {
                    return Err(anyhow::anyhow!(
                        "get_commit_range: mode must be 'text' or 'regex', got '{other}'"
                    ))
                }
            },
        };
        let filter = CommitFilter {
            paths: &paths,
            change,
            message: args
                .get_str("message")
                .filter(|message| !message.is_empty()),
        };
        let include_body = args.get_bool_or("include_body", false);
        let window = super::list_window(&args, crate::response_budget::DEFAULT_LIST_LIMIT as u64);
        engine
            .get_commit_range(repo, base, branch, &filter, include_body, window)
            .await
    }
}

/// Handler for get_hotspots tool
pub struct GetHotspotsHandler;

#[async_trait::async_trait]
impl ToolHandler for GetHotspotsHandler {
    fn name(&self) -> &'static str {
        "get_hotspots"
    }

    async fn execute(&self, engine: &CodeIntelEngine, args: Value) -> Result<String> {
        let repo = args.get_str("repo").unwrap_or("");
        let days = args.get_u64_or("days", 30) as u32;
        let min_complexity = args.get_u64("min_complexity").map(|v| v as usize);
        engine.get_hotspots(repo, days, min_complexity).await
    }
}

/// Handler for get_contributors tool
pub struct GetContributorsHandler;

#[async_trait::async_trait]
impl ToolHandler for GetContributorsHandler {
    fn name(&self) -> &'static str {
        "get_contributors"
    }

    async fn execute(&self, engine: &CodeIntelEngine, args: Value) -> Result<String> {
        let repo = args.get_str("repo").unwrap_or("");
        let path = args.get_str("path");
        let window = super::list_window(&args, DEFAULT_CONTRIBUTOR_LIMIT);
        engine.get_contributors(repo, path, window).await
    }
}

/// Handler for get_commit_diff tool
pub struct GetCommitDiffHandler;

#[async_trait::async_trait]
impl ToolHandler for GetCommitDiffHandler {
    fn name(&self) -> &'static str {
        "get_commit_diff"
    }

    async fn execute(&self, engine: &CodeIntelEngine, args: Value) -> Result<String> {
        let repo = args.get_str("repo").unwrap_or("");
        // An empty commit reaches git as `show ''`, whose "ambiguous argument"
        // error names neither the tool nor the argument that was missing.
        let commit = super::require_arg(&args, "commit", "get_commit_diff")?;
        let base = args.get_str("base");
        let path = args.get_str("path");
        let max_bytes = args.get_u64("max_bytes").map(|v| v as usize);
        let context_lines = args.get_u64("context_lines").map(|v| v as usize);
        engine
            .get_commit_diff(repo, commit, base, path, max_bytes, context_lines)
            .await
    }
}

/// Handler for get_symbol_history tool
pub struct GetSymbolHistoryHandler;

#[async_trait::async_trait]
impl ToolHandler for GetSymbolHistoryHandler {
    fn name(&self) -> &'static str {
        "get_symbol_history"
    }

    async fn execute(&self, engine: &CodeIntelEngine, args: Value) -> Result<String> {
        let repo = args.get_str("repo").unwrap_or("");
        let path = args.get_str("path").unwrap_or("");
        let symbol = args.get_str("symbol").unwrap_or("");
        let max_commits = args.get_u64_or("max_commits", 10) as usize;
        engine
            .get_symbol_history(repo, path, symbol, max_commits)
            .await
    }
}

/// Handler for get_branch_info tool
pub struct GetBranchInfoHandler;

#[async_trait::async_trait]
impl ToolHandler for GetBranchInfoHandler {
    fn name(&self) -> &'static str {
        "get_branch_info"
    }

    async fn execute(&self, engine: &CodeIntelEngine, args: Value) -> Result<String> {
        let repo = args.get_str("repo").unwrap_or("");
        let window = super::list_window(&args, DEFAULT_BRANCH_INFO_LIMIT);
        let branches = args.get_bool_or("branches", false);
        engine.get_branch_info(repo, window, branches).await
    }
}

/// Handler for get_modified_files tool
pub struct GetModifiedFilesHandler;

#[async_trait::async_trait]
impl ToolHandler for GetModifiedFilesHandler {
    fn name(&self) -> &'static str {
        "get_modified_files"
    }

    async fn execute(&self, engine: &CodeIntelEngine, args: Value) -> Result<String> {
        let repo = args.get_str("repo").unwrap_or("");
        engine.get_modified_files(repo).await
    }
}
