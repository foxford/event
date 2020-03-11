CREATE TYPE change_type AS ENUM ('addition', 'modification', 'removal');

CREATE TABLE change (
    id uuid default gen_random_uuid(),
    edition_id uuid NOT NULL,
    kind change_type NOT NULL,
    data jsonb NOT NULL DEFAULT '{}'::jsonb,
    event_id uuid,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    FOREIGN KEY (edition_id) REFERENCES edition (id) ON DELETE CASCADE,
    FOREIGN KEY (event_id) REFERENCES event (id) ON DELETE CASCADE,
    PRIMARY KEY (id)
);

CREATE INDEX change_edition_id_idx ON change (edition_id);
