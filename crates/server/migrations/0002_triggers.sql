-- Activation des workflows (déclencheurs automatiques).
ALTER TABLE workflows ADD COLUMN active BOOLEAN NOT NULL DEFAULT false;

-- Déclencheurs des workflows actifs (index reconstruit à chaque activation / sauvegarde).
CREATE TABLE triggers (
    id           UUID PRIMARY KEY,
    workflow_id  UUID NOT NULL REFERENCES workflows (id) ON DELETE CASCADE,
    workspace_id UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    node_id      TEXT NOT NULL,
    kind         TEXT NOT NULL CHECK (kind IN ('manual', 'schedule', 'webhook', 'form', 'poll', 'workflow', 'error')),
    -- Jeton secret des URL publiques (webhook, formulaire).
    token        TEXT UNIQUE,
    config       JSONB NOT NULL DEFAULT '{}',
    -- État persistant (éléments déjà vus par la surveillance d'API).
    state        JSONB NOT NULL DEFAULT '{}',
    next_run_at  TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workflow_id, node_id)
);
CREATE INDEX triggers_due_idx ON triggers (next_run_at) WHERE next_run_at IS NOT NULL;
CREATE INDEX triggers_error_idx ON triggers (workspace_id) WHERE kind = 'error';

-- Historique des exécutions côté serveur.
CREATE TABLE executions (
    id            UUID PRIMARY KEY,
    workspace_id  UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    workflow_id   UUID REFERENCES workflows (id) ON DELETE SET NULL,
    workflow_name TEXT NOT NULL,
    trigger_kind  TEXT NOT NULL,
    trigger_node  TEXT,
    parent_id     UUID REFERENCES executions (id) ON DELETE SET NULL,
    status        TEXT NOT NULL CHECK (status IN ('running', 'success', 'error')),
    error         TEXT,
    started_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at   TIMESTAMPTZ,
    duration_ms   DOUBLE PRECISION,
    report        JSONB,
    output        JSONB
);
CREATE INDEX executions_workspace_idx ON executions (workspace_id, started_at DESC);
CREATE INDEX executions_workflow_idx ON executions (workflow_id, started_at DESC);
