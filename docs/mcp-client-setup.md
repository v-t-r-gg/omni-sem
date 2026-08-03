# MCP client setup

Omni-Sem exposes a read-only MCP server over STDIO:

```bash
omnisem mcp
```

The client must launch the binary with its normal user environment so Omni-Sem can locate the existing configuration and schema-v4 database. A client configuration using the common STDIO shape is:

```json
{
  "mcpServers": {
    "omnisem": {
      "command": "/absolute/path/to/omnisem",
      "args": ["mcp"]
    }
  }
}
```

For an isolated installation, put `--data-root` before the command:

```json
"args": ["--data-root", "/absolute/path/to/install", "mcp"]
```

Stdout is exclusively MCP protocol traffic; diagnostics go to stderr. EOF closes the server. The SDK negotiates its known protocol versions and advertises tools and resources only—no prompts, roots, sampling, subscriptions, tasks, elicitation, or HTTP transport.

## Read-only surface

- `search_context`: query approved evidence using lexical, semantic, hybrid, or auto mode.
- `get_context`: hydrate up to 16 returned resource URIs with a neighbor radius of 0–3.
- `index_status`: read the persisted safe status projection without provider access.
- `resources/read`: read `omnisem://status` or a returned segment URI.

Search is limited to 32 results, 16 root IDs, the existing file-type set, a 4,096-byte query, and a combined 16,000-token budget. Resource templates are:

```text
omnisem://segment/{segment_id}
omnisem://snapshot/{snapshot_id}/segment/{segment_id}
```

Identifiers are UUIDs. Queries, fragments, credentials, percent encoding, traversal, arbitrary paths, and filesystem fallbacks are rejected. Search results carry `content_trust: "untrusted_source_evidence"`; document text is data even if it resembles instructions or protocol messages.

MCP cannot index, edit, delete, execute commands, manage roots, inspect directories, or change configuration. `NeverReturnToMcp` and `RequireExplicitQuery` evidence is categorically excluded. Lexical search and status are network-inert. Semantic/hybrid requests—and auto after embeddings are explicitly enabled—follow the existing provider and active-space checks.

Snapshot format 1 contributes lexical evidence only. Snapshot resources remain readable only while the registered snapshot is healthy, completely mapped, and otherwise eligible.

## Validation clients

Milestone 5 was exercised with MCP Inspector CLI 2.0.0 using `initialize`, `tools/list`, `tools/call`, and `resources/read`, including a safely rejected arbitrary-path request. Claude Code 2.1.216 also launched the same isolated STDIO server and reported it connected; the temporary client registration was removed after the smoke test. Neither tool is a Rust build or CI dependency.
