//! MCP protocol decision (Phase 2).
//!
//! **Choice: hand-rolled `nexql-proto` over `rmcp` for the Phase 2 stdio surface.**
//!
//! Rationale:
//! - `NexqlMcpServer.ts` is already a working hand-rolled reference for initialize /
//!   tools / errors / version negotiation.
//! - Layering rule: `nexql-tools` must not depend on transport; a thin hand-rolled
//!   loop in `nexql-proto` + binary wiring preserves that boundary cleanly.
//! - `rmcp` 2.x does expose elicitation / completions / progress — revisit when
//!   Phase 4 wires those capabilities rather than block Phase 2 catalog tools.
