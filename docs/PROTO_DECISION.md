//! MCP protocol decision (Phase 2).
//!
//! **Choice: hand-rolled `nexql-proto` over `rmcp` for the Phase 2 stdio surface.**
//!
//! Rationale:
//! - The VS Code stdio host wiring in `NexqlMcpStdioHost.ts` / `McpDefinitionProvider.ts`
//!   already exercises initialize / tools / errors / version negotiation through the
//!   same protocol surface we ship to external clients.
//! - Layering rule: `nexql-tools` must not depend on transport; a thin hand-rolled
//!   loop in `nexql-proto` + binary wiring preserves that boundary cleanly.
//! - `rmcp` 2.x does expose elicitation / completions / progress — revisit when
//!   Phase 4 wires those capabilities rather than block Phase 2 catalog tools.
