-- Comptes utilisateurs
CREATE TABLE users (
    id               UUID PRIMARY KEY,
    email            TEXT NOT NULL,
    email_normalized TEXT NOT NULL UNIQUE,
    display_name     TEXT NOT NULL,
    password_hash    TEXT NOT NULL,
    failed_logins    INTEGER NOT NULL DEFAULT 0,
    locked_until     TIMESTAMPTZ,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    password_changed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Sessions : seul le hash SHA-256 du jeton est stocké.
CREATE TABLE sessions (
    token_hash   BYTEA PRIMARY KEY,
    user_id      UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at   TIMESTAMPTZ NOT NULL,
    user_agent   TEXT,
    ip           TEXT
);
CREATE INDEX sessions_user_id_idx ON sessions (user_id);
CREATE INDEX sessions_expires_at_idx ON sessions (expires_at);

CREATE TABLE workspaces (
    id         UUID PRIMARY KEY,
    name       TEXT NOT NULL,
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE workspace_members (
    workspace_id UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    user_id      UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role         TEXT NOT NULL CHECK (role IN ('owner', 'member')),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, user_id)
);
CREATE INDEX workspace_members_user_id_idx ON workspace_members (user_id);

CREATE TABLE workflows (
    id           UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    definition   JSONB NOT NULL,
    version      INTEGER NOT NULL DEFAULT 1,
    created_by   UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by   UUID REFERENCES users (id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX workflows_workspace_id_idx ON workflows (workspace_id);
