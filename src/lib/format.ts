// Pretty-print a tool name for display. MCP tools use double-underscore namespacing,
// e.g. "mcp__archer-db__query" → "archer-db / query".
export function prettyToolName(name: string): string {
  if (!name.startsWith("mcp__")) return name;
  const parts = name.slice(5).split("__");
  if (parts.length < 2) return name.slice(5);
  const server = parts[0];
  const method = parts.slice(1).join(".");
  return `${server} / ${method}`;
}

// Compact token count formatting: 1234567 → "1.2M", 12345 → "12.3k"
export function formatTokenCompact(tokens: number): string {
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(1)}M`;
  if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}k`;
  return `${tokens}`;
}

// Friendly model labels: "claude-opus-4-7" → "Opus 4.7"
export function formatModel(model: string): string {
  const match = model.match(/claude-(opus|sonnet|haiku)-(\d+)-(\d+)/);
  if (!match) return model;
  const family = match[1].charAt(0).toUpperCase() + match[1].slice(1);
  return `${family} ${match[2]}.${match[3]}`;
}
