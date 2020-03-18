CREATE TYPE change_type AS ENUM ('addition', 'modification', 'removal');

CREATE TABLE change (
    id uuid DEFAULT gen_random_uuid(),
    edition_id uuid NOT NULL,
    kind change_type NOT NULL,
    event_id uuid,

    event_kind TEXT,
    event_set TEXT,
    event_label TEXT,
    event_data jsonb,
    event_occurred_at BIGINT,
    event_created_by agent_id,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    FOREIGN KEY (edition_id) REFERENCES edition (id) ON DELETE CASCADE,
    FOREIGN KEY (event_id) REFERENCES event (id) ON DELETE CASCADE,
    PRIMARY KEY (id),

    CONSTRAINT event_data_and_event_id_present CHECK
        (
        CASE kind
            WHEN 'addition' THEN
                event_id IS NULL
                AND event_kind IS NOT NULL
                AND event_data IS NOT NULL
                AND event_occurred_at IS NOT NULL
                AND event_created_by IS NOT NULL
            WHEN 'modification' THEN
                event_id IS NOT NULL
                AND (
                    event_kind IS NOT NULL
                    OR event_set IS NOT NULL
                    OR event_label IS NOT NULL
                    OR event_data IS NOT NULL
                    OR event_occurred_at IS NOT NULL
                    OR event_created_by IS NOT NULL
                )
            WHEN 'removal' THEN
                event_id IS NOT NULL
        END
        )
);

CREATE INDEX change_edition_id_idx ON change (edition_id);
