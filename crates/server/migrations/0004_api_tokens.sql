-- Jetons d'accès personnels (IA connectées par MCP) : limités à un workspace,
-- seul le hash SHA-256 du jeton est stocké.
CREATE TABLE api_tokens (
    id           UUID PRIMARY KEY,
    token_hash   BYTEA NOT NULL UNIQUE,
    user_id      UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    workspace_id UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ,
    expires_at   TIMESTAMPTZ
);
CREATE INDEX api_tokens_user_workspace_idx ON api_tokens (user_id, workspace_id);
