//! AWS observability access: CloudWatch Logs + X-Ray (lands in M2).
//!
//! Responsibilities:
//! - Load AWS config via the standard credential chain (profiles, SSO).
//! - Tail/search log groups (`FilterLogEvents`, Logs Insights `StartQuery`).
//! - Fetch trace summaries and full traces (`GetTraceSummaries`, `BatchGetTraces`).
//!
//! See `docs/PRD.md` for the product context.
