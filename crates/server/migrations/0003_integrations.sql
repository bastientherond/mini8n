-- Identifiants des intégrations (bases de données, SMTP, webhooks de messagerie).
-- `secret` : JSON complet chiffré (AES-256-GCM, nonce || texte chiffré) ;
-- `summary` : champs non secrets affichables (hôte, utilisateur…).
CREATE TABLE credentials (
    id           UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    kind         TEXT NOT NULL,
    secret       BYTEA NOT NULL,
    summary      JSONB NOT NULL DEFAULT '{}',
    created_by   UUID REFERENCES users (id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, name)
);

-- Fichiers du workspace (créés par les workflows ou téléversés).
CREATE TABLE files (
    id           UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    content_type TEXT NOT NULL,
    size         BIGINT NOT NULL,
    content      BYTEA NOT NULL,
    created_by   UUID REFERENCES users (id) ON DELETE SET NULL,
    execution_id UUID,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX files_workspace_idx ON files (workspace_id, created_at DESC);
