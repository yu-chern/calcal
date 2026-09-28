CREATE TABLE conversations (
    id uuid PRIMARY KEY,
    owner_id text NOT NULL,
    title text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX conversations_owner_updated ON conversations(owner_id, updated_at DESC);
CREATE TABLE runs (
    id uuid PRIMARY KEY,
    conversation_id uuid NOT NULL REFERENCES conversations(id),
    status text NOT NULL CHECK (status IN ('running','completed','failed')),
    reason text,
    activity text NOT NULL DEFAULT '准备开始',
    error text,
    snapshot jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    deadline_at timestamptz NOT NULL,
    finished_at timestamptz
);
CREATE UNIQUE INDEX one_active_run_per_conversation ON runs(conversation_id) WHERE status = 'running';
CREATE INDEX runs_conversation_created ON runs(conversation_id, created_at);
CREATE TABLE conversation_entries (
    sequence bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    conversation_id uuid NOT NULL REFERENCES conversations(id),
    run_id uuid NOT NULL REFERENCES runs(id),
    kind text NOT NULL,
    payload jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX entries_conversation_sequence ON conversation_entries(conversation_id, sequence);
CREATE INDEX entries_run_sequence ON conversation_entries(run_id, sequence);
